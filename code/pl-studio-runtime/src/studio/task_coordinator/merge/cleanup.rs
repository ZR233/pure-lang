use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::{AgentRuntimeHandle, AgentState, ThreadId, WorktreeHandle, WorktreeManager};

use super::git::{checked_git, run_git};
use crate::studio::task_coordinator::{MergeCleanupResult, TaskCoordinator, TaskMergeScope};

impl TaskCoordinator {
    pub(in crate::studio::task_coordinator) async fn validate_accepted_cleanup_replay(
        &self,
        _scope: &TaskMergeScope,
    ) -> Result<()> {
        Ok(())
    }
}

pub(crate) async fn cleanup_accepted_delivery(
    scope: &TaskMergeScope,
    runtime: Option<&AgentRuntimeHandle>,
) -> MergeCleanupResult {
    match validate_cleanup_identity(scope).await {
        Ok(CleanupPresence::AlreadyAbsent) => return MergeCleanupResult::AlreadyAbsent,
        Ok(CleanupPresence::Present) => {}
        Err(error) => return cleanup_failure(error.to_string()),
    }
    if let Some(runtime) = runtime {
        let agent_id = match ThreadId::new(scope.completion.executor_agent_id.clone()) {
            Ok(agent_id) => agent_id,
            Err(error) => return cleanup_failure(error.to_string()),
        };
        let listed = match runtime.list().await {
            Ok(listed) => listed,
            Err(error) => return cleanup_failure(error.to_string()),
        };
        if listed.iter().any(|snapshot| {
            snapshot.identity.id == agent_id
                && !matches!(
                    snapshot.state,
                    AgentState::Closing(_) | AgentState::Closed(_)
                )
        }) {
            let cleanup = match runtime.close(agent_id).await {
                Ok(_) => MergeCleanupResult::Discarded,
                Err(error) => cleanup_failure(error.to_string()),
            };
            return verify_cleanup_result(scope, cleanup).await;
        }
    }

    let manager = WorktreeManager::local(PathBuf::from(&scope.run.workspace_root));
    let handle = WorktreeHandle {
        path: PathBuf::from(&scope.work_unit.worktree_path),
        branch: scope.work_unit.branch.clone(),
    };
    let cleanup = match manager.discard(&handle).await {
        Ok(()) => MergeCleanupResult::Discarded,
        Err(error)
            if cleanup_is_already_absent(&scope.work_unit.worktree_path, &error.to_string()) =>
        {
            MergeCleanupResult::AlreadyAbsent
        }
        Err(error) => cleanup_failure(error.to_string()),
    };
    verify_cleanup_result(scope, cleanup).await
}

enum CleanupPresence {
    Present,
    AlreadyAbsent,
}

async fn validate_cleanup_identity(scope: &TaskMergeScope) -> anyhow::Result<CleanupPresence> {
    let repository = Path::new(&scope.run.workspace_root);
    let worktree = Path::new(&scope.work_unit.worktree_path);
    let reference = format!("refs/heads/{}", scope.work_unit.branch);
    let branch = run_git(
        repository,
        vec!["rev-parse".into(), "--verify".into(), reference],
    )
    .await?;
    if !worktree.exists() && !branch.success {
        return Ok(CleanupPresence::AlreadyAbsent);
    }
    if !worktree.exists() || !branch.success {
        anyhow::bail!("executor cleanup resources are partially missing");
    }
    let branch_tip = branch.stdout_text()?.trim().to_string();
    let worktree_head = checked_git(worktree, vec!["rev-parse".into(), "HEAD".into()]).await?;
    let delivery_head = resolve_delivery_head(worktree, &scope.delivery.head_commit).await?;
    let worktree_branch = checked_git(
        worktree,
        vec![
            "symbolic-ref".into(),
            "--quiet".into(),
            "--short".into(),
            "HEAD".into(),
        ],
    )
    .await?;
    if branch_tip != delivery_head
        || worktree_head != delivery_head
        || worktree_branch != scope.work_unit.branch
    {
        anyhow::bail!("executor branch or worktree tip drifted before cleanup");
    }
    Ok(CleanupPresence::Present)
}

async fn resolve_delivery_head(worktree: &Path, delivery_head: &str) -> Result<String> {
    checked_git(
        worktree,
        vec![
            "rev-parse".into(),
            "--verify".into(),
            format!("{delivery_head}^{{commit}}"),
        ],
    )
    .await
}

async fn verify_cleanup_result(
    scope: &TaskMergeScope,
    cleanup: MergeCleanupResult,
) -> MergeCleanupResult {
    if matches!(cleanup, MergeCleanupResult::Failed { .. }) {
        return cleanup;
    }
    match validate_cleanup_identity(scope).await {
        Ok(CleanupPresence::AlreadyAbsent) => cleanup,
        Ok(CleanupPresence::Present) => {
            cleanup_failure("executor cleanup reported success but resources remain".to_string())
        }
        Err(error) => cleanup_failure(error.to_string()),
    }
}

fn cleanup_is_already_absent(path: &str, error: &str) -> bool {
    !Path::new(path).exists()
        && (error.contains("not a working tree")
            || error.contains("not found")
            || error.contains("branch") && error.contains("not found"))
}

fn cleanup_failure(detail: String) -> MergeCleanupResult {
    MergeCleanupResult::Failed { detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn short_delivery_head_resolves_to_the_canonical_commit() {
        let repository = tempfile::tempdir().expect("temporary repository");
        checked_git(
            repository.path(),
            vec!["init".into(), "-b".into(), "main".into()],
        )
        .await
        .expect("initialize repository");
        std::fs::write(repository.path().join("README.md"), "fixture\n").expect("write fixture");
        checked_git(repository.path(), vec!["add".into(), "README.md".into()])
            .await
            .expect("stage fixture");
        checked_git(
            repository.path(),
            vec!["commit".into(), "-m".into(), "initialize fixture".into()],
        )
        .await
        .expect("commit fixture");
        let full_head = checked_git(repository.path(), vec!["rev-parse".into(), "HEAD".into()])
            .await
            .expect("resolve full head");
        let short_head = &full_head[..7];

        let resolved = resolve_delivery_head(repository.path(), short_head)
            .await
            .expect("resolve short head");

        assert_eq!(resolved, full_head);
    }
}
