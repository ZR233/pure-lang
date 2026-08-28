use std::path::{Path, PathBuf};

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_lsp::{
    LspHostBackend, LspHostError, LspHostFileStat, LspHostProcess, LspHostProcessExit,
    LspHostSpawnRequest,
};

use crate::tool::{
    CommandBackend, CommandOutputTarget, CommandSpawnRequest, WorkspaceFileBackend,
    WorkspaceFileReadBytesRequest, shell_quote_word,
};

use super::RemoteWorkspaceHost;

impl LspHostBackend for RemoteWorkspaceHost {
    fn identity(&self) -> String {
        format!(
            "{}:{}",
            self.files.canonical_path(),
            self.files.workspace_id()
        )
    }

    fn read_file<'a>(
        &'a self,
        path: &'a Path,
        max_bytes: u64,
    ) -> BoxFuture<'a, Result<Vec<u8>, LspHostError>> {
        async move {
            let path = self.relative_path(path)?;
            let max_bytes = usize::try_from(max_bytes)
                .map_err(|_| LspHostError::new("LSP file byte limit is unsupported"))?;
            self.files
                .read_bytes(WorkspaceFileReadBytesRequest {
                    path,
                    cwd: None,
                    max_bytes,
                })
                .await
                .map_err(host_error)
        }
        .boxed()
    }

    fn stat<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<Option<LspHostFileStat>, LspHostError>> {
        async move {
            let path = self.relative_path(path)?;
            self.files
                .stat_optional(path, None)
                .await
                .map(|stat| {
                    stat.map(|stat| LspHostFileStat {
                        is_file: stat.is_file,
                        byte_size: stat.len.unwrap_or(0),
                    })
                })
                .map_err(host_error)
        }
        .boxed()
    }

    fn list_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<Vec<String>, LspHostError>> {
        async move {
            let path = self.relative_path(path)?;
            self.files
                .list_directory_entries(path)
                .await
                .map(|listing| {
                    listing
                        .entries
                        .into_iter()
                        .filter(|entry| !entry.is_symlink)
                        .map(|entry| entry.name)
                        .collect()
                })
                .map_err(host_error)
        }
        .boxed()
    }

    fn spawn<'a>(
        &'a self,
        request: LspHostSpawnRequest,
    ) -> BoxFuture<'a, Result<LspHostProcess, LspHostError>> {
        async move {
            let command = std::iter::once(request.program)
                .chain(request.args)
                .map(|part| shell_quote_word(&part))
                .collect::<Vec<_>>()
                .join(" ");
            let cwd = self.relative_path(&request.cwd)?;
            let capture_path = PathBuf::from(".pure")
                .join("remote")
                .join("lsp")
                .join(format!("{}.log", request.process_id));
            let mut process = self
                .commands
                .spawn(CommandSpawnRequest {
                    process_id: request.process_id.clone(),
                    command,
                    cwd,
                    output_target: CommandOutputTarget::new(capture_path.clone(), capture_path),
                })
                .await
                .map_err(host_error)?;
            let stdin = process.take_stdin();
            let stdout = process.take_stdout();
            let stderr = process.take_stderr();
            let commands = self.commands.clone();
            let process_id = request.process_id;
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
                        .map_err(LspHostError::new)
                },
                move || {
                    async move {
                        commands.terminate(&process_id, None).await;
                    }
                    .boxed()
                },
            ))
        }
        .boxed()
    }
}

impl RemoteWorkspaceHost {
    fn relative_path(&self, path: &Path) -> Result<String, LspHostError> {
        relative_workspace_path(Path::new(self.files.canonical_path()), path)
    }
}

fn relative_workspace_path(root: &Path, path: &Path) -> Result<String, LspHostError> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).map_err(|_| {
            LspHostError::new(format!(
                "LSP path '{}' escapes remote workspace '{}'",
                path.display(),
                root.display()
            ))
        })?
    } else {
        path
    };
    let value = relative.to_string_lossy().replace('\\', "/");
    if value.split('/').any(|component| component == "..") {
        return Err(LspHostError::new("LSP path escapes remote workspace"));
    }
    Ok(if value.is_empty() {
        ".".to_string()
    } else {
        value
    })
}

fn host_error(error: impl std::fmt::Display) -> LspHostError {
    LspHostError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_lsp_paths_stay_inside_workspace() {
        assert_eq!(
            relative_workspace_path(
                Path::new("/srv/project"),
                Path::new("/srv/project/src/lib.rs")
            )
            .expect("relative path"),
            "src/lib.rs"
        );
        assert!(
            relative_workspace_path(Path::new("/srv/project"), Path::new("/srv/other/lib.rs"))
                .is_err()
        );
    }
}
