use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::git::{checked_git, run_git};
use super::validation::{
    changed_files_between, validate_merge_preflight, validate_repository_identity,
};
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
        let compensation = match abort_merge(workspace).await {
            Ok(()) => {
                self.pause_after_merge_abort().await;
                match validate_merge_preflight(
                    &scope.run,
                    &scope.lease,
                    &scope.work_unit,
                    &scope.delivery,
                    &scope.run.expected_head,
                )
                .await
                {
                    Ok(_) => "merge --abort restored exact prestate".to_string(),
                    Err(error) => format!(
                        "unsafe abort result preserved for inspection; exact prestate validation failed: {error:#}"
                    ),
                }
            }
            Err(error) => {
                format!("unsafe abort failure preserved merge state for inspection: {error:#}")
            }
        };
        let persistence = self
            .store
            .fail_task_merge(FailTaskMerge {
                merge_id: scope.merge.id.clone(),
                reason: reason.clone(),
                verification_steps: verification,
                compensation: Some(compensation.clone()),
            })
            .await;
        self.release_owned_process_lease(&scope.run.id);
        if let Err(error) = persistence {
            bail!(
                "{reason}; merge failure persistence also failed: {error:#}; compensation: {compensation}"
            );
        }
        bail!("{reason}")
    }

    pub(super) async fn fail_committed_merge_without_compensation(
        &self,
        scope: &TaskMergeScope,
        verification: Vec<MergeVerificationStep>,
        reason: String,
    ) -> Result<TaskMergeAgentOutput> {
        let persistence = self
            .store
            .fail_task_merge(FailTaskMerge {
                merge_id: scope.merge.id.clone(),
                reason: reason.clone(),
                verification_steps: verification,
                compensation: Some(
                    "unsafe post-commit state preserved without reset or cleanup".to_string(),
                ),
            })
            .await;
        self.release_owned_process_lease(&scope.run.id);
        match persistence {
            Ok(_) => bail!("{reason}"),
            Err(error) => bail!("{reason}; merge failure persistence also failed: {error:#}"),
        }
    }

    pub(super) async fn compensate_failed_durable_cas(
        &self,
        scope: &TaskMergeScope,
        workspace: &Path,
        proof: &MergeCommitProof,
        verification: Vec<MergeVerificationStep>,
        operation: anyhow::Error,
    ) -> Result<TaskMergeAgentOutput> {
        if let Err(safety_error) = verify_created_merge_commit(scope, workspace, proof).await {
            let persistence = self
                .store
                .fail_task_merge(FailTaskMerge {
                    merge_id: scope.merge.id.clone(),
                    reason: format!("durable merge CAS failed: {operation}"),
                    verification_steps: verification,
                    compensation: Some(format!(
                        "unsafe compensation was not attempted; external Git state preserved: {safety_error}"
                    )),
                })
                .await;
            self.release_owned_process_lease(&scope.run.id);
            if let Err(error) = persistence {
                return Err(operation).context(format!(
                    "durable merge CAS failed with unsafe compensation state; failure persistence also failed: {error:#}"
                ));
            }
            return Err(operation)
                .context("durable merge CAS failed with unsafe compensation state");
        }
        if let Err(safety_error) = verify_created_merge_commit(scope, workspace, proof).await {
            let persistence = self
                .store
                .fail_task_merge(FailTaskMerge {
                    merge_id: scope.merge.id.clone(),
                    reason: format!("durable merge CAS failed: {operation}"),
                    verification_steps: verification,
                    compensation: Some(format!(
                        "unsafe compensation was cancelled by second full proof; external Git state preserved: {safety_error}"
                    )),
                })
                .await;
            self.release_owned_process_lease(&scope.run.id);
            if let Err(error) = persistence {
                return Err(operation).context(format!(
                    "durable merge CAS failed after compensation proof drift; failure persistence also failed: {error:#}"
                ));
            }
            return Err(operation)
                .context("durable merge CAS failed after compensation proof drift");
        }
        let compensation = match run_git(
            workspace,
            vec![
                "reset".into(),
                "--hard".into(),
                scope.run.expected_head.clone(),
            ],
        )
        .await
        {
            Ok(reset) if reset.success => match validate_merge_preflight(
                &scope.run,
                &scope.lease,
                &scope.work_unit,
                &scope.delivery,
                &scope.run.expected_head,
            )
            .await
            {
                Ok(_) => "exact merge commit reset to previous HEAD".to_string(),
                Err(error) => {
                    format!("merge reset ran but exact prestate validation failed: {error:#}")
                }
            },
            Ok(reset) => format!("merge compensation failed: {}", reset.stderr_lossy()),
            Err(error) => format!("merge compensation process failed: {error:#}"),
        };
        let persistence = self
            .store
            .fail_task_merge(FailTaskMerge {
                merge_id: scope.merge.id.clone(),
                reason: format!("durable merge CAS failed: {operation}"),
                verification_steps: verification,
                compensation: Some(compensation),
            })
            .await;
        self.release_owned_process_lease(&scope.run.id);
        if let Err(error) = persistence {
            return Err(operation).context(format!(
                "durable merge CAS failed after Git commit; failure persistence also failed: {error:#}"
            ));
        }
        Err(operation).context("durable merge CAS failed after Git commit")
    }
}

pub(super) struct MergeCommitProof {
    pub(super) commit: String,
    pub(super) expected_tree: String,
}

pub(super) async fn verify_created_merge_commit(
    scope: &TaskMergeScope,
    workspace: &Path,
    proof: &MergeCommitProof,
) -> Result<()> {
    validate_repository_identity(
        workspace,
        Path::new(&scope.run.workspace_root),
        Path::new(&scope.run.git_common_dir),
        &scope.run.branch,
        &proof.commit,
        true,
    )
    .await?;
    let parents = checked_git(
        workspace,
        vec![
            "show".into(),
            "-s".into(),
            "--format=%P".into(),
            proof.commit.clone(),
        ],
    )
    .await?;
    if parents != format!("{} {}", scope.run.expected_head, scope.delivery.head_commit) {
        bail!("current HEAD is not the exact coordinator merge commit");
    }
    let commit_tree = checked_git(
        workspace,
        vec![
            "show".into(),
            "-s".into(),
            "--format=%T".into(),
            proof.commit.clone(),
        ],
    )
    .await?;
    if commit_tree != proof.expected_tree {
        bail!("merge commit tree differs from the verified pre-commit index tree");
    }
    let commit_files =
        changed_files_between(workspace, &scope.run.expected_head, &proof.commit).await?;
    let allowed = scope
        .delivery
        .changed_files
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    if commit_files.iter().any(|path| !allowed.contains(path)) {
        bail!("merge commit contains files outside the validated delivery scope");
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
