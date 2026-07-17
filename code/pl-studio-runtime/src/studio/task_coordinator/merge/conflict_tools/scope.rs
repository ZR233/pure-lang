use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};

use super::super::conflict_index::parse_unmerged_entries;
use super::super::conflict_status::parse_porcelain_entries;
use super::super::git::{checked_git, run_git};
use crate::studio::task_coordinator::{
    ConflictEntry, MergeIndexStage, MergeStatus, TaskCoordinator, TaskMergeScope, TaskRunPhase,
};

impl TaskCoordinator {
    pub(super) async fn load_active_conflict_scope(
        &self,
        session_id: &str,
        merge_id: &str,
    ) -> Result<(TaskMergeScope, BTreeMap<String, Vec<MergeIndexStage>>)> {
        if merge_id.is_empty() {
            bail!("mergeId must not be empty");
        }
        let run = self
            .store
            .read_active_task_run_for_session(session_id)
            .await?;
        if run.phase != TaskRunPhase::ResolvingConflict {
            bail!("conflict tools require phase resolvingConflict");
        }
        let records = self.store.list_merge_records(&run.id).await?;
        let merge = records
            .into_iter()
            .find(|record| record.id == merge_id)
            .context("merge record not found for active task run")?;
        if merge.status != MergeStatus::Conflicted || merge.task_run_id != run.id {
            bail!("merge record is not the exact active conflict");
        }
        let active_count = self
            .store
            .list_merge_records(&run.id)
            .await?
            .into_iter()
            .filter(|record| record.status == MergeStatus::Conflicted)
            .count();
        if active_count != 1 {
            bail!("task run must have exactly one conflicted merge");
        }
        let evidence = merge
            .evidence
            .as_ref()
            .context("conflicted merge has no versioned evidence")?;
        let manifest = evidence
            .conflict_manifest
            .as_ref()
            .context("conflicted merge has no durable manifest")?;
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("task branch lease not found")?;
        self.ensure_process_lease_owned(&run)?;
        if lease.expected_head != run.expected_head
            || lease.branch != run.branch
            || lease.git_common_dir != run.git_common_dir
            || merge.expected_head != run.expected_head
            || merge.source_commit != manifest.merge_head
        {
            bail!("task run, lease, merge record, and manifest identity drifted");
        }
        validate_repository_merge_state(&run.workspace_root, &run, &merge.source_commit).await?;
        let unmerged = read_unmerged(&run.workspace_root).await?;
        validate_unmerged_scope(&unmerged, &manifest.conflicts)?;
        validate_changed_path_scope(
            &run.workspace_root,
            &evidence.changed_files,
            &manifest.conflicts,
            &unmerged,
        )
        .await?;
        validate_stage_zero_scope(&run.workspace_root, manifest).await?;
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
        Ok((
            TaskMergeScope {
                #[cfg(test)]
                origin_phase: evidence.origin_phase,
                run,
                lease,
                work_unit,
                outcome,
                delivery,
                merge,
            },
            unmerged,
        ))
    }
}

pub(super) async fn validate_changed_path_scope(
    workspace: &str,
    changed_files: &[String],
    conflicts: &[ConflictEntry],
    unmerged: &BTreeMap<String, Vec<MergeIndexStage>>,
) -> Result<()> {
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
    if !status.success {
        bail!(
            "failed to inspect conflict workspace: {}",
            status.stderr_lossy()
        );
    }
    let mut allowed = changed_files
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    for conflict in conflicts {
        allowed.insert(&conflict.path);
        allowed.extend(conflict.rename_source.as_deref());
        allowed.extend(conflict.rename_destination.as_deref());
    }
    for entry in parse_porcelain_entries(&status.stdout)? {
        if entry.status == "??" {
            bail!(
                "conflict workspace contains untracked path `{}`",
                entry.path
            );
        }
        if !allowed.contains(entry.path.as_str()) {
            bail!(
                "conflict workspace contains unrelated path `{}`",
                entry.path
            );
        }
        let status = entry.status.as_bytes();
        if !unmerged.contains_key(&entry.path) && status.get(1).is_some_and(|value| *value != b' ')
        {
            bail!(
                "resolved conflict workspace contains unstaged edit `{}`",
                entry.path
            );
        }
        if let Some(original) = entry.original_path
            && !allowed.contains(original.as_str())
        {
            bail!("conflict workspace contains unrelated rename source `{original}`");
        }
    }
    Ok(())
}

