use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::git::{checked_git, run_git};
use super::validation::{changed_files_between, validate_merge_preflight};
use super::verifier::abort_merge;
use crate::studio::task_coordinator::{
    FailTaskMerge, MergeCleanupEvidence, MergeVerificationStep, TaskCoordinator,
    TaskMergeAgentOutput, TaskMergeScope,
};

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct MergeCommitTestBarrier {
    entered: std::sync::Arc<tokio::sync::Barrier>,
    release: std::sync::Arc<tokio::sync::Barrier>,
}

#[cfg(test)]
impl MergeCommitTestBarrier {
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

    pub(crate) async fn wait_until_committed(&self) {
        self.entered.wait().await;
    }

    pub(crate) async fn release(&self) {
        self.release.wait().await;
    }
}

impl TaskCoordinator {
    pub(super) async fn fail_uncommitted_merge(
        &self,
        scope: &TaskMergeScope,
        workspace: &Path,
        verification: Vec<MergeVerificationStep>,
        reason: String,
    ) -> Result<TaskMergeAgentOutput> {
        abort_merge(workspace).await?;
        validate_merge_preflight(
            &scope.run,
            &scope.lease,
            &scope.work_unit,
            &scope.delivery,
            &scope.run.expected_head,
        )
        .await?;
        self.store
            .fail_task_merge(FailTaskMerge {
                merge_id: scope.merge.id.clone(),
                reason: reason.clone(),
                verification_steps: verification,
                compensation: Some("merge --abort restored prestate".to_string()),
            })
            .await?;
        self.release_owned_process_lease(&scope.run.id);
        bail!("{reason}")
    }

    pub(super) async fn compensate_failed_durable_cas(
        &self,
        scope: &TaskMergeScope,
        workspace: &Path,
        merge_commit: &str,
        verification: Vec<MergeVerificationStep>,
        operation: anyhow::Error,
    ) -> Result<TaskMergeAgentOutput> {
        if let Err(safety_error) = verify_created_merge_commit(scope, workspace, merge_commit).await
        {
            self.store
                .fail_task_merge(FailTaskMerge {
                    merge_id: scope.merge.id.clone(),
                    reason: format!("durable merge CAS failed: {operation}"),
                    verification_steps: verification,
                    compensation: Some(format!(
                        "unsafe compensation was not attempted; external Git state preserved: {safety_error}"
                    )),
                })
                .await?;
            self.release_owned_process_lease(&scope.run.id);
            return Err(operation)
                .context("durable merge CAS failed with unsafe compensation state");
        }
        let reset = run_git(
            workspace,
            vec![
                "reset".into(),
                "--hard".into(),
                scope.run.expected_head.clone(),
            ],
        )
        .await?;
        let compensation = if reset.success {
            validate_merge_preflight(
                &scope.run,
                &scope.lease,
                &scope.work_unit,
                &scope.delivery,
                &scope.run.expected_head,
            )
            .await?;
            "exact merge commit reset to previous HEAD".to_string()
        } else {
            format!("merge compensation failed: {}", reset.stderr_lossy())
        };
        self.store
            .fail_task_merge(FailTaskMerge {
                merge_id: scope.merge.id.clone(),
                reason: format!("durable merge CAS failed: {operation}"),
                verification_steps: verification,
                compensation: Some(compensation),
            })
            .await?;
        self.release_owned_process_lease(&scope.run.id);
        Err(operation).context("durable merge CAS failed after Git commit")
    }
}

pub(super) async fn verify_created_merge_commit(
    scope: &TaskMergeScope,
    workspace: &Path,
    merge_commit: &str,
) -> Result<()> {
    let head = checked_git(workspace, vec!["rev-parse".into(), "HEAD".into()]).await?;
    let parents = checked_git(
        workspace,
        vec![
            "show".into(),
            "-s".into(),
            "--format=%P".into(),
            merge_commit.into(),
        ],
    )
    .await?;
    if head != merge_commit
        || parents != format!("{} {}", scope.run.expected_head, scope.delivery.head_commit)
    {
        bail!("current HEAD is not the exact coordinator merge commit");
    }
    let commit_files =
        changed_files_between(workspace, &scope.run.expected_head, merge_commit).await?;
    let allowed = scope
        .delivery
        .changed_files
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    if commit_files.iter().any(|path| !allowed.contains(path)) {
        bail!("merge commit contains files outside the validated delivery scope");
    }
    let status = checked_git(
        workspace,
        vec![
            "status".into(),
            "--porcelain=v1".into(),
            "--untracked-files=all".into(),
        ],
    )
    .await?;
    if !status.is_empty() {
        bail!("merge commit left the task workspace dirty");
    }
    Ok(())
}

pub(super) fn merge_commit_message(
    scope: &TaskMergeScope,
    verification: &[MergeVerificationStep],
) -> String {
    let verification = verification
        .iter()
        .map(|step| step.command.join(" "))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Merge executor {}\n\nPure-Task-Run: {}\nPure-Source-Agent: {}\nPure-Previous-Head: {}\nPure-Source-Commit: {}\nPure-Verification: {}",
        scope.outcome.agent_id,
        scope.run.id,
        scope.outcome.agent_id,
        scope.run.expected_head,
        scope.delivery.head_commit,
        verification
    )
}

pub(super) fn pending_cleanup() -> MergeCleanupEvidence {
    MergeCleanupEvidence {
        status: "pending".to_string(),
        detail: None,
    }
}
