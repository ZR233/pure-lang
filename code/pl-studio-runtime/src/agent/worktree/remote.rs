use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_core::remote::SshManager;
use pl_core::tool::{ExecutionBackend, ExecutionOutput, ExecutionRequest, GitPolicy};

use super::backend::{changed_files, checked_output, non_empty_head};
use super::{WorktreeBackend, WorktreeCreateFailure, WorktreeError, WorktreeStatus};

const WORKTREE_GIT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct RemoteWorktreeBackend {
    transport: Arc<dyn RemoteWorktreeTransport>,
    repo_root: PathBuf,
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
    repo_root: PathBuf,
}

impl SshRemoteWorktreeTransport {
    async fn host(&self) -> Result<pl_core::remote::RemoteWorkspaceHost, String> {
        self.ssh_manager
            .open_workspace_host(
                &self.server_id,
                self.repo_root.to_string_lossy().into_owned(),
            )
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
    ) -> Self {
        let server_id = server_id.into();
        Self {
            transport: Arc::new(SshRemoteWorktreeTransport {
                ssh_manager,
                server_id,
                repo_root: repo_root.clone(),
            }),
            repo_root,
            policy: GitPolicy::default(),
        }
    }

    async fn run_git(
        &self,
        cwd: &Path,
        args: &[String],
    ) -> Result<pl_core::ExecutionOutput, WorktreeError> {
        let mut full_args = vec![
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "-c".to_string(),
            format!("safe.directory={}", cwd.display()),
            "-c".to_string(),
            "credential.helper=".to_string(),
        ];
        full_args.extend_from_slice(args);
        self.transport
            .run(ExecutionRequest {
                program: PathBuf::from("git"),
                args: full_args,
                cwd: cwd.to_path_buf(),
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
        path.strip_prefix(&self.repo_root)
            .map_err(|_| {
                WorktreeError::InvalidResource(format!(
                    "{} is outside {}",
                    path.display(),
                    self.repo_root.display()
                ))
            })
            .map(|path| path.to_string_lossy().replace('\\', "/"))
    }
}

impl WorktreeBackend for RemoteWorktreeBackend {
    fn resolve_repo_root<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<PathBuf, WorktreeError>> {
        async move {
            let args = vec!["rev-parse".to_string(), "--show-toplevel".to_string()];
            let output = checked_output(&args, self.run_git(path, &args).await?)?;
            non_empty_head(&args, &output.stdout).map(PathBuf::from)
        }
        .boxed()
    }

    fn create_parent<'a>(
        &'a self,
        _repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            let parent = target_path.parent().ok_or_else(|| {
                WorktreeError::InvalidResource("worktree target has no parent".to_string())
            })?;
            self.transport
                .create_directory(self.relative_path(parent)?)
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
                target_path.to_string_lossy().into_owned(),
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
            args.push(target_path.to_string_lossy().into_owned());
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug, Default)]
    struct RecordingTransport {
        requests: Mutex<Vec<ExecutionRequest>>,
        directories: Mutex<Vec<String>>,
        removals: Mutex<Vec<String>>,
    }

    impl RemoteWorktreeTransport for RecordingTransport {
        fn run<'a>(
            &'a self,
            request: ExecutionRequest,
        ) -> BoxFuture<'a, Result<ExecutionOutput, String>> {
            async move {
                let stdout = if request.args.ends_with(&["rev-parse".into(), "HEAD".into()]) {
                    "base-commit\n"
                } else {
                    ""
                };
                self.requests.lock().unwrap().push(request);
                Ok(ExecutionOutput {
                    status: 0,
                    stdout: stdout.to_string(),
                    stderr: String::new(),
                })
            }
            .boxed()
        }

        fn create_directory<'a>(
            &'a self,
            relative_path: String,
        ) -> BoxFuture<'a, Result<(), String>> {
            async move {
                self.directories.lock().unwrap().push(relative_path);
                Ok(())
            }
            .boxed()
        }

        fn path_exists<'a>(
            &'a self,
            _relative_path: String,
        ) -> BoxFuture<'a, Result<bool, String>> {
            async { Ok(true) }.boxed()
        }

        fn remove_path<'a>(&'a self, relative_path: String) -> BoxFuture<'a, Result<(), String>> {
            async move {
                self.removals.lock().unwrap().push(relative_path);
                Ok(())
            }
            .boxed()
        }
    }

    fn backend(transport: Arc<RecordingTransport>) -> RemoteWorktreeBackend {
        RemoteWorktreeBackend {
            transport,
            repo_root: PathBuf::from("/repo"),
            policy: GitPolicy::default(),
        }
    }

    #[tokio::test]
    async fn ssh_backend_uses_safe_timed_git_for_create_and_cleanup() {
        let transport = Arc::new(RecordingTransport::default());
        let backend = backend(transport.clone());
        let target = PathBuf::from("/repo/.pure/worktrees/root/child");

        backend
            .create_parent(Path::new("/repo"), &target)
            .await
            .unwrap();
        backend
            .create(
                Path::new("/repo"),
                "pure-agent-child",
                &target,
                "base-commit",
            )
            .await
            .unwrap();
        backend
            .remove(Path::new("/repo"), &target, true)
            .await
            .unwrap();
        backend
            .remove_leaf(Path::new("/repo"), &target)
            .await
            .unwrap();
        backend
            .delete_branch(Path::new("/repo"), "pure-agent-child")
            .await
            .unwrap();

        assert_eq!(
            transport.directories.lock().unwrap().as_slice(),
            [".pure/worktrees/root"]
        );
        assert_eq!(
            transport.removals.lock().unwrap().as_slice(),
            [".pure/worktrees/root/child"]
        );
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for request in requests.iter() {
            assert_eq!(request.program, PathBuf::from("git"));
            assert_eq!(request.timeout, Some(Duration::from_secs(120)));
            assert!(
                request
                    .args
                    .windows(2)
                    .any(|args| args == ["-c", "core.hooksPath=/dev/null"])
            );
            assert!(
                request
                    .args
                    .windows(2)
                    .any(|args| args == ["-c", "credential.helper="])
            );
        }
        assert!(requests[0].args.ends_with(&[
            "worktree".into(),
            "add".into(),
            "-b".into(),
            "pure-agent-child".into(),
            "/repo/.pure/worktrees/root/child".into(),
            "base-commit".into(),
        ]));
        assert!(requests[1].args.ends_with(&[
            "worktree".into(),
            "remove".into(),
            "--force".into(),
            "/repo/.pure/worktrees/root/child".into(),
        ]));
        assert!(requests[2].args.ends_with(&[
            "branch".into(),
            "-D".into(),
            "pure-agent-child".into(),
        ]));
    }
}
