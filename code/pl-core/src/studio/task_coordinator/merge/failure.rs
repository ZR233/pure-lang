use std::path::Path;

use anyhow::{Context, Result};

use super::accept::{MergeCommitProof, verify_created_merge_commit};
use super::barriers::MergeFailurePoint;
use super::conflict::validate_merge_failure_workspace;
use super::git::{checked_git, run_git};
use super::validation::{validate_merge_preflight, validate_repository_identity};
use super::verifier::abort_merge;
use crate::studio::task_coordinator::{
    FailTaskMerge, MergeVerificationStep, TaskCoordinator, TaskMergeAgentOutput, TaskMergeScope,
};

pub(super) enum MergeFailureStage {
    BeforeCommit,
    CommitAttempted { expected_tree: Option<String> },
    Conflict,
}

impl TaskCoordinator {
    pub(super) async fn handle_merge_stage_failure(
        &self,
        scope: &TaskMergeScope,
        workspace: &Path,
        verification: Vec<MergeVerificationStep>,
        operation: anyhow::Error,
        stage: MergeFailureStage,
    ) -> Result<TaskMergeAgentOutput> {
        let stage_label = stage.label();
        let recovery = match stage {
            MergeFailureStage::BeforeCommit | MergeFailureStage::Conflict => {
                recover_uncommitted_merge(self, scope, workspace).await
            }
            MergeFailureStage::CommitAttempted { expected_tree } => {
                recover_commit_attempt(self, scope, workspace, expected_tree).await
            }
        };
        let reason = format!("merge {stage_label} failed: {operation:#}");
        self.persist_stage_failure(scope, verification, &reason, &recovery, operation)
            .await
    }

    async fn persist_stage_failure(
        &self,
        scope: &TaskMergeScope,
        verification: Vec<MergeVerificationStep>,
        reason: &str,
        recovery: &str,
        operation: anyhow::Error,
    ) -> Result<TaskMergeAgentOutput> {
        let persistence = match self.inject_merge_failure(MergeFailurePoint::FailurePersistence) {
            Ok(()) => {
                self.store
                    .fail_task_merge(FailTaskMerge {
                        merge_id: scope.merge.id.clone(),
                        reason: reason.to_string(),
                        verification_steps: verification,
                        compensation: Some(recovery.to_string()),
                    })
                    .await
            }
            Err(error) => Err(error),
        };
        if let Err(persistence_error) = persistence {
            let fallback_reason = format!(
                "{reason}; Git recovery: {recovery}; merge failure persistence also failed: {persistence_error:#}"
            );
            let block = self.block_run(&scope.run, fallback_reason.clone()).await;
            self.release_owned_process_lease(&scope.run.id);
            return match block {
                Ok(()) => Err(operation).context(fallback_reason),
                Err(block_error) => Err(operation).context(format!(
                    "{fallback_reason}; exact TaskRun block also failed: {block_error:#}"
                )),
            };
        }
        self.release_owned_process_lease(&scope.run.id);
        Err(operation).context(format!("{reason}; Git recovery: {recovery}"))
    }
}

impl MergeFailureStage {
    fn label(&self) -> &'static str {
        match self {
            Self::BeforeCommit => "before commit",
            Self::CommitAttempted { .. } => "commit attempt",
            Self::Conflict => "conflict persistence",
        }
    }
}

