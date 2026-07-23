use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};
use pl_core::path_safety::validate_path_for_write_async;

use super::git::{git_path_is_ignored, run_git_checked};
use super::{OriginalPath, ValidatedDesignPatch};
use crate::tool::{CodexPatchHunk, ToolPathPolicy, parse_codex_patch};

pub(super) async fn validate_design_patch(
    workspace: &Path,
    patch: &str,
) -> Result<ValidatedDesignPatch> {
    let hunks = parse_codex_patch(patch).map_err(anyhow::Error::from)?;
    if hunks.is_empty() {
        bail!("task_update_design patch must contain at least one hunk");
    }
    let mut paths = BTreeSet::new();
    for hunk in &hunks {
        match hunk {
            CodexPatchHunk::Add { path, .. } | CodexPatchHunk::Delete { path } => {
                paths.insert(validate_design_path(workspace, path).await?);
            }
            CodexPatchHunk::Update {
                path, move_path, ..
            } => {
                paths.insert(validate_design_path(workspace, path).await?);
                if let Some(move_path) = move_path {
                    paths.insert(validate_design_path(workspace, move_path).await?);
                }
            }
        }
    }
    Ok(ValidatedDesignPatch {
        patch: patch.to_string(),
        paths: paths.into_iter().collect(),
    })
}

pub(super) async fn validate_design_path(workspace: &Path, raw: &str) -> Result<String> {
    if raw.is_empty() || raw.trim() != raw || raw.contains('\\') {
        bail!("design patch path must be a normalized workspace-relative path: `{raw}`");
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        bail!("design patch path must be workspace-relative: `{raw}`");
    }
    let components = path.components().collect::<Vec<_>>();
    if components.len() < 2
        || !matches!(components.first(), Some(Component::Normal(part)) if *part == "design")
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("design patch path must be within design/**: `{raw}`");
    }
    let normalized = components
        .iter()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if normalized != raw {
        bail!("design patch path is not normalized: `{raw}`");
    }
    validate_real_path_ancestors(workspace, path).await?;
    if git_path_is_ignored(workspace, &normalized).await? {
        bail!("design patch path is ignored by Git: `{normalized}`");
    }
    Ok(normalized)
}

pub(super) async fn validate_real_path_ancestors(workspace: &Path, relative: &Path) -> Result<()> {
    validate_path_for_write_async(workspace, &workspace.join(relative))
        .await
        .context("design patch path traverses a symbolic link or Windows reparse point")
}

pub(super) async fn snapshot_paths(
    workspace: &Path,
    paths: &[String],
) -> Result<BTreeMap<String, OriginalPath>> {
    let mut originals = BTreeMap::new();
    for path in paths {
        let absolute = workspace.join(path);
        let original = match tokio::fs::read(&absolute).await {
            Ok(content) => OriginalPath::File(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => OriginalPath::Missing,
            Err(error) => return Err(error).context(format!("failed to snapshot `{path}`")),
        };
        originals.insert(path.clone(), original);
    }
    Ok(originals)
}

pub(super) async fn rollback_paths(
    workspace: &Path,
    originals: &BTreeMap<String, OriginalPath>,
    head: &str,
) -> Result<()> {
    let paths = originals.keys().map(String::as_str).collect::<Vec<_>>();
    if !paths.is_empty() {
        let mut args = vec!["reset", "--quiet", head, "--"];
        args.extend(paths.iter().copied());
        run_git_checked(workspace, &args).await?;
    }
    restore_originals(workspace, originals).await
}

pub(super) async fn restore_originals(
    workspace: &Path,
    originals: &BTreeMap<String, OriginalPath>,
) -> Result<()> {
    let path_policy = ToolPathPolicy::new(workspace.to_path_buf(), false, "task design rollback")?;
    for (path, original) in originals {
        let absolute = resolve_safe_restore_path(&path_policy, path).await?;
        match original {
            OriginalPath::Missing => match tokio::fs::remove_file(&absolute).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context(format!("failed to remove `{path}`")),
            },
            OriginalPath::File(content) => {
                if let Some(parent) = absolute.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&absolute, content)
                    .await
                    .with_context(|| format!("failed to restore `{path}`"))?;
            }
        }
    }
    Ok(())
}

async fn resolve_safe_restore_path(
    path_policy: &ToolPathPolicy,
    path: &str,
) -> Result<std::path::PathBuf> {
    validate_real_path_ancestors(path_policy.root(), Path::new(path)).await?;
    path_policy
        .resolve_for_write(path)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("rollback path `{path}` is no longer safely inside the workspace"))
}

pub(super) fn ensure_only_validated_design_changes(
    changed: &[String],
    validated: &[String],
) -> Result<()> {
    let validated = validated.iter().collect::<BTreeSet<_>>();
    for path in changed {
        if !validated.contains(path) || !is_normalized_design_path(path) {
            bail!("workspace contains a change outside the validated design patch: `{path}`");
        }
    }
    Ok(())
}

pub(super) fn is_normalized_design_path(path: &str) -> bool {
    let components = Path::new(path).components().collect::<Vec<_>>();
    components.len() >= 2
        && matches!(components.first(), Some(Component::Normal(part)) if *part == "design")
        && components
            .iter()
            .all(|component| matches!(component, Component::Normal(_)))
        && !path.contains('\\')
}