async fn validate_stage_zero_scope(
    workspace: &str,
    manifest: &crate::studio::task_coordinator::ConflictManifest,
) -> Result<()> {
    let output = run_git(
        workspace,
        vec!["ls-files".into(), "--stage".into(), "-z".into()],
    )
    .await?;
    if !output.success {
        bail!(
            "failed to inspect conflict index: {}",
            output.stderr_lossy()
        );
    }
    let mut conflict_paths = manifest
        .conflicts
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<HashSet<_>>();
    for conflict in &manifest.conflicts {
        conflict_paths.extend(conflict.rename_source.as_deref());
        conflict_paths.extend(conflict.rename_destination.as_deref());
    }
    let expected = manifest
        .index_stage_zero_entries
        .iter()
        .filter(|entry| !conflict_paths.contains(entry.path.as_str()))
        .map(|entry| {
            (
                entry.path.clone(),
                (entry.mode.clone(), entry.object_id.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut actual = BTreeMap::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("index path separator missing")?;
        let metadata = std::str::from_utf8(&record[..tab])?;
        let path = std::str::from_utf8(&record[tab + 1..])?.replace('\\', "/");
        let mut fields = metadata.split_whitespace();
        let mode = fields.next().context("index mode missing")?;
        let object_id = fields.next().context("index object id missing")?;
        let stage = fields.next().context("index stage missing")?;
        if stage == "0" && !conflict_paths.contains(path.as_str()) {
            actual.insert(path, (mode.to_string(), object_id.to_string()));
        }
    }
    if actual != expected {
        bail!("non-conflict stage-zero index entries drifted from the durable manifest");
    }
    Ok(())
}

pub(super) fn validate_conflict_path(path: &str) -> Result<()> {
    let candidate = Path::new(path);
    if path.is_empty()
        || path.trim() != path
        || path.contains('\\')
        || candidate.is_absolute()
        || candidate
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("conflict path must be normalized and workspace-relative");
    }
    Ok(())
}

pub(super) fn conflict_entry<'a>(
    scope: &'a TaskMergeScope,
    path: &str,
) -> Result<&'a ConflictEntry> {
    validate_conflict_path(path)?;
    scope
        .merge
        .evidence
        .as_ref()
        .and_then(|evidence| evidence.conflict_manifest.as_ref())
        .and_then(|manifest| manifest.conflicts.iter().find(|entry| entry.path == path))
        .context("path is not authorized by the active conflict manifest")
}

pub(super) async fn read_unmerged(
    workspace: impl AsRef<Path>,
) -> Result<BTreeMap<String, Vec<MergeIndexStage>>> {
    let output = run_git(workspace, vec!["ls-files".into(), "-u".into(), "-z".into()]).await?;
    if !output.success {
        bail!("failed to read unmerged index: {}", output.stderr_lossy());
    }
    parse_unmerged_entries(&output.stdout)
}

async fn validate_repository_merge_state(
    workspace: &str,
    run: &crate::studio::task_coordinator::TaskRunRecord,
    source_commit: &str,
) -> Result<()> {
    let snapshot =
        crate::studio::task_coordinator::git::inspect_repository(workspace, false).await?;
    if normalized_path(&snapshot.git_common_dir) != normalized_path(Path::new(&run.git_common_dir))
        || snapshot.branch != run.branch
        || snapshot.head != run.expected_head
    {
        bail!("conflict repository identity, branch, or HEAD drifted");
    }
    let merge_head = checked_git(
        workspace,
        vec!["rev-parse".into(), "--verify".into(), "MERGE_HEAD".into()],
    )
    .await?;
    if merge_head != source_commit {
        bail!("MERGE_HEAD no longer matches the active conflict source commit");
    }
    Ok(())
}

fn validate_unmerged_scope(
    unmerged: &BTreeMap<String, Vec<MergeIndexStage>>,
    conflicts: &[ConflictEntry],
) -> Result<()> {
    let allowed = conflicts
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<HashSet<_>>();
    for (path, stages) in unmerged {
        if !allowed.contains(path.as_str()) {
            bail!("unmerged index contains unrelated path `{path}`");
        }
        let expected = conflicts
            .iter()
            .find(|entry| entry.path == *path)
            .context("unmerged conflict is absent from manifest")?;
        if stages != &expected.stages {
            bail!("unmerged index stages drifted for `{path}`");
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
