use std::collections::HashSet;
use std::path::Path;

use anyhow::{Result, bail};

use super::git::checked_git;
use super::validation::{changed_files_between, validate_repository_identity};
use crate::studio::task_coordinator::{
    MergeCleanupEvidence, MergeVerificationStep, TaskMergeScope,
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
        scope.completion.executor_agent_id,
        scope.run.id,
        scope.completion.executor_agent_id,
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
