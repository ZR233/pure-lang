use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use crate::tool::{
    ExecutionBackend, ExecutionOutput, ExecutionRequest, GitPolicy, LocalExecutionBackend,
};

use super::error::WorktreeError;

/// `BoxFuture` 别名，用于让 [`WorktreeBackend`] 可作为 trait object 被非泛型的
/// `AgentSupervisor` 经 `Arc` 持有（与仓库 `AgentToolRegistrar` 同样的 dyn-friendly 风格）。
pub(super) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// `git merge` 结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOutcome {
    /// 合并成功。
    Merged,
    /// 合并冲突，worktree 不应释放。
    Conflict,
}

/// worktree 底层执行端口。
///
/// 封装 `git worktree add/remove`、兜底提交与合并。默认实现
/// [`LocalWorktreeBackend`] 复用 pl-core 的 [`LocalExecutionBackend`] shell out
/// `git`。需要自定义执行环境（如容器内 git）的宿主可注入自己的实现。
pub trait WorktreeBackend: fmt::Debug + Send + Sync {
    /// 在 `target_path` 创建基于 `repo_root`、分支 `branch` 的 worktree。
    fn create<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
        target_path: &'a Path,
        base_commit: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>>;

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

    /// 兜底提交 worktree 内全部改动。无改动时返回 `Ok(())`。
    fn commit_all<'a>(
        &'a self,
        worktree_path: &'a Path,
        message: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>>;

    /// 把 `branch` merge 到 `main_workspace` 当前分支。
    fn merge_branch<'a>(
        &'a self,
        main_workspace: &'a Path,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<MergeOutcome, WorktreeError>>;
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
            return Err(WorktreeError::GitCommand {
                args: args.join(" "),
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
        let mut full_args = vec![
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "-c".to_string(),
            format!("safe.directory={}", cwd.display()),
            "-c".to_string(),
            "credential.helper=".to_string(),
        ];
        full_args.extend_from_slice(args);
        let request = ExecutionRequest {
            program: self.git_binary.clone(),
            args: full_args,
            cwd: cwd.to_path_buf(),
            env: BTreeMap::new(),
            timeout: Some(WORKTREE_GIT_TIMEOUT),
        };
        self.backend
            .run(request)
            .await
            .map_err(|error| WorktreeError::GitCommand {
                args: args.join(" "),
                stderr: error,
            })
    }
}

impl WorktreeBackend for LocalWorktreeBackend {
    fn create<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
        target_path: &'a Path,
        base_commit: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        Box::pin(async move {
            self.policy
                .validate_branch(branch)
                .map_err(|_| WorktreeError::UnsafeBranch(branch.to_string()))?;
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
            self.run_git(repo_root, &args).await?;
            Ok(())
        })
    }

    fn remove<'a>(
        &'a self,
        repo_root: &'a Path,
        target_path: &'a Path,
        force: bool,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        Box::pin(async move {
            let target = target_path.to_string_lossy().to_string();
            let mut args: Vec<String> = vec!["worktree".to_string(), "remove".to_string()];
            if force {
                args.push("--force".to_string());
            }
            args.push(target);
            self.run_git(repo_root, &args).await?;
            Ok(())
        })
    }

    fn delete_branch<'a>(
        &'a self,
        repo_root: &'a Path,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        Box::pin(async move {
            self.policy
                .validate_branch(branch)
                .map_err(|_| WorktreeError::UnsafeBranch(branch.to_string()))?;
            let args: Vec<String> =
                vec!["branch".to_string(), "-D".to_string(), branch.to_string()];
            self.run_git(repo_root, &args).await?;
            Ok(())
        })
    }

    fn commit_all<'a>(
        &'a self,
        worktree_path: &'a Path,
        message: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        Box::pin(async move {
            let add_args: Vec<String> = vec!["add".to_string(), "-A".to_string()];
            self.run_git(worktree_path, &add_args).await?;
            // 仅在确有改动时提交，避免 `nothing to commit` 退出码非零被当作错误。
            let status_args: Vec<String> = vec!["status".to_string(), "--porcelain".to_string()];
            let status = self.run_git_output(worktree_path, &status_args).await?;
            if status.stdout.trim().is_empty() {
                return Ok(());
            }
            let commit_args: Vec<String> =
                vec!["commit".to_string(), "-m".to_string(), message.to_string()];
            self.run_git(worktree_path, &commit_args).await?;
            Ok(())
        })
    }

    fn merge_branch<'a>(
        &'a self,
        main_workspace: &'a Path,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<MergeOutcome, WorktreeError>> {
        Box::pin(async move {
            self.policy
                .validate_branch(branch)
                .map_err(|_| WorktreeError::UnsafeBranch(branch.to_string()))?;
            let args: Vec<String> = vec![
                "merge".to_string(),
                "--no-ff".to_string(),
                "-m".to_string(),
                format!("Merge subagent branch {branch}"),
                branch.to_string(),
            ];
            let output = self.run_git_output(main_workspace, &args).await?;
            if output.status == 0 {
                return Ok(MergeOutcome::Merged);
            }
            // 退出码非零：先尝试回退，再按是否为冲突分类。
            let abort_args: Vec<String> = vec!["merge".to_string(), "--abort".to_string()];
            let _ = self.run_git(main_workspace, &abort_args).await;
            let combined = format!("{} {}", output.stdout, output.stderr).to_lowercase();
            if combined.contains("conflict") {
                Ok(MergeOutcome::Conflict)
            } else {
                Err(WorktreeError::GitCommand {
                    args: "merge".to_string(),
                    stderr: output.stderr,
                })
            }
        })
    }
}
