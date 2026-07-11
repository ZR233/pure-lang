use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::git::{checked_git, run_git};
use super::validation::validate_final_head;
use crate::studio::task_coordinator::{
    FailTaskMerge, MergeRecord, MergeStatus, TaskCoordinator, TaskMergeScope, TaskRunRecord,
};

pub(crate) enum MergeRestartRecovery {
    Resume(Box<TaskRunRecord>),
    Blocked,
}

impl TaskCoordinator {
    pub(crate) async fn recover_merging_run(
        &self,
        run: &TaskRunRecord,
    ) -> Result<MergeRestartRecovery> {
        let records = self.store.list_merge_records(&run.id).await?;
        let active = records
            .iter()
            .filter(|record| matches!(record.status, MergeStatus::Pending | MergeStatus::Verifying))
            .collect::<Vec<_>>();
        let record = match active.as_slice() {
            [record] => *record,
            [] => bail!("merging run has no pending or verifying merge record"),
            _ => bail!("merging run has multiple active merge records"),
        };
        let evidence = record
            .evidence
            .as_ref()
            .context("active merge has no versioned evidence")?;
        validate_recovery_repository(run).await?;
        let workspace = Path::new(&run.workspace_root);
        let merge_head = run_git(
            workspace,
            vec!["rev-parse".into(), "--verify".into(), "MERGE_HEAD".into()],
        )
        .await?;
        if !merge_head.success {
            if record.status != MergeStatus::Pending {
                return self
                    .block_unsafe_merge_recovery(
                        run,
                        record,
                        "verifying merge lost MERGE_HEAD".to_string(),
                    )
                    .await;
            }
            validate_clean_prestate(run, &evidence.pre_index_tree).await?;
            let recovered = self.store.recover_unstarted_task_merge(&record.id).await?;
            return Ok(MergeRestartRecovery::Resume(Box::new(recovered)));
        }
        let actual_merge_head = merge_head.stdout_text()?.trim().to_string();
        if actual_merge_head != record.source_commit {
            return self
                .block_unsafe_merge_recovery(
                    run,
                    record,
                    format!(
                        "MERGE_HEAD drifted: expected {}, actual {actual_merge_head}",
                        record.source_commit
                    ),
                )
                .await;
        }
        let unmerged =
            run_git(workspace, vec!["ls-files".into(), "-u".into(), "-z".into()]).await?;
        if !unmerged.success {
            bail!(
                "failed to inspect merge recovery index: {}",
                unmerged.stderr_lossy()
            );
        }
        if !unmerged.stdout.is_empty() {
            let scope = self.load_merge_recovery_scope(run, record).await?;
            self.persist_merge_conflict(&scope, workspace).await?;
            let recovered = self
                .store
                .read_task_run(&run.id)
                .await?
                .context("conflict recovery task run disappeared")?;
            return Ok(MergeRestartRecovery::Resume(Box::new(recovered)));
        }
        match validate_applied_merge_scope(run, record).await {
            Ok(()) => {}
            Err(error) => {
                return self
                    .block_unsafe_merge_recovery(run, record, error.to_string())
                    .await;
            }
        }
        let aborted = run_git(workspace, vec!["merge".into(), "--abort".into()]).await?;
        if !aborted.success {
            return self
                .block_unsafe_merge_recovery(
                    run,
                    record,
                    format!("restart merge --abort failed: {}", aborted.stderr_lossy()),
                )
                .await;
        }
        validate_clean_prestate(run, &evidence.pre_index_tree).await?;
        self.store
            .fail_task_merge(FailTaskMerge {
                merge_id: record.id.clone(),
                reason: "restart recovered an interrupted verifying merge".to_string(),
                verification_steps: evidence.verification_steps.clone(),
                compensation: Some(
                    "restart recovery aborted exact verifying merge to prestate".to_string(),
                ),
            })
            .await?;
        self.release_owned_process_lease(&run.id);
        Ok(MergeRestartRecovery::Blocked)
    }

