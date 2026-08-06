use anyhow::{Context, Result};

use crate::studio::task_coordinator::{AgentDelivery, AgentWorktreeDelivery, WorkCompletionRecord};

pub(super) fn delivery_from_completion(completion: &WorkCompletionRecord) -> Result<AgentDelivery> {
    Ok(AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: completion.worktree_path.clone(),
            branch: completion.branch.clone(),
        },
        base_commit: completion.base_commit.clone(),
        head_commit: completion
            .head_commit
            .clone()
            .context("approved delivery completion has no head commit")?,
        changed_files: completion.changed_files.clone(),
        verification_summary: completion.verification_summary.clone(),
    })
}