async fn recover_uncommitted_merge(
    coordinator: &TaskCoordinator,
    scope: &TaskMergeScope,
    workspace: &Path,
) -> String {
    if validate_merge_preflight(
        &scope.run,
        &scope.lease,
        &scope.work_unit,
        &scope.delivery,
        &scope.run.expected_head,
    )
    .await
    .is_ok()
    {
        return "workspace already matched the exact pre-merge state; no abort was required"
            .to_string();
    }
    if let Err(error) = validate_exact_uncommitted_merge(scope, workspace).await {
        return format!(
            "unsafe abort failure prevented recovery; Git state preserved without an abort attempt: {error:#}"
        );
    }
    if let Err(error) = abort_merge(workspace).await {
        return format!("unsafe abort failure preserved merge state for inspection: {error:#}");
    }
    coordinator.pause_after_merge_abort().await;
    match validate_merge_preflight(
        &scope.run,
        &scope.lease,
        &scope.work_unit,
        &scope.delivery,
        &scope.run.expected_head,
    )
    .await
    {
        Ok(_) => "merge --abort restored the exact pre-merge state".to_string(),
        Err(error) => format!(
            "merge --abort ran but exact prestate validation failed; resulting Git state preserved: {error:#}"
        ),
    }
}

async fn validate_exact_uncommitted_merge(scope: &TaskMergeScope, workspace: &Path) -> Result<()> {
    validate_repository_identity(
        workspace,
        Path::new(&scope.run.workspace_root),
        Path::new(&scope.run.git_common_dir),
        &scope.run.branch,
        &scope.run.expected_head,
        false,
    )
    .await?;
    let merge_head = checked_git(
        workspace,
        vec!["rev-parse".into(), "--verify".into(), "MERGE_HEAD".into()],
    )
    .await?;
    if merge_head != scope.delivery.head_commit {
        anyhow::bail!("MERGE_HEAD does not match the accepted executor delivery");
    }
    validate_merge_failure_workspace(scope, workspace).await
}

async fn recover_commit_attempt(
    coordinator: &TaskCoordinator,
    scope: &TaskMergeScope,
    workspace: &Path,
    expected_tree: Option<String>,
) -> String {
    let candidate = match checked_git(workspace, vec!["rev-parse".into(), "HEAD".into()]).await {
        Ok(candidate) => candidate,
        Err(error) => {
            return format!(
                "unsafe commit recovery was not attempted because candidate HEAD could not be read; Git state preserved: {error:#}"
            );
        }
    };
    if candidate == scope.run.expected_head {
        return recover_uncommitted_merge(coordinator, scope, workspace).await;
    }
    let Some(expected_tree) = expected_tree else {
        return format!(
            "candidate HEAD {candidate} differs from the pre-merge HEAD but no verified index tree is available; Git state preserved"
        );
    };
    compensate_candidate_commit(scope, workspace, candidate, expected_tree).await
}

async fn compensate_candidate_commit(
    scope: &TaskMergeScope,
    workspace: &Path,
    candidate: String,
    expected_tree: String,
) -> String {
    let proof = MergeCommitProof {
        commit: candidate,
        expected_tree,
    };
    if let Err(error) = verify_created_merge_commit(scope, workspace, &proof).await {
        return format!(
            "unsafe post-commit state preserved without reset or cleanup; candidate HEAD failed full proof: {error:#}"
        );
    }
    if let Err(error) = verify_created_merge_commit(scope, workspace, &proof).await {
        return format!(
            "unsafe commit compensation was cancelled by repeated full-proof drift; Git state was preserved: {error:#}"
        );
    }
    let reset = match run_git(
        workspace,
        vec![
            "reset".into(),
            "--hard".into(),
            scope.run.expected_head.clone(),
        ],
    )
    .await
    {
        Ok(reset) if reset.success => reset,
        Ok(reset) => {
            return format!(
                "verified candidate merge commit could not be reset; Git state preserved: {}",
                reset.stderr_lossy()
            );
        }
        Err(error) => {
            return format!(
                "verified candidate merge commit reset runner failed; Git state preserved: {error:#}"
            );
        }
    };
    drop(reset);
    match validate_merge_preflight(
        &scope.run,
        &scope.lease,
        &scope.work_unit,
        &scope.delivery,
        &scope.run.expected_head,
    )
    .await
    {
        Ok(_) => "exact candidate merge commit reset to previous HEAD".to_string(),
        Err(error) => format!(
            "candidate merge commit reset ran but exact prestate validation failed; resulting Git state preserved: {error:#}"
        ),
    }
}