    async fn load_merge_recovery_scope(
        &self,
        run: &TaskRunRecord,
        record: &MergeRecord,
    ) -> Result<TaskMergeScope> {
        let evidence = record
            .evidence
            .as_ref()
            .context("active merge has no versioned evidence")?;
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("task branch lease not found")?;
        let work_unit = self
            .store
            .read_work_unit(&evidence.work_unit_id)
            .await?
            .context("merge work unit not found")?;
        let outcome = self
            .store
            .list_agent_outcomes(&run.id)
            .await?
            .into_iter()
            .find(|outcome| outcome.id == evidence.outcome_id)
            .context("merge outcome not found")?;
        let delivery = outcome
            .delivery
            .clone()
            .context("merge outcome delivery disappeared")?;
        Ok(TaskMergeScope {
            #[cfg(test)]
            origin_phase: evidence.origin_phase,
            run: run.clone(),
            lease,
            work_unit,
            outcome,
            delivery,
            merge: record.clone(),
        })
    }

    async fn block_unsafe_merge_recovery(
        &self,
        run: &TaskRunRecord,
        record: &MergeRecord,
        reason: String,
    ) -> Result<MergeRestartRecovery> {
        let evidence = record
            .evidence
            .as_ref()
            .context("active merge has no versioned evidence")?;
        self.store
            .fail_task_merge(FailTaskMerge {
                merge_id: record.id.clone(),
                reason: format!("unsafe merge restart state: {reason}"),
                verification_steps: evidence.verification_steps.clone(),
                compensation: Some(
                    "unsafe restart recovery preserved Git state without abort".to_string(),
                ),
            })
            .await?;
        self.release_owned_process_lease(&run.id);
        Ok(MergeRestartRecovery::Blocked)
    }
}

async fn validate_recovery_repository(run: &TaskRunRecord) -> Result<()> {
    let snapshot =
        crate::studio::task_coordinator::git::inspect_repository(&run.workspace_root, false)
            .await?;
    if normalized_path(&snapshot.git_common_dir) != normalized_path(Path::new(&run.git_common_dir))
        || snapshot.branch != run.branch
        || snapshot.head != run.expected_head
    {
        bail!("merge recovery repository identity or HEAD drifted");
    }
    Ok(())
}

async fn validate_clean_prestate(run: &TaskRunRecord, pre_index_tree: &str) -> Result<()> {
    validate_final_head(run, &run.expected_head).await?;
    let tree = checked_git(Path::new(&run.workspace_root), vec!["write-tree".into()]).await?;
    if tree != pre_index_tree {
        bail!("merge recovery index did not restore the durable pre-index tree");
    }
    Ok(())
}

async fn validate_applied_merge_scope(run: &TaskRunRecord, record: &MergeRecord) -> Result<()> {
    let evidence = record
        .evidence
        .as_ref()
        .context("active merge has no versioned evidence")?;
    let workspace = Path::new(&run.workspace_root);
    let unstaged = run_git(
        workspace,
        vec!["diff".into(), "--name-only".into(), "-z".into()],
    )
    .await?;
    if !unstaged.success || !unstaged.stdout.is_empty() {
        bail!("verifying merge has unstaged or unreadable changes");
    }
    let status = run_git(
        workspace,
        vec![
            "status".into(),
            "--porcelain=v1".into(),
            "-z".into(),
            "--untracked-files=all".into(),
        ],
    )
    .await?;
    if !status.success
        || status
            .stdout
            .split(|byte| *byte == 0)
            .any(|entry| entry.starts_with(b"?? "))
    {
        bail!("verifying merge has untracked or unreadable state");
    }
    let staged = run_git(
        workspace,
        vec![
            "diff".into(),
            "--cached".into(),
            "--name-only".into(),
            "-z".into(),
        ],
    )
    .await?;
    if !staged.success {
        bail!("failed to inspect verifying merge index");
    }
    let allowed = evidence.changed_files.iter().collect::<HashSet<_>>();
    for path in staged
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(path)?.replace('\\', "/");
        if !allowed.contains(&path) {
            bail!("verifying merge contains unrelated path `{path}`");
        }
    }
    Ok(())
}

fn normalized_path(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}
