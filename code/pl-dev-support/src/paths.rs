//! Workspace path discovery shared by the engineering tools.

use anyhow::{Context, bail};
use std::path::{Path, PathBuf};

pub use anyhow::Result;

/// Returns the Pure-Lang workspace root that contains this crate.
///
/// The crate lives at `<workspace>/code/pl-dev-support`, so the root is two
/// levels above `CARGO_MANIFEST_DIR`. The discovered root is validated by
/// [`ensure_workspace_shape`] before being returned, so a moved or vendored
/// checkout fails fast instead of silently pointing at `code/`.
///
/// # Errors
/// Returns an error when the manifest directory is not nested two levels
/// below a workspace root that passes [`ensure_workspace_shape`].
pub fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .context("pl-dev-support manifest directory has no workspace root parent")?
        .to_path_buf();
    ensure_workspace_shape(&root)?;
    Ok(root)
}

/// Returns the Studio (Flutter app) directory under [workspace_root].
pub fn studio_app_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("code").join("anywork")
}

/// Returns the release staging directory under [workspace_root].
pub fn release_dist_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("dist").join("anywork-release")
}

/// Validates that [workspace_root] looks like the Pure-Lang workspace.
///
/// # Errors
/// Returns an error when the root has no root `Cargo.toml` or when the Studio
/// app directory has no `pubspec.yaml`.
pub fn ensure_workspace_shape(workspace_root: &Path) -> Result<()> {
    let app_dir = studio_app_dir(workspace_root);
    if !workspace_root.join("Cargo.toml").is_file() {
        bail!(
            "workspace root does not contain Cargo.toml: {}",
            workspace_root.display()
        );
    }
    if !app_dir.join("pubspec.yaml").is_file() {
        bail!(
            "Studio app directory is invalid; workspace root: {}, Studio app dir: {}",
            workspace_root.display(),
            app_dir.display()
        );
    }
    Ok(())
}
