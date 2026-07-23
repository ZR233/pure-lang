use std::path::Path;

use anyhow::{Context, Result, bail};
use pl_core::path_safety::validate_path_for_write_async;

use super::super::git::run_git;
use super::ConflictResolutionChoice;
use super::scope::{conflict_entry, read_unmerged, validate_conflict_path};
use crate::studio::task_coordinator::{
    ConflictKind, ConflictResolveOutput, MergeIndexStage, TaskCoordinator,
};
use crate::tool::{
    CodexPatchHunk, LocalWorkspaceFileBackend, apply_patch_to_backend, parse_codex_patch,
};

impl TaskCoordinator {
    pub(crate) async fn resolve_active_conflict(
        &self,
        session_id: &str,
        merge_id: &str,
        path: &str,
        choice: ConflictResolutionChoice,
    ) -> Result<ConflictResolveOutput> {
        let guard = self.lock_branch_mutation().await;
        self.ensure_branch_mutation_guard(&guard)?;
        let (scope, unmerged) = self
            .load_active_conflict_scope(session_id, merge_id)
            .await?;
        let entry = conflict_entry(&scope, path)?.clone();
        if !unmerged.contains_key(path) {
            bail!("conflict path is already resolved");
        }
        if matches!(choice, ConflictResolutionChoice::Patch(_))
            && (entry.binary
                || matches!(
                    entry.kind,
                    ConflictKind::RenameDelete | ConflictKind::ModifyDelete
                ))
        {
            bail!("binary and delete/rename conflicts require ours, theirs, or delete");
        }
        let workspace = Path::new(&scope.run.workspace_root);
        validate_real_path_ancestors(workspace, path).await?;
        match &choice {
            ConflictResolutionChoice::Patch(patch) => {
                validate_exact_patch(patch, path)?;
                let backend =
                    LocalWorkspaceFileBackend::new(workspace.to_path_buf(), false).await?;
                let output = apply_patch_to_backend(&backend, ".".to_string(), patch).await?;
                if output.changed_files != [path.to_string()] {
                    bail!("conflict patch modified a path outside the exact conflict target");
                }
                reject_conflict_markers(workspace, path).await?;
                stage_path(workspace, path).await?;
            }
            ConflictResolutionChoice::Ours => {
                write_stage_choice(workspace, path, &entry.stages, 2).await?;
                reject_conflict_markers_if_text(workspace, path, entry.binary).await?;
                stage_path(workspace, path).await?;
            }
            ConflictResolutionChoice::Theirs => {
                write_stage_choice(workspace, path, &entry.stages, 3).await?;
                reject_conflict_markers_if_text(workspace, path, entry.binary).await?;
                stage_path(workspace, path).await?;
            }
            ConflictResolutionChoice::Delete => {
                let output =
                    run_git(workspace, vec!["rm".into(), "--".into(), path.to_string()]).await?;
                if !output.success {
                    bail!(
                        "failed to stage conflict deletion: {}",
                        output.stderr_lossy()
                    );
                }
            }
        }
        let unresolved = read_unmerged(workspace).await?;
        if unresolved.contains_key(path) {
            bail!("resolution did not remove the target's unmerged index entries");
        }
        Ok(ConflictResolveOutput {
            merge_id: scope.merge.id,
            path: path.to_string(),
            strategy: choice.label().to_string(),
            unresolved_paths: unresolved.into_keys().collect(),
        })
    }
}

fn validate_exact_patch(patch: &str, target: &str) -> Result<()> {
    let hunks = parse_codex_patch(patch).map_err(anyhow::Error::from)?;
    match hunks.as_slice() {
        [
            CodexPatchHunk::Update {
                path,
                move_path: None,
                ..
            },
        ] if path == target => Ok(()),
        _ => bail!("conflict patch must contain exactly one update for the exact target path"),
    }
}

async fn write_stage_choice(
    workspace: &Path,
    path: &str,
    stages: &[MergeIndexStage],
    stage: u8,
) -> Result<()> {
    let item = stages
        .iter()
        .find(|item| item.stage == stage)
        .with_context(|| format!("selected stage {stage} is unavailable for `{path}`"))?;
    if item.mode == "160000" {
        bail!("submodule conflict choices are not supported by file resolution");
    }
    let blob = run_git(
        workspace,
        vec!["cat-file".into(), "blob".into(), item.object_id.clone()],
    )
    .await?;
    if !blob.success {
        bail!(
            "failed to read selected conflict stage: {}",
            blob.stderr_lossy()
        );
    }
    let absolute = workspace.join(path);
    if let Some(parent) = absolute.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&absolute, blob.stdout)
        .await
        .with_context(|| format!("failed to write selected conflict stage `{path}`"))
}

async fn stage_path(workspace: &Path, path: &str) -> Result<()> {
    let output = run_git(workspace, vec!["add".into(), "--".into(), path.to_string()]).await?;
    if !output.success {
        bail!(
            "failed to stage resolved conflict: {}",
            output.stderr_lossy()
        );
    }
    Ok(())
}

async fn reject_conflict_markers_if_text(workspace: &Path, path: &str, binary: bool) -> Result<()> {
    if binary {
        return Ok(());
    }
    reject_conflict_markers(workspace, path).await
}

pub(super) async fn reject_conflict_markers(workspace: &Path, path: &str) -> Result<()> {
    let content = tokio::fs::read_to_string(workspace.join(path))
        .await
        .with_context(|| format!("resolved text conflict `{path}` is not readable UTF-8"))?;
    if content.lines().any(|line| {
        line.starts_with("<<<<<<<") || line.starts_with("=======") || line.starts_with(">>>>>>>")
    }) {
        bail!("resolved text conflict still contains conflict markers");
    }
    Ok(())
}

async fn validate_real_path_ancestors(workspace: &Path, path: &str) -> Result<()> {
    validate_conflict_path(path)?;
    validate_path_for_write_async(workspace, &workspace.join(path))
        .await
        .context("conflict path traverses a symbolic link or Windows reparse point")
}
