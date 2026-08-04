use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use pl_core::path_safety::{metadata_if_real, real_directory_entries};

use super::super::git::run_git;
use super::scope::conflict_entry;
use crate::studio::task_coordinator::{
    ConflictBlob, ConflictListItem, ConflictReadOutput, MergeIndexStage, TaskCoordinator,
};

impl TaskCoordinator {
    pub(crate) async fn list_active_conflicts(
        &self,
        thread_id: &str,
        merge_id: &str,
    ) -> Result<Vec<ConflictListItem>> {
        let guard = self.lock_branch_mutation().await;
        self.ensure_branch_mutation_guard(&guard)?;
        let (scope, unmerged) = self.load_active_conflict_scope(thread_id, merge_id).await?;
        let manifest = scope
            .merge
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.conflict_manifest.as_ref())
            .context("conflicted merge has no durable manifest")?;
        Ok(manifest
            .conflicts
            .iter()
            .map(|entry| ConflictListItem {
                path: entry.path.clone(),
                kind: entry.kind,
                stages: entry.stages.clone(),
                resolved: !unmerged.contains_key(&entry.path),
                binary: entry.binary,
                rename_source: entry.rename_source.clone(),
                rename_destination: entry.rename_destination.clone(),
            })
            .collect())
    }

    pub(crate) async fn read_active_conflict(
        &self,
        thread_id: &str,
        merge_id: &str,
        path: &str,
    ) -> Result<ConflictReadOutput> {
        let guard = self.lock_branch_mutation().await;
        self.ensure_branch_mutation_guard(&guard)?;
        let (scope, _) = self.load_active_conflict_scope(thread_id, merge_id).await?;
        let entry = conflict_entry(&scope, path)?;
        let workspace = Path::new(&scope.run.workspace_root);
        let combined = run_git(
            workspace,
            vec!["diff".into(), "--cc".into(), "--".into(), path.to_string()],
        )
        .await?;
        if !combined.success {
            bail!(
                "failed to read combined conflict diff: {}",
                combined.stderr_lossy()
            );
        }
        Ok(ConflictReadOutput {
            merge_id: scope.merge.id.clone(),
            path: entry.path.clone(),
            kind: entry.kind,
            binary: entry.binary,
            base: read_stage_blob(workspace, &entry.stages, 1).await?,
            ours: read_stage_blob(workspace, &entry.stages, 2).await?,
            theirs: read_stage_blob(workspace, &entry.stages, 3).await?,
            combined_diff: combined.stdout_text()?,
            design_references: locate_design_references(workspace, path)?,
        })
    }
}

async fn read_stage_blob(
    workspace: &Path,
    stages: &[MergeIndexStage],
    stage: u8,
) -> Result<ConflictBlob> {
    let Some(item) = stages.iter().find(|item| item.stage == stage) else {
        return Ok(ConflictBlob {
            available: false,
            binary: false,
            content: None,
            object_id: None,
        });
    };
    if item.mode == "160000" {
        return Ok(ConflictBlob {
            available: true,
            binary: true,
            content: None,
            object_id: Some(item.object_id.clone()),
        });
    }
    let output = run_git(
        workspace,
        vec!["cat-file".into(), "blob".into(), item.object_id.clone()],
    )
    .await?;
    if !output.success {
        bail!("failed to read conflict blob: {}", output.stderr_lossy());
    }
    let content = String::from_utf8(output.stdout.clone()).ok();
    Ok(ConflictBlob {
        available: true,
        binary: content.is_none() || output.stdout.contains(&0),
        content,
        object_id: Some(item.object_id.clone()),
    })
}

fn locate_design_references(workspace: &Path, affected_path: &str) -> Result<Vec<String>> {
    let design = workspace.join("design");
    if !metadata_if_real(&design)
        .map_err(anyhow::Error::from)?
        .is_some_and(|metadata| metadata.is_dir())
    {
        return Ok(Vec::new());
    }
    let file_name = Path::new(affected_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(affected_path);
    let mut references = Vec::new();
    let mut pending = vec![design];
    while let Some(directory) = pending.pop() {
        for path in real_directory_entries(&directory)
            .with_context(|| format!("failed to inspect `{}`", directory.display()))?
        {
            let Some(metadata) = metadata_if_real(&path).map_err(anyhow::Error::from)? else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
                continue;
            }
            let content = fs::read_to_string(&path).unwrap_or_default();
            if content.contains(affected_path) || content.contains(file_name) {
                references.push(relative_design_path(workspace, path)?);
            }
        }
    }
    references.sort();
    references.dedup();
    Ok(references)
}

fn relative_design_path(workspace: &Path, path: PathBuf) -> Result<String> {
    Ok(path
        .strip_prefix(workspace)
        .context("design reference escaped workspace")?
        .to_string_lossy()
        .replace('\\', "/"))
}
