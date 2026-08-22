use crate::agent::worktree::same_worktree_path;
use crate::studio::task_coordinator::{
    AgentDelivery, WorkCompletionKind, WorkCompletionRecord, WorkCompletionStatus, WorkUnit,
    WorkUnitStateKind,
};
use anyhow::{Result, bail};

pub(super) fn ensure_preflight_delivery_identity(
    task_run_id: &str,
    agent_id: &str,
    work_unit: &WorkUnit,
    completion: &WorkCompletionRecord,
    delivery: &AgentDelivery,
) -> Result<()> {
    let mut mismatches = Vec::new();
    if work_unit.task_run_id != task_run_id {
        mismatches.push("taskRunId");
    }
    if work_unit.executor_thread_id.as_deref() != Some(agent_id) {
        mismatches.push("agentId");
    }
    if work_unit.kind() != WorkUnitStateKind::ReviewPassed {
        mismatches.push("delivery status");
    }
    if work_unit.attempt == 0 {
        mismatches.push("attempt");
    }
    if completion.task_run_id != task_run_id
        || completion.work_unit_id != work_unit.id
        || completion.executor_agent_id != agent_id
        || completion.kind() != WorkCompletionKind::Delivery
        || completion.status() != WorkCompletionStatus::Approved
    {
        mismatches.push("completion");
    }
    if !same_worktree_path(&delivery.worktree.path, &work_unit.worktree_path) {
        mismatches.push("worktree path");
    }
    if delivery.worktree.branch != work_unit.branch {
        mismatches.push("worktree branch");
    }
    if delivery.base_commit != work_unit.base_commit {
        mismatches.push("base commit");
    }
    if completion.head_commit() != Some(delivery.head_commit.as_str())
        || completion.changed_files() != delivery.changed_files
        || completion.worktree_path != delivery.worktree.path
        || completion.branch != delivery.worktree.branch
        || completion.base_commit != delivery.base_commit
    {
        mismatches.push("completion delivery");
    }
    if !mismatches.is_empty() {
        bail!(
            "agent delivery does not match the planner-owned approved completion: {}",
            mismatches.join(", ")
        );
    }
    Ok(())
}
