use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};

use super::git::{git_path_is_ignored, run_git_checked};
use super::{OriginalPath, ValidatedDesignPatch};
use crate::tool::{CodexPatchHunk, parse_codex_patch};

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
    reject_symlink_ancestors(workspace, path).await?;
    if git_path_is_ignored(workspace, &normalized).await? {
        bail!("design patch path is ignored by Git: `{normalized}`");
    }
    Ok(normalized)
}

pub(super) async fn reject_symlink_ancestors(workspace: &Path, relative: &Path) -> Result<()> {
    let mut candidate = workspace.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            bail!("design patch path contains an invalid component");
        };
        candidate.push(part);
        match tokio::fs::symlink_metadata(&candidate).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "design patch path traverses symbolic link `{}`",
                    candidate.display()
                )
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect design path `{}`", candidate.display())
                });
            }
        }
    }
    Ok(())
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
    for (path, original) in originals {
        let absolute = workspace.join(path);
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
