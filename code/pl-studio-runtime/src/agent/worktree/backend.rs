use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_core::tool::{
    ExecutionOutput, ExecutionRequest, GitPolicy, LocalExecutionBackend, LocalExecutionFailure,
};

use super::error::WorktreeError;

/// worktree create 失败是否可能拥有本次 spec 创建的资源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateFailureDisposition {
    /// 失败发生在产生副作用之前，不得按 spec 清理。
    NoSideEffects,
    /// create 已启动且结果不确定，或 backend 明确知道已产生部分资源。
    MayHaveCreated,
}

/// 带资源所有权 disposition 的 worktree create 失败。
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct WorktreeCreateFailure {
    #[source]
    error: WorktreeError,
    disposition: CreateFailureDisposition,
}

impl WorktreeCreateFailure {
    /// 构造明确未产生副作用的 create 失败。
    pub fn no_side_effects(error: WorktreeError) -> Self {
        Self {
            error,
            disposition: CreateFailureDisposition::NoSideEffects,
        }
    }

    /// 构造可能已产生本次 spec 资源的 create 失败。
    pub fn may_have_created(error: WorktreeError) -> Self {
        Self {
            error,
            disposition: CreateFailureDisposition::MayHaveCreated,
        }
    }

    /// 返回 manager 应采用的补偿清理策略。
    pub(crate) fn disposition(&self) -> CreateFailureDisposition {
        self.disposition
    }

    /// 取出原始 worktree 错误。
    pub fn into_error(self) -> WorktreeError {
        self.error
    }
}

/// worktree 底层执行端口。
///
/// 封装 `git worktree add/remove` 与 branch cleanup。默认实现
/// [`LocalWorktreeBackend`] 复用 pl-core 的 [`LocalExecutionBackend`] shell out
/// `git`。需要自定义执行环境（如容器内 git）的宿主可注入自己的实现。
pub trait WorktreeBackend: fmt::Debug + Send + Sync {
    /// 为 worktree 目标创建父目录。
    fn create_parent<'a>(
        &'a self,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            if let Some(parent) = target_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| WorktreeError::Io(error.to_string()))?;
            }
            Ok(())
        }
        .boxed()
    }

    /// 检查 worktree leaf 是否仍存在。
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

    /// 删除 worktree leaf，且不得跟随越界链接。
    fn remove_leaf<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            let worktree_root = repo_root.join(".pure/worktrees");
            pl_core::path_safety::remove_dir_all_no_follow_async(&worktree_root, target_path)
                .await
                .map_err(|error| WorktreeError::Io(error.to_string()))
        }
        .boxed()
    }

    /// 在 `target_path` 创建基于 `repo_root`、分支 `branch` 的 worktree。
    ///
    /// 实现必须区分明确无副作用的失败与可能已创建本次 spec 资源的失败；manager 只会
    /// 对 [`WorktreeCreateFailure::may_have_created`] 构造的失败执行补偿清理。
    fn create<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
        target_path: &'a Path,
        base_commit: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeCreateFailure>>;

    /// Resolve the commit actually checked out in a newly created worktree.
    fn resolve_head<'a>(
        &'a self,
        worktree_path: &'a Path,
    ) -> BoxFuture<'a, Result<String, WorktreeError>>;

    /// 移除 worktree；`force` 为真时忽略未提交改动。
    fn remove<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
        force: bool,
    ) -> BoxFuture<'a, Result<(), WorktreeError>>;

    /// 删除本地分支。
    fn delete_branch<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>>;
}

/// worktree git 命令超时。
const WORKTREE_GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// 基于本地 `git` 二进制的默认 worktree 后端。
#[derive(Debug, Clone)]
pub struct LocalWorktreeBackend {
    backend: LocalExecutionBackend,
    policy: GitPolicy,
    git_binary: PathBuf,
}

impl Default for LocalWorktreeBackend {
    fn default() -> Self {
        Self {
            backend: LocalExecutionBackend,
            policy: GitPolicy::default(),
            git_binary: PathBuf::from("git"),
        }
    }
}

