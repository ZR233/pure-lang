use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_core::remote::SshManager;
use pl_core::tool::{ExecutionBackend, ExecutionRequest, GitPolicy};

use super::{WorktreeBackend, WorktreeCreateFailure, WorktreeError};

const WORKTREE_GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// 通过 `pl-core::remote::SshManager` 复用远端文件与执行原语的 worktree backend。
#[derive(Debug, Clone)]
pub struct RemoteWorktreeBackend {
    ssh_manager: Arc<SshManager>,
    server_id: String,
    repo_root: PathBuf,
    policy: GitPolicy,
}

impl RemoteWorktreeBackend {
    pub fn new(
        ssh_manager: Arc<SshManager>,
        server_id: impl Into<String>,
        repo_root: PathBuf,
    ) -> Self {
        Self {
            ssh_manager,
            server_id: server_id.into(),
            repo_root,
            policy: GitPolicy::default(),
        }
    }

    async fn host(&self) -> Result<pl_core::remote::RemoteWorkspaceHost, WorktreeError> {
        self.ssh_manager
            .open_workspace_host(
                &self.server_id,
                self.repo_root.to_string_lossy().into_owned(),
            )
            .await
            .map_err(|error| WorktreeError::Io(error.to_string()))
    }

    async fn run_git(
        &self,
        cwd: &Path,
        args: Vec<String>,
    ) -> Result<pl_core::ExecutionOutput, WorktreeError> {
        let host = self.host().await?;
        let mut full_args = vec![
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "-c".to_string(),
            format!("safe.directory={}", cwd.display()),
            "-c".to_string(),
            "credential.helper=".to_string(),
        ];
        full_args.extend(args.iter().cloned());
        host.git
            .run(ExecutionRequest {
                program: PathBuf::from("git"),
                args: full_args,
                cwd: cwd.to_path_buf(),
                env: BTreeMap::new(),
                timeout: Some(WORKTREE_GIT_TIMEOUT),
            })
            .await
            .map_err(|message| WorktreeError::GitStatusUnknown {
                args: args.join(" "),
                stderr: message,
            })
    }

    fn relative_path(&self, path: &Path) -> Result<String, WorktreeError> {
        path.strip_prefix(&self.repo_root)
            .map_err(|_| {
                WorktreeError::InvalidRepoRoot(format!(
                    "{} is outside {}",
                    path.display(),
                    self.repo_root.display()
                ))
            })
            .map(|path| {
                let value = path.to_string_lossy().replace('\\', "/");
                if value.is_empty() {
                    ".".to_string()
                } else {
                    value
                }
            })
    }

    fn checked_output(
        args: &[String],
        output: pl_core::ExecutionOutput,
    ) -> Result<pl_core::ExecutionOutput, WorktreeError> {
        if output.status == 0 {
            Ok(output)
        } else {
            Err(WorktreeError::GitExited {
                args: args.join(" "),
                exit_code: output.status,
                stderr: output.stderr,
            })
        }
    }
}

impl WorktreeBackend for RemoteWorktreeBackend {
    fn create_parent<'a>(
        &'a self,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            let parent = target_path
                .parent()
                .ok_or_else(|| WorktreeError::InvalidRepoRoot("worktree has no parent".into()))?;
            let relative = self.relative_path(parent)?;
            self.host()
                .await?
                .files
                .create_directory(relative, None)
                .await
                .map_err(|error| WorktreeError::Io(error.to_string()))
        }
        .boxed()
    }

    fn path_exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<bool, WorktreeError>> {
        async move {
            let relative = self.relative_path(path)?;
            self.host()
                .await?
                .files
                .stat_optional(relative, None)
                .await
                .map(|stat| stat.is_some())
                .map_err(|error| WorktreeError::Io(error.to_string()))
        }
        .boxed()
    }

    fn remove_leaf<'a>(
        &'a self,
        _repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            let relative = self.relative_path(target_path)?;
            self.host()
                .await?
                .files
                .remove_path(relative, None, true)
                .await
                .map_err(|error| WorktreeError::Io(error.to_string()))
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
                target_path.to_string_lossy().into_owned(),
                base_commit.to_string(),
            ];
            let output = self
                .run_git(repo_root, args.clone())
                .await
                .map_err(WorktreeCreateFailure::may_have_created)?;
            Self::checked_output(&args, output)
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
            let output =
                Self::checked_output(&args, self.run_git(worktree_path, args.clone()).await?)?;
            let head = output.stdout.trim();
            if head.is_empty() {
                return Err(WorktreeError::GitStatusUnknown {
                    args: args.join(" "),
                    stderr: "git rev-parse HEAD returned an empty value".to_string(),
                });
            }
            Ok(head.to_string())
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
            args.push(target_path.to_string_lossy().into_owned());
            let output = self.run_git(repo_root, args.clone()).await?;
            Self::checked_output(&args, output).map(|_| ())
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
            let output = self.run_git(repo_root, args.clone()).await?;
            Self::checked_output(&args, output).map(|_| ())
        }
        .boxed()
    }
}
