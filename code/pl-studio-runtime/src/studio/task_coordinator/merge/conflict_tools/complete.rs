use std::path::Path;

use anyhow::{Context, Result, bail};

use super::super::accept::{
    MergeCommitProof, merge_commit_message, pending_cleanup, verify_created_merge_commit,
};
use super::super::git::{checked_git, run_git};
use super::super::validation::validate_final_head;
use crate::AgentRuntimeHandle;
use crate::studio::task_coordinator::{
    AbortConflictMerge, CompleteConflictMerge, MergeRecord, MergeStatus, TaskCoordinator,
    TaskMergeAgentOutput, TaskMergeScope,
};

impl TaskCoordinator {
    pub(crate) async fn continue_active_conflict(
        &self,
        session_id: &str,
        merge_id: &str,
        resolution_summary: &str,
        runtime: Option<&AgentRuntimeHandle>,
    ) -> Result<TaskMergeAgentOutput> {
        if resolution_summary.is_empty() {
            bail!("resolutionSummary must not be empty");
        }
        let (scope, output) = {
            let guard = self.lock_branch_mutation().await;
            self.ensure_branch_mutation_guard(&guard)?;
            let (scope, unmerged) = self
                .load_active_conflict_scope(session_id, merge_id)
                .await?;
            if !unmerged.is_empty() {
                bail!("merge_continue requires zero unresolved conflict entries");
            }
            let verification = scope
                .merge
                .evidence
                .as_ref()
                .and_then(|evidence| evidence.conflict_verification.as_ref())
                .context("merge_continue requires a successful current merge_verify")?;
            if !verification.success
                || verification.attempt != scope.merge.attempt
                || verification.attempt > 3
            {
                bail!("merge_continue requires a successful current merge_verify");
            }
            let expected_tree = verification
                .index_tree
                .clone()
                .context("successful conflict verification has no index tree")?;
            let workspace = Path::new(&scope.run.workspace_root);
            let current_tree = checked_git(workspace, vec!["write-tree".into()]).await?;
            if current_tree != expected_tree {
                bail!("conflict index changed after the successful verification");
            }
            let message = format!(
                "{}\nPure-Conflict-Resolution: {resolution_summary}",
                merge_commit_message(&scope, &verification.steps)
            );
            let commit = run_git(workspace, vec!["commit".into(), "-m".into(), message]).await?;
            if !commit.success {
                bail!("conflict merge commit failed: {}", commit.stderr_lossy());
            }
            let merge_commit =
                match checked_git(workspace, vec!["rev-parse".into(), "HEAD".into()]).await {
                    Ok(head) => head,
                    Err(error) => {
                        self.block_unaccepted_conflict_commit(
                            &scope,
                            format!("read conflict merge commit HEAD failed: {error:#}"),
                        )
                        .await?;
                        return Err(error).context("read conflict merge commit HEAD failed");
                    }
                };
            let proof = MergeCommitProof {
                commit: merge_commit.clone(),
                expected_tree,
            };
            if let Err(error) = verify_created_merge_commit(&scope, workspace, &proof).await {
                self.block_unaccepted_conflict_commit(
                    &scope,
                    format!("conflict merge commit proof failed: {error:#}"),
                )
                .await?;
                return Err(error).context("conflict merge commit proof failed");
            }
            if let Err(error) = self
                .store
                .complete_conflict_merge(CompleteConflictMerge {
                    merge_id: scope.merge.id.clone(),
                    expected_head: scope.run.expected_head.clone(),
                    merge_commit: merge_commit.clone(),
                    resolution_summary: resolution_summary.to_string(),
                })
                .await
            {
                self.block_unaccepted_conflict_commit(
                    &scope,
                    format!("conflict merge durable CAS failed: {error:#}"),
                )
                .await?;
                return Err(error).context("conflict merge durable CAS failed");
            }
            let durable_run = match self.store.read_task_run(&scope.run.id).await {
                Ok(Some(run)) => run,
                Ok(None) => {
                    return self
                        .block_accepted_scope_failure(
                            &scope,
                            anyhow::anyhow!("accepted conflict task run disappeared"),
                        )
                        .await;
                }
                Err(error) => {
                    return self
                        .block_accepted_scope_failure(
                            &scope,
                            error.context("read accepted conflict task run"),
                        )
                        .await;
                }
            };
            if let Err(error) = validate_final_head(&durable_run, &merge_commit).await {
                return self.block_accepted_scope_failure(&scope, error).await;
            }
            let output = TaskMergeAgentOutput {
                merge_id: scope.merge.id.clone(),
                status: MergeStatus::Merged,
                previous_head: scope.run.expected_head.clone(),
                new_head: Some(merge_commit),
                agent_id: scope.outcome.agent_id.clone(),
                source_commit: scope.delivery.head_commit.clone(),
                changed_files: scope.delivery.changed_files.clone(),
                verification: verification.steps.clone(),
                cleanup: pending_cleanup(),
                conflict_files: scope.merge.conflict_files.clone(),
            };
            (scope, output)
        };
        self.finish_accepted_delivery_cleanup(&scope, output, runtime)
            .await
    }

    pub(crate) async fn abort_active_conflict(
        &self,
        session_id: &str,
        merge_id: &str,
        reason: &str,
    ) -> Result<MergeRecord> {
        if reason.is_empty() {
            bail!("reason must not be empty");
        }
        let guard = self.lock_branch_mutation().await;
        self.ensure_branch_mutation_guard(&guard)?;
        let (scope, _) = self
            .load_active_conflict_scope(session_id, merge_id)
            .await?;
        abort_conflict_scope(self, &scope, reason).await
    }

    async fn block_unaccepted_conflict_commit(
        &self,
        scope: &TaskMergeScope,
        reason: String,
    ) -> Result<()> {
        let result = self
            .store
            .abort_conflict_merge(AbortConflictMerge {
                merge_id: scope.merge.id.clone(),
                expected_head: scope.run.expected_head.clone(),
                reason: reason.clone(),
                compensation: "post-commit failure preserved exact conflict merge commit"
                    .to_string(),
            })
            .await;
        match result {
            Ok(_) => {
                self.finish_blocked_transition(&scope.run.id).await?;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

pub(super) async fn abort_conflict_scope(
    coordinator: &TaskCoordinator,
    scope: &TaskMergeScope,
    reason: &str,
) -> Result<MergeRecord> {
    let workspace = Path::new(&scope.run.workspace_root);
    let aborted = run_git(workspace, vec!["merge".into(), "--abort".into()]).await?;
    if !aborted.success {
        bail!("git merge --abort failed: {}", aborted.stderr_lossy());
    }
    validate_final_head(&scope.run, &scope.run.expected_head).await?;
    let expected_tree = scope
        .merge
        .evidence
        .as_ref()
        .map(|evidence| evidence.pre_index_tree.as_str())
        .context("conflicted merge has no pre-index evidence")?;
    let actual_tree = checked_git(workspace, vec!["write-tree".into()]).await?;
    if actual_tree != expected_tree {
        bail!("merge abort did not restore the durable pre-index tree");
    }
    let record = coordinator
        .store
        .abort_conflict_merge(AbortConflictMerge {
            merge_id: scope.merge.id.clone(),
            expected_head: scope.run.expected_head.clone(),
            reason: reason.to_string(),
            compensation: "git merge --abort restored exact expected HEAD, index, and workspace"
                .to_string(),
        })
        .await?;
    coordinator
        .finish_blocked_transition(&scope.run.id)
        .await?;
    Ok(record)
}