impl LocalWorktreeBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// 执行 git 命令并要求退出码为 0。
    async fn run_git(&self, cwd: &Path, args: &[String]) -> Result<ExecutionOutput, WorktreeError> {
        let output = self.run_git_output(cwd, args).await?;
        if output.status != 0 {
            return Err(WorktreeError::GitExited {
                args: args.join(" "),
                exit_code: output.status,
                stderr: output.stderr,
            });
        }
        Ok(output)
    }

    /// 执行 git 命令并返回原始输出，不检查退出码。
    async fn run_git_output(
        &self,
        cwd: &Path,
        args: &[String],
    ) -> Result<ExecutionOutput, WorktreeError> {
        let request = self.git_request(cwd, args);
        let args = args.join(" ");
        self.backend
            .run_classified(request)
            .await
            .map_err(|failure| match failure {
                LocalExecutionFailure::BeforeSpawn(message) => {
                    WorktreeError::GitLaunchFailed { args, message }
                }
                LocalExecutionFailure::AfterSpawn(message) => WorktreeError::GitStatusUnknown {
                    args,
                    stderr: message,
                },
                LocalExecutionFailure::TimedOut => WorktreeError::GitTimedOut { args },
            })
    }

    async fn create_worktree(
        &self,
        repo_root: &Path,
        args: &[String],
    ) -> Result<(), WorktreeCreateFailure> {
        let request = self.git_request(repo_root, args);
        let output = self
            .backend
            .run_classified(request)
            .await
            .map_err(|failure| {
                let disposition = match &failure {
                    LocalExecutionFailure::BeforeSpawn(_) => {
                        CreateFailureDisposition::NoSideEffects
                    }
                    LocalExecutionFailure::AfterSpawn(_) | LocalExecutionFailure::TimedOut => {
                        CreateFailureDisposition::MayHaveCreated
                    }
                };
                let arguments = args.join(" ");
                let error = match failure {
                    LocalExecutionFailure::BeforeSpawn(message) => WorktreeError::GitLaunchFailed {
                        args: arguments,
                        message,
                    },
                    LocalExecutionFailure::AfterSpawn(message) => WorktreeError::GitStatusUnknown {
                        args: arguments,
                        stderr: message,
                    },
                    LocalExecutionFailure::TimedOut => {
                        WorktreeError::GitTimedOut { args: arguments }
                    }
                };
                match disposition {
                    CreateFailureDisposition::NoSideEffects => {
                        WorktreeCreateFailure::no_side_effects(error)
                    }
                    CreateFailureDisposition::MayHaveCreated => {
                        WorktreeCreateFailure::may_have_created(error)
                    }
                }
            })?;
        if output.status == -1 {
            return Err(WorktreeCreateFailure::may_have_created(
                WorktreeError::GitStatusUnknown {
                    args: args.join(" "),
                    stderr: output.stderr,
                },
            ));
        }
        if output.status != 0 {
            return Err(WorktreeCreateFailure::no_side_effects(
                WorktreeError::GitExited {
                    args: args.join(" "),
                    exit_code: output.status,
                    stderr: output.stderr,
                },
            ));
        }
        Ok(())
    }

    fn git_request(&self, cwd: &Path, args: &[String]) -> ExecutionRequest {
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
            program: self.git_binary.clone(),
            args: full_args,
            cwd: cwd.to_path_buf(),
            env: BTreeMap::new(),
            timeout: Some(WORKTREE_GIT_TIMEOUT),
        }
    }
}

impl WorktreeBackend for LocalWorktreeBackend {
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
            let target = target_path.to_string_lossy().to_string();
            let args: Vec<String> = [
                "worktree",
                "add",
                "-b",
                branch,
                target.as_str(),
                base_commit,
            ]
            .iter()
            .map(|item| item.to_string())
            .collect();
            self.create_worktree(repo_root, &args).await
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
            let target = target_path.to_string_lossy().to_string();
            let mut args: Vec<String> = vec!["worktree".to_string(), "remove".to_string()];
            if force {
                args.push("--force".to_string());
            }
            args.push(target);
            self.run_git(repo_root, &args).await?;
            Ok(())
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

    fn delete_branch<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        async move {
            self.policy
                .validate_branch(branch)
                .map_err(|_| WorktreeError::UnsafeBranch(branch.to_string()))?;
            let args: Vec<String> =
                vec!["branch".to_string(), "-D".to_string(), branch.to_string()];
            self.run_git(repo_root, &args).await?;
            Ok(())
        }
        .boxed()
    }
}
