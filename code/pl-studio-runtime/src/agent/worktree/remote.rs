use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_tool::execution::{ExecutionBackend, ExecutionOutput, ExecutionRequest};
use pl_tool::remote::{SshManager, normalize_remote_absolute_path};

use pl_tool::git::GitPolicy;

use super::backend::{changed_files, checked_output, non_empty_head};
use super::{WorktreeBackend, WorktreeCreateFailure, WorktreeError, WorktreeStatus};

const WORKTREE_GIT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct RemoteWorktreeBackend {
    transport: Arc<dyn RemoteWorktreeTransport>,
    repo_root: String,
    policy: GitPolicy,
}

trait RemoteWorktreeTransport: std::fmt::Debug + Send + Sync {
    fn run<'a>(
        &'a self,
        request: ExecutionRequest,
    ) -> BoxFuture<'a, Result<ExecutionOutput, String>>;

    fn create_directory<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>>;

    fn path_exists<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<bool, String>>;

    fn remove_path<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>>;
}

#[derive(Debug)]
struct SshRemoteWorktreeTransport {
    ssh_manager: Arc<SshManager>,
    server_id: String,
    repo_root: String,
}

impl SshRemoteWorktreeTransport {
    async fn host(&self) -> Result<pl_tool::remote::RemoteWorkspaceHost, String> {
        self.ssh_manager
            .open_workspace_host(&self.server_id, self.repo_root.clone())
            .await
            .map_err(|error| error.to_string())
    }
}

impl RemoteWorktreeTransport for SshRemoteWorktreeTransport {
    fn run<'a>(
        &'a self,
        request: ExecutionRequest,
    ) -> BoxFuture<'a, Result<ExecutionOutput, String>> {
        async move {
            self.host()
                .await?
                .git
                .run(request)
                .await
                .map_err(|error| error.to_string())
        }
        .boxed()
    }

    fn create_directory<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>> {
        async move {
            self.host()
                .await?
                .files
                .create_directory(relative_path, None)
                .await
                .map_err(|error| error.to_string())
        }
        .boxed()
    }

    fn path_exists<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<bool, String>> {
        async move {
            self.host()
                .await?
                .files
                .stat_optional(relative_path, None)
                .await
                .map(|stat| stat.is_some())
                .map_err(|error| error.to_string())
        }
        .boxed()
    }

    fn remove_path<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>> {
        async move {
            self.host()
                .await?
                .files
                .remove_path(relative_path, None, true)
                .await
                .map_err(|error| error.to_string())
        }
        .boxed()
    }
}

impl RemoteWorktreeBackend {
    pub fn new(
        ssh_manager: Arc<SshManager>,
        server_id: impl Into<String>,
        repo_root: PathBuf,
    ) -> Result<Self, WorktreeError> {
        let server_id = server_id.into();
        let repo_root = remote_path(&repo_root)?;
        Ok(Self {
            transport: Arc::new(SshRemoteWorktreeTransport {
                ssh_manager,
                server_id,
                repo_root: repo_root.clone(),
            }),
            repo_root,
            policy: GitPolicy::default(),
        })
    }

    async fn run_git(
        &self,
        cwd: &Path,
        args: &[String],
    ) -> Result<pl_tool::execution::ExecutionOutput, WorktreeError> {
        let cwd = remote_path(cwd)?;
        let mut full_args = vec![
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "-c".to_string(),
            format!("safe.directory={cwd}"),
            "-c".to_string(),
            "credential.helper=".to_string(),
        ];
        full_args.extend_from_slice(args);
        self.transport
            .run(ExecutionRequest {
                program: PathBuf::from("git"),
                args: full_args,
                cwd: PathBuf::from(cwd),
                env: BTreeMap::new(),
                timeout: Some(WORKTREE_GIT_TIMEOUT),
            })
            .await
            .map_err(|stderr| WorktreeError::GitStatusUnknown {
                args: args.join(" "),
                stderr,
            })
    }

    fn relative_path(&self, path: &Path) -> Result<String, WorktreeError> {
        self.relative_path_text(&remote_path(path)?)
    }

    /// 把已归一化的 POSIX 路径表达成仓库根下的 workspace-relative 形式。
    ///
    /// 归一化在字符串层完成：宿主形态的 target 无法用 `Path::strip_prefix` 与仓库根匹配，
    /// 因此先把两侧都表达成 POSIX，再按目录边界剥离。这样目录/文件操作与 git 路径参数
    /// 都源自同一个 POSIX 结果。
    fn relative_path_text(&self, path: &str) -> Result<String, WorktreeError> {
        let path = normalize_remote_absolute_path(path)
            .map_err(|error| WorktreeError::InvalidResource(error.to_string()))?;
        let relative = if path == self.repo_root {
            ""
        } else if self.repo_root == "/" {
            path.strip_prefix('/').unwrap_or(&path)
        } else {
            match path.strip_prefix(&self.repo_root) {
                Some(relative) if relative.starts_with('/') => &relative[1..],
                _ => {
                    return Err(WorktreeError::InvalidResource(format!(
                        "{} is outside {}",
                        path, self.repo_root
                    )));
                }
            }
        };
        Ok(relative.to_string())
    }
}

