//! Local workspace LSP host: filesystem primitives plus supervised language servers.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_lsp::host::{
    LspHostBackend, LspHostError, LspHostFileStat, LspHostProcess, LspHostProcessExit,
    LspHostSpawnRequest,
};

use crate::command::LocalCommandBackend;
use crate::workspace::ToolPathPolicy;
use crate::workspace::path_safety::real_directory_entries_async;

/// 本地 workspace 的 LSP 宿主。
///
/// 文件原语在 workspace 边界内直接读取本地文件系统；language server 交给本地命令后端
/// 的 per-child 监督 owner 启动，因此进程树随宿主生存，并在终止时按
/// `SIGTERM → 2s → SIGKILL` 整树回收，而不是只结束 leader。
#[derive(Debug, Clone)]
pub struct LocalLspHostBackend {
    policy: ToolPathPolicy,
    commands: LocalCommandBackend,
}

impl LocalLspHostBackend {
    /// 构造绑定到 `workspace_root`、以 `worker_executable` 为监督者的本地 LSP 宿主。
    ///
    /// `worker_executable` 由宿主从既有 remote helper 资源注入，不在本层回退到 PATH 或
    /// 未监督 spawn。
    ///
    /// # Errors
    /// workspace 根无法解析为本地真实目录时返回错误。
    pub fn new<P>(
        workspace_root: impl Into<PathBuf>,
        worker_executable: P,
    ) -> Result<Self, LspHostError>
    where
        P: AsRef<Path> + std::fmt::Debug + Send + Sync + 'static,
    {
        let workspace_root = workspace_root.into();
        let policy =
            ToolPathPolicy::new(workspace_root.clone(), false, "lsp_host").map_err(host_error)?;
        let commands =
            LocalCommandBackend::new(workspace_root).with_worker_executable(worker_executable);
        Ok(Self { policy, commands })
    }
}

impl LspHostBackend for LocalLspHostBackend {
    fn identity(&self) -> String {
        format!("local:{}", self.policy.root().display())
    }

    fn read_file<'a>(
        &'a self,
        path: &'a Path,
        max_bytes: u64,
    ) -> BoxFuture<'a, Result<Vec<u8>, LspHostError>> {
        async move {
            use tokio::io::AsyncReadExt;
            let resolved = self.resolve_existing(path)?;
            let file = tokio::fs::File::open(&resolved).await.map_err(host_error)?;
            let metadata = file.metadata().await.map_err(host_error)?;
            if !metadata.is_file() {
                return Err(LspHostError::new(format!(
                    "LSP document is not a file: {}",
                    resolved.display()
                )));
            }
            // Bounded read: never pull more than one byte past the limit, so the source
            // cannot force an unbounded allocation between the stat check and the read.
            let limit = max_bytes.saturating_add(1);
            let mut reader = file.take(limit);
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await.map_err(host_error)?;
            if bytes.len() as u64 > max_bytes {
                return Err(LspHostError::new(format!(
                    "LSP document {} exceeds the {max_bytes} byte source limit",
                    resolved.display()
                )));
            }
            Ok(bytes)
        }
        .boxed()
    }

    fn stat<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<Option<LspHostFileStat>, LspHostError>> {
        async move {
            match self.resolve_existing(path) {
                Ok(resolved) => {
                    let metadata = tokio::fs::metadata(&resolved).await.map_err(host_error)?;
                    Ok(Some(LspHostFileStat {
                        is_file: metadata.is_file(),
                        byte_size: metadata.len(),
                    }))
                }
                // `resolve_existing` conflates "absent" with boundary/permission failures.
                // Only a genuinely missing target is a `None`; every other failure — an
                // out-of-workspace path, a symlink boundary, or a permission error — must
                // surface instead of being hidden as "no host fact".
                Err(error) => {
                    match tokio::fs::symlink_metadata(self.policy.candidate(path)).await {
                        Err(io) if io.kind() == std::io::ErrorKind::NotFound => Ok(None),
                        _ => Err(error),
                    }
                }
            }
        }
        .boxed()
    }

    fn list_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<Vec<String>, LspHostError>> {
        async move {
            // Contract matches the remote host and the trait doc: first-level *names*
            // (not paths), excluding link/reparse entries, for `glob_match` detection.
            let display = path.display().to_string();
            let resolved = self
                .policy
                .resolve_existing_directory(path, &display)
                .map_err(host_error)?;
            let entries = real_directory_entries_async(&resolved)
                .await
                .map_err(host_error)?;
            Ok(entries
                .into_iter()
                .filter_map(|entry| {
                    entry
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_string)
                })
                .collect())
        }
        .boxed()
    }

    fn spawn<'a>(
        &'a self,
        request: LspHostSpawnRequest,
    ) -> BoxFuture<'a, Result<LspHostProcess, LspHostError>> {
        async move {
            let mut process = self
                .commands
                .spawn_argv(OsStr::new(&request.program), &request.args, &request.cwd)
                .await
                .map_err(host_error)?;
            let stdin = process.take_stdin();
            let stdout = process.take_stdout();
            let stderr = process.take_stderr();
            let cancellation = process.cancellation();
            Ok(LspHostProcess::new(
                stdin,
                stdout,
                stderr,
                async move {
                    process
                        .wait()
                        .await
                        .map(|exit| LspHostProcessExit {
                            exit_code: exit.exit_code,
                        })
                        .map_err(host_error)
                },
                move || {
                    async move {
                        cancellation.cancel();
                    }
                    .boxed()
                },
            ))
        }
        .boxed()
    }
}

impl LocalLspHostBackend {
    fn resolve_existing(&self, path: &Path) -> Result<PathBuf, LspHostError> {
        self.policy
            .resolve_existing_path(path, &path.display().to_string())
            .map_err(host_error)
    }
}

fn host_error(error: impl std::fmt::Display) -> LspHostError {
    LspHostError::new(error.to_string())
}
