use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_core::tool::{
    ExecutionOutput, ExecutionRequest, GitPolicy, LocalExecutionBackend, LocalExecutionFailure,
};

const WORKTREE_GIT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("invalid worktree resource: {0}")]
    InvalidResource(String),
    #[error("unsafe git branch `{0}`")]
    UnsafeBranch(String),
    #[error("failed to launch git {args}: {message}")]
    GitLaunchFailed { args: String, message: String },
    #[error("git {args} timed out")]
    GitTimedOut { args: String },
    #[error("git {args} exited with {exit_code}: {stderr}")]
    GitExited {
        args: String,
        exit_code: i32,
        stderr: String,
    },
    #[error("git {args} status is unknown: {stderr}")]
    GitStatusUnknown { args: String, stderr: String },
    #[error("worktree io error: {0}")]
    Io(String),
    #[error("{operation}; rollback succeeded")]
    OperationFailedAfterCleanup { operation: Box<WorktreeError> },
    #[error("{operation}; rollback failed: {cleanup}")]
    OperationFailedWithCleanup {
        operation: Box<WorktreeError>,
        cleanup: Box<WorktreeError>,
    },
    #[error("{context} cleanup failed: {failures:?}")]
    CleanupFailed {
        context: String,
        failures: Vec<WorktreeError>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeCreateFailureDisposition {
    NoSideEffects,
    MayHaveCreated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeStatus {
    pub head: String,
    pub changed_files: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct WorktreeCreateFailure {
    error: WorktreeError,
    disposition: WorktreeCreateFailureDisposition,
}

impl WorktreeCreateFailure {
    pub fn no_side_effects(error: WorktreeError) -> Self {
        Self {
            error,
            disposition: WorktreeCreateFailureDisposition::NoSideEffects,
        }
    }

    pub fn may_have_created(error: WorktreeError) -> Self {
        Self {
            error,
            disposition: WorktreeCreateFailureDisposition::MayHaveCreated,
        }
    }

    pub fn disposition(&self) -> WorktreeCreateFailureDisposition {
        self.disposition
    }

    pub fn into_error(self) -> WorktreeError {
        self.error
    }
}

pub trait WorktreeBackend: fmt::Debug + Send + Sync {
    fn resolve_repo_root<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<PathBuf, WorktreeError>>;

    fn create_parent<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>>;

    fn path_exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<bool, WorktreeError>>;

    fn remove_leaf<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>>;

    fn create<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
        target_path: &'a Path,
        base_commit: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeCreateFailure>>;

    fn resolve_head<'a>(
        &'a self,
        worktree_path: &'a Path,
    ) -> BoxFuture<'a, Result<String, WorktreeError>>;

    fn status<'a>(
        &'a self,
        worktree_path: &'a Path,
    ) -> BoxFuture<'a, Result<WorktreeStatus, WorktreeError>>;

    fn remove<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
        force: bool,
    ) -> BoxFuture<'a, Result<(), WorktreeError>>;

    fn delete_branch<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>>;
}

#[derive(Debug, Clone)]
pub struct LocalWorktreeBackend {
    backend: LocalExecutionBackend,
    policy: GitPolicy,
}

impl Default for LocalWorktreeBackend {
    fn default() -> Self {
        Self {
            backend: LocalExecutionBackend,
            policy: GitPolicy::default(),
        }
    }
}

impl LocalWorktreeBackend {
    async fn run_git_output(
        &self,
        cwd: &Path,
        args: &[String],
    ) -> Result<ExecutionOutput, WorktreeError> {
        let request = git_request(cwd, args);
        let arguments = args.join(" ");
        self.backend
            .run_classified(request)
            .await
            .map_err(|failure| match failure {
                LocalExecutionFailure::BeforeSpawn(message) => WorktreeError::GitLaunchFailed {
                    args: arguments,
                    message,
                },
                LocalExecutionFailure::AfterSpawn(stderr) => WorktreeError::GitStatusUnknown {
                    args: arguments,
                    stderr,
                },
                LocalExecutionFailure::TimedOut => WorktreeError::GitTimedOut { args: arguments },
            })
    }

    async fn run_git(&self, cwd: &Path, args: &[String]) -> Result<ExecutionOutput, WorktreeError> {
        checked_output(args, self.run_git_output(cwd, args).await?)
    }
}

impl WorktreeBackend for LocalWorktreeBackend {
    fn resolve_repo_root<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<PathBuf, WorktreeError>> {
        async move {
            let args = vec!["rev-parse".to_string(), "--show-toplevel".to_string()];
            let output = self.run_git(path, &args).await?;
            let root = non_empty_head(&args, &output.stdout)?;
            std::fs::canonicalize(root).map_err(|error| WorktreeError::Io(error.to_string()))
        }
        .boxed()
    }

    fn create_parent<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            pl_core::path_safety::validate_path_for_write(repo_root, target_path)
                .map_err(|error| WorktreeError::InvalidResource(error.to_string()))?;
            let parent = target_path.parent().ok_or_else(|| {
                WorktreeError::InvalidResource("worktree target has no parent".to_string())
            })?;
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| WorktreeError::Io(error.to_string()))
        }
        .boxed()
    }

    fn path_exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<bool, WorktreeError>> {
        async move {
            match tokio::fs::symlink_metadata(path).await {
                Ok(_) => Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(WorktreeError::Io(error.to_string())),
            }
        }
        .boxed()
    }

    fn remove_leaf<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            pl_core::path_safety::remove_dir_all_no_follow_async(
                &repo_root.join(".pure/worktrees"),
                target_path,
            )
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
            let output =
                self.run_git_output(repo_root, &args)
                    .await
                    .map_err(|error| match error {
                        WorktreeError::GitLaunchFailed { .. } => {
                            WorktreeCreateFailure::no_side_effects(error)
                        }
                        _ => WorktreeCreateFailure::may_have_created(error),
                    })?;
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
            let output = self.run_git(worktree_path, &args).await?;
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
            let output = self.run_git(worktree_path, &args).await?;
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
            self.run_git(repo_root, &args).await.map(|_| ())
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
            self.run_git(repo_root, &args).await.map(|_| ())
        }
        .boxed()
    }
}

pub(super) fn git_request(cwd: &Path, args: &[String]) -> ExecutionRequest {
    let mut full_args = vec![
        "-c".to_string(),
        "core.hooksPath=/dev/null".to_string(),
        "-c".to_string(),
        format!("safe.directory={}", cwd.display()),
        "-c".to_string(),
        "credential.helper=".to_string(),
    ];
    full_args.extend_from_slice(args);
    ExecutionRequest {
        program: PathBuf::from("git"),
        args: full_args,
        cwd: cwd.to_path_buf(),
        env: BTreeMap::new(),
        timeout: Some(WORKTREE_GIT_TIMEOUT),
    }
}

pub(super) fn checked_output(
    args: &[String],
    output: ExecutionOutput,
) -> Result<ExecutionOutput, WorktreeError> {
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

pub(super) fn non_empty_head(args: &[String], stdout: &str) -> Result<String, WorktreeError> {
    let head = stdout.trim();
    if head.is_empty() {
        Err(WorktreeError::GitStatusUnknown {
            args: args.join(" "),
            stderr: "git rev-parse HEAD returned an empty value".to_string(),
        })
    } else {
        Ok(head.to_string())
    }
}

pub(super) fn changed_files(stdout: &str) -> Vec<String> {
    let mut files = stdout
        .lines()
        .filter_map(|line| line.get(3..))
        .map(|path| path.rsplit_once(" -> ").map_or(path, |(_, path)| path))
        .map(str::to_string)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}