fn remote_path(path: &Path) -> Result<String, WorktreeError> {
    normalize_remote_absolute_path(&path.to_string_lossy())
        .map_err(|error| WorktreeError::InvalidResource(error.to_string()))
}

impl WorktreeBackend for RemoteWorktreeBackend {
    fn resolve_repo_root<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<PathBuf, WorktreeError>> {
        async move {
            let args = vec!["rev-parse".to_string(), "--show-toplevel".to_string()];
            let output = checked_output(&args, self.run_git(path, &args).await?)?;
            let root = non_empty_head(&args, &output.stdout)?;
            remote_path(Path::new(&root)).map(PathBuf::from)
        }
        .boxed()
    }

    fn create_parent<'a>(
        &'a self,
        _repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            // 先归一化为 POSIX，再在 POSIX 空间取父目录：宿主 `Path::parent()` 会在归一化
            // 之前按本地分隔符切分混用形态的 target，从而算出错误的父目录。
            let target = remote_path(target_path)?;
            let parent = target
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .ok_or_else(|| {
                    WorktreeError::InvalidResource("worktree target has no parent".to_string())
                })?;
            self.transport
                .create_directory(self.relative_path_text(parent)?)
                .await
                .map_err(WorktreeError::Io)
        }
        .boxed()
    }

    fn path_exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<bool, WorktreeError>> {
        async move {
            self.transport
                .path_exists(self.relative_path(path)?)
                .await
                .map_err(WorktreeError::Io)
        }
        .boxed()
    }

    fn remove_leaf<'a>(
        &'a self,
        _repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            self.transport
                .remove_path(self.relative_path(target_path)?)
                .await
                .map_err(WorktreeError::Io)
        }
        .boxed()
    }

    fn create<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
        target_path: &'a Path,
        base_commit: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeCreateFailure>> {
        async move {
            self.policy.validate_branch(branch).map_err(|_| {
                WorktreeCreateFailure::no_side_effects(WorktreeError::UnsafeBranch(
                    branch.to_string(),
                ))
            })?;
            let args = vec![
                "worktree".to_string(),
                "add".to_string(),
                "-b".to_string(),
                branch.to_string(),
                // git 路径参数使用 POSIX 绝对路径：它与 lease 记录的 `path`、会话工作区根
                // 以及远端 workspace handle 根是同一字符串；目录/文件操作所用的
                // workspace-relative 形式是该绝对 POSIX 路径对仓库根的确定性投影。
                remote_path(target_path).map_err(WorktreeCreateFailure::no_side_effects)?,
                base_commit.to_string(),
            ];
            let output = self
                .run_git(repo_root, &args)
                .await
                .map_err(WorktreeCreateFailure::may_have_created)?;
            checked_output(&args, output)
                .map(|_| ())
                .map_err(WorktreeCreateFailure::no_side_effects)
        }
        .boxed()
    }

    fn resolve_head<'a>(
        &'a self,
        worktree_path: &'a Path,
    ) -> BoxFuture<'a, Result<String, WorktreeError>> {
        async move {
            let args = vec!["rev-parse".to_string(), "HEAD".to_string()];
            let output = checked_output(&args, self.run_git(worktree_path, &args).await?)?;
            non_empty_head(&args, &output.stdout)
        }
        .boxed()
    }

    fn status<'a>(
        &'a self,
        worktree_path: &'a Path,
    ) -> BoxFuture<'a, Result<WorktreeStatus, WorktreeError>> {
        async move {
            let head = self.resolve_head(worktree_path).await?;
            let args = vec![
                "status".to_string(),
                "--porcelain=v1".to_string(),
                "--untracked-files=all".to_string(),
            ];
            let output = checked_output(&args, self.run_git(worktree_path, &args).await?)?;
            Ok(WorktreeStatus {
                head,
                changed_files: changed_files(&output.stdout),
            })
        }
        .boxed()
    }

    fn remove<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
        force: bool,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            let mut args = vec!["worktree".to_string(), "remove".to_string()];
            if force {
                args.push("--force".to_string());
            }
            args.push(remote_path(target_path)?);
            let output = self.run_git(repo_root, &args).await?;
            checked_output(&args, output).map(|_| ())
        }
        .boxed()
    }

    fn delete_branch<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            self.policy
                .validate_branch(branch)
                .map_err(|_| WorktreeError::UnsafeBranch(branch.to_string()))?;
            let args = vec!["branch".to_string(), "-D".to_string(), branch.to_string()];
            let output = self.run_git(repo_root, &args).await?;
            checked_output(&args, output).map(|_| ())
        }
        .boxed()
    }
}
