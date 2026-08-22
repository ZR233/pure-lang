use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::{AgentLifecycleState, AgentRuntimeHandle, ThreadId, WorktreeHandle, WorktreeManager};

use super::git::{checked_git, run_git};
use crate::studio::task_coordinator::{MergeCleanupEvidence, TaskCoordinator, TaskMergeScope};

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
) -> MergeCleanupEvidence {
    match validate_cleanup_identity(scope).await {
        Ok(CleanupPresence::AlreadyAbsent) => return cleanup_success("alreadyAbsent"),
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
                    snapshot.lifecycle,
                    AgentLifecycleState::Closing | AgentLifecycleState::Closed
                )
        }) {
            let cleanup = match runtime.close(agent_id).await {
                Ok(_) => cleanup_success("discarded"),
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
        Ok(()) => cleanup_success("discarded"),
        Err(error)
            if cleanup_is_already_absent(&scope.work_unit.worktree_path, &error.to_string()) =>
        {
            cleanup_success("alreadyAbsent")
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
    if branch_tip != scope.delivery.head_commit
        || worktree_head != scope.delivery.head_commit
        || worktree_branch != scope.work_unit.branch
    {
        anyhow::bail!("executor branch or worktree tip drifted before cleanup");
    }
    Ok(CleanupPresence::Present)
}

async fn verify_cleanup_result(
    scope: &TaskMergeScope,
    cleanup: MergeCleanupEvidence,
) -> MergeCleanupEvidence {
    if cleanup.status == "failed" {
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

fn cleanup_success(status: &str) -> MergeCleanupEvidence {
    MergeCleanupEvidence {
        status: status.to_string(),
        detail: None,
    }
}

fn cleanup_failure(detail: String) -> MergeCleanupEvidence {
    MergeCleanupEvidence {
        status: "failed".to_string(),
        detail: Some(detail),
    }
}
