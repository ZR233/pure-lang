use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::barriers::MergeFailurePoint;
use super::conflict_index::parse_unmerged_entries;
use super::conflict_status::parse_porcelain_entries;
use super::failure::MergeFailureStage;
use super::git::{checked_git, run_git};
use super::recovery::{
    capture_conflict_workspace_evidence, read_conflict_status, validate_conflict_workspace_evidence,
};
use crate::studio::task_coordinator::{
    ConflictEntry, ConflictKind, ConflictManifest, ConflictTaskMerge, MergeCleanupEvidence,
    MergeIndexEntry, MergeIndexStage, MergeRecord, MergeStatus, TaskCoordinator,
    TaskMergeAgentOutput, TaskMergeScope, TaskRunRecord,
};

#[cfg(test)]
#[path = "tests/conflict_recovery.rs"]
mod recovery_tests;

impl TaskCoordinator {
    pub(super) async fn persist_merge_conflict(
        &self,
        scope: &TaskMergeScope,
        workspace: &Path,
    ) -> Result<TaskMergeAgentOutput> {
        let manifest = match self.inject_merge_failure(MergeFailurePoint::ConflictManifest) {
            Ok(()) => match build_conflict_manifest(scope, workspace).await {
                Ok(manifest) => manifest,
                Err(error) => {
                    return self
                        .handle_merge_stage_failure(
                            scope,
                            workspace,
                            Vec::new(),
                            error.context("build merge conflict manifest"),
                            MergeFailureStage::Conflict,
                        )
                        .await;
                }
            },
            Err(error) => {
                return self
                    .handle_merge_stage_failure(
                        scope,
                        workspace,
                        Vec::new(),
                        error.context("build merge conflict manifest"),
                        MergeFailureStage::Conflict,
                    )
                    .await;
            }
        };
        let conflict_files = manifest
            .conflicts
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let persistence = match self.inject_merge_failure(MergeFailurePoint::ConflictPersistence) {
            Ok(()) => {
                self.store
                    .conflict_task_merge(ConflictTaskMerge {
                        merge_id: scope.merge.id.clone(),
                        manifest,
                    })
                    .await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = persistence {
            return self
                .handle_merge_stage_failure(
                    scope,
                    workspace,
                    Vec::new(),
                    error.context("persist merge conflict manifest"),
                    MergeFailureStage::Conflict,
                )
                .await;
        }
        Ok(TaskMergeAgentOutput {
            merge_id: scope.merge.id.clone(),
            status: MergeStatus::Conflicted,
            previous_head: scope.run.expected_head.clone(),
            new_head: None,
            agent_id: scope.outcome.agent_id.clone(),
            source_commit: scope.delivery.head_commit.clone(),
            changed_files: scope.delivery.changed_files.clone(),
            verification: Vec::new(),
            cleanup: MergeCleanupEvidence {
                status: "pendingConflictResolution".to_string(),
                detail: None,
            },
            conflict_files,
        })
    }
}

pub(super) async fn validate_merge_failure_workspace(
    scope: &TaskMergeScope,
    workspace: &Path,
) -> Result<()> {
    let status = read_conflict_status(workspace).await?;
    let allowed = scope
        .delivery
        .changed_files
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    validate_conflict_status_scope(&status, &allowed)
}

async fn build_conflict_manifest(
    scope: &TaskMergeScope,
    workspace: &Path,
) -> Result<ConflictManifest> {
    let head = checked_git(workspace, vec!["rev-parse".into(), "HEAD".into()]).await?;
    let merge_head = checked_git(
        workspace,
        vec!["rev-parse".into(), "--verify".into(), "MERGE_HEAD".into()],
    )
    .await?;
    if head != scope.run.expected_head || merge_head != scope.delivery.head_commit {
        bail!("conflict Git state does not match the active merge");
    }
    let merge_base = checked_git(
        workspace,
        vec![
            "merge-base".into(),
            scope.run.expected_head.clone(),
            scope.delivery.head_commit.clone(),
        ],
    )
    .await?;
    let unmerged = run_git(workspace, vec!["ls-files".into(), "-u".into(), "-z".into()]).await?;
    if !unmerged.success {
        bail!("failed to read unmerged index: {}", unmerged.stderr_lossy());
    }
    let mut grouped = parse_unmerged_entries(&unmerged.stdout)?;
    if grouped.is_empty() {
        bail!("git merge failed without unmerged index entries");
    }
    let rename_pairs = rename_pairs(workspace, &merge_base, scope).await?;
    let mut conflicts = Vec::new();
    for (path, stages) in &mut grouped {
        stages.sort_by_key(|stage| stage.stage);
        let binary = stages_are_binary(workspace, stages).await?;
        let rename = rename_pairs
            .iter()
            .find(|(source, destination)| source == path || destination == path);
        let kind = classify_conflict(stages, binary, rename.is_some());
        conflicts.push(ConflictEntry {
            path: path.clone(),
            kind,
            stages: stages.clone(),
            worktree_object_id: None,
            binary,
            rename_source: rename.map(|pair| pair.0.clone()),
            rename_destination: rename.map(|pair| pair.1.clone()),
        });
    }
    conflicts.sort_by(|left, right| left.path.cmp(&right.path));
    validate_conflict_paths(&conflicts)?;
    validate_no_unrelated_merge_edits(scope, workspace, &conflicts).await?;
    let workspace_evidence = capture_conflict_workspace_evidence(workspace, &mut conflicts).await?;
    let auto_merged_entries = auto_merged_entries(workspace, &conflicts).await?;
    let pre_index_tree = scope
        .merge
        .evidence
        .as_ref()
        .map(|evidence| evidence.pre_index_tree.clone())
        .context("active merge record has no pre-index evidence")?;
    Ok(ConflictManifest {
        merge_head,
        merge_base,
        pre_index_tree,
        conflicts,
        status_porcelain_v1_z: workspace_evidence.status_porcelain_v1_z,
        index_stage_zero_entries: workspace_evidence.index_stage_zero_entries,
        auto_merged_entries,
    })
}

pub(crate) async fn validate_conflict_recovery(
    run: &TaskRunRecord,
    record: &MergeRecord,
) -> Result<()> {
    if record.status != MergeStatus::Conflicted
        || record.task_run_id != run.id
        || record.expected_head != run.expected_head
    {
        bail!("resolving-conflict run has no exact conflicted merge record");
    }
    let manifest = record
        .evidence
        .as_ref()
        .and_then(|evidence| evidence.conflict_manifest.as_ref())
        .context("conflicted merge has no durable manifest")?;
    let workspace = Path::new(&run.workspace_root);
    let head = checked_git(workspace, vec!["rev-parse".into(), "HEAD".into()]).await?;
    let merge_head = checked_git(
        workspace,
        vec!["rev-parse".into(), "--verify".into(), "MERGE_HEAD".into()],
    )
    .await?;
    let merge_base = checked_git(
        workspace,
        vec![
            "merge-base".into(),
            run.expected_head.clone(),
            record.source_commit.clone(),
        ],
    )
    .await?;
    if head != run.expected_head
        || merge_head != record.source_commit
        || merge_head != manifest.merge_head
        || merge_base != manifest.merge_base
    {
        bail!("durable conflict HEAD, MERGE_HEAD, or merge base drifted");
    }
    validate_conflict_workspace_evidence(workspace, manifest).await?;
    let unmerged = run_git(workspace, vec!["ls-files".into(), "-u".into(), "-z".into()]).await?;
    if !unmerged.success {
        bail!(
            "failed to read recovery unmerged index: {}",
            unmerged.stderr_lossy()
        );
    }
    let actual = parse_unmerged_entries(&unmerged.stdout)?;
    let expected = manifest
        .conflicts
        .iter()
        .map(|conflict| (conflict.path.clone(), conflict.stages.clone()))
        .collect::<BTreeMap<_, _>>();
    if actual != expected {
        bail!("durable conflict stage mode/object evidence drifted");
    }
    let auto_merged = auto_merged_entries(workspace, &manifest.conflicts).await?;
    if auto_merged != manifest.auto_merged_entries {
        bail!("durable conflict auto-merged index evidence drifted");
    }
    Ok(())
}

async fn stages_are_binary(workspace: &Path, stages: &[MergeIndexStage]) -> Result<bool> {
    for stage in stages {
        if stage.mode == "160000" {
            return Ok(true);
        }
        let blob = run_git(
            workspace,
            vec!["cat-file".into(), "blob".into(), stage.object_id.clone()],
        )
        .await?;
        if !blob.success {
            bail!("failed to inspect conflict blob: {}", blob.stderr_lossy());
        }
        if blob.stdout.contains(&0) || std::str::from_utf8(&blob.stdout).is_err() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn classify_conflict(stages: &[MergeIndexStage], binary: bool, rename: bool) -> ConflictKind {
    if binary {
        return ConflictKind::Binary;
    }
    let present = stages
        .iter()
        .map(|stage| stage.stage)
        .collect::<HashSet<_>>();
    if !present.contains(&1) && present.contains(&2) && present.contains(&3) {
        ConflictKind::AddAdd
    } else if rename && present.len() < 3 {
        ConflictKind::RenameDelete
    } else if present.contains(&1) && present.len() == 2 {
        ConflictKind::ModifyDelete
    } else {
        ConflictKind::Text
    }
}

async fn rename_pairs(
    workspace: &Path,
    merge_base: &str,
    scope: &TaskMergeScope,
) -> Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    for head in [&scope.run.expected_head, &scope.delivery.head_commit] {
        let output = run_git(
            workspace,
            vec![
                "diff".into(),
                "--name-status".into(),
                "-z".into(),
                "--find-renames".into(),
                format!("{merge_base}..{head}"),
            ],
        )
        .await?;
        if !output.success {
            bail!("failed to inspect merge renames: {}", output.stderr_lossy());
        }
        let fields = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|field| !field.is_empty())
            .collect::<Vec<_>>();
        let mut index = 0;
        while index < fields.len() {
            let status = std::str::from_utf8(fields[index])?;
            index += 1;
            if status.starts_with('R') {
                let source =
                    std::str::from_utf8(fields.get(index).context("rename source missing")?)?;
                let destination = std::str::from_utf8(
                    fields
                        .get(index + 1)
                        .context("rename destination missing")?,
                )?;
                pairs.push((source.replace('\\', "/"), destination.replace('\\', "/")));
                index += 2;
            } else {
                index += 1;
            }
        }
    }
    pairs.sort();
    pairs.dedup();
    Ok(pairs)
}

fn validate_conflict_paths(conflicts: &[ConflictEntry]) -> Result<()> {
    for conflict in conflicts {
        let path = Path::new(&conflict.path);
        if path.is_absolute()
            || conflict.path.contains('\\')
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            bail!("unsafe conflict path `{}`", conflict.path);
        }
    }
    Ok(())
}

async fn validate_no_unrelated_merge_edits(
    scope: &TaskMergeScope,
    workspace: &Path,
    conflicts: &[ConflictEntry],
) -> Result<()> {
    let status = read_conflict_status(workspace).await?;
    let mut allowed = scope
        .delivery
        .changed_files
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    for conflict in conflicts {
        allowed.insert(conflict.path.clone());
        allowed.extend(conflict.rename_source.iter().cloned());
        allowed.extend(conflict.rename_destination.iter().cloned());
    }
    validate_conflict_status_scope(&status, &allowed)
}

fn validate_conflict_status_scope(status: &[u8], allowed: &HashSet<String>) -> Result<()> {
    for entry in parse_porcelain_entries(status)? {
        if entry.status == "??" {
            bail!(
                "merge conflict contains untracked workspace path `{}`",
                entry.path
            );
        }
        if !allowed.contains(&entry.path) {
            bail!(
                "merge conflict produced unrelated workspace edit `{}`",
                entry.path
            );
        }
        if let Some(original_path) = entry.original_path
            && !allowed.contains(&original_path)
        {
            bail!("merge conflict produced unrelated rename source `{original_path}`");
        }
    }
    Ok(())
}

async fn auto_merged_entries(
    workspace: &Path,
    conflicts: &[ConflictEntry],
) -> Result<Vec<MergeIndexEntry>> {
    let conflict_paths = conflicts
        .iter()
        .map(|entry| &entry.path)
        .collect::<HashSet<_>>();
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
        bail!(
            "failed to inspect auto-merged paths: {}",
            staged.stderr_lossy()
        );
    }
    let all = run_git(
        workspace,
        vec!["ls-files".into(), "--stage".into(), "-z".into()],
    )
    .await?;
    if !all.success {
        bail!("failed to inspect merged index: {}", all.stderr_lossy());
    }
    let staged_paths = staged
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| std::str::from_utf8(path).map(str::to_string))
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    let mut entries = Vec::new();
    for record in all
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("index path missing")?;
        let metadata = std::str::from_utf8(&record[..tab])?;
        let path = std::str::from_utf8(&record[tab + 1..])?.replace('\\', "/");
        let mut fields = metadata.split_whitespace();
        let mode = fields.next().context("index mode missing")?;
        let object_id = fields.next().context("index object id missing")?;
        let stage = fields.next().context("index stage missing")?;
        if stage == "0" && staged_paths.contains(&path) && !conflict_paths.contains(&path) {
            entries.push(MergeIndexEntry {
                path,
                mode: mode.to_string(),
                object_id: object_id.to_string(),
            });
        }
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}
