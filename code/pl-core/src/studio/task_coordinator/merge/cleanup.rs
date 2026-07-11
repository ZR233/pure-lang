use std::path::{Path, PathBuf};

use crate::AgentSupervisor;
use crate::agent::worktree::{CloseDisposition, WorktreeHandle, WorktreeManager};

use super::git::{checked_git, run_git};
use crate::studio::task_coordinator::{MergeCleanupEvidence, TaskMergeScope};

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct MergeCleanupTestBarrier {
    entered: std::sync::Arc<tokio::sync::Barrier>,
    release: std::sync::Arc<tokio::sync::Barrier>,
}

#[cfg(test)]
impl MergeCleanupTestBarrier {
    pub(crate) fn new() -> Self {
        Self {
            entered: std::sync::Arc::new(tokio::sync::Barrier::new(2)),
            release: std::sync::Arc::new(tokio::sync::Barrier::new(2)),
        }
    }

    pub(crate) async fn pause(&self) {
        self.entered.wait().await;
        self.release.wait().await;
    }

    pub(crate) async fn wait_until_entered(&self) {
        self.entered.wait().await;
    }

    pub(crate) async fn release(&self) {
        self.release.wait().await;
    }
}

pub(crate) async fn cleanup_accepted_delivery(
    scope: &TaskMergeScope,
    supervisor: &AgentSupervisor,
    event_tx: &pl_trace::AgentEventSender,
    call_id: &str,
) -> MergeCleanupEvidence {
    match validate_cleanup_identity(scope).await {
        Ok(CleanupPresence::AlreadyAbsent) => return cleanup_success("alreadyAbsent"),
        Ok(CleanupPresence::Present) => {}
        Err(error) => return cleanup_failure(error.to_string()),
    }
    if supervisor.record(&scope.outcome.agent_id).await.is_some() {
        let cleanup = match supervisor
            .close_agent(
                "/root",
                &scope.outcome.agent_id,
                "executor delivery accepted by task_merge_agent",
                event_tx,
                call_id.to_string(),
                CloseDisposition::Discard,
            )
            .await
        {
            Ok(_) => cleanup_success("discarded"),
            Err(error) => cleanup_failure(error.to_string()),
        };
        return verify_cleanup_result(scope, cleanup).await;
    }

    let manager = WorktreeManager::local(PathBuf::from(&scope.run.workspace_root));
    let handle = WorktreeHandle {
        path: PathBuf::from(&scope.work_unit.worktree_path),
        branch: scope.work_unit.branch.clone(),
    };
    let cleanup = match manager.close(&handle, CloseDisposition::Discard).await {
        Ok(_) => cleanup_success("discarded"),
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
