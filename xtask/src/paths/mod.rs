use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub(crate) fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .context("pl-xtask manifest directory has no parent workspace root")?
        .to_path_buf();
    ensure_workspace_shape(&root)?;
    Ok(root)
}

pub(crate) fn flutter_app_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("code").join("pure-studio-flutter")
}

pub(crate) fn release_dist_dir(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join("dist")
        .join("pure-studio-flutter-release")
}

pub(crate) fn ensure_workspace_shape(workspace_root: &Path) -> Result<()> {
    let app_dir = flutter_app_dir(workspace_root);
    if !workspace_root.join("Cargo.toml").is_file() {
        bail!(
            "workspace root does not contain Cargo.toml: {}",
            workspace_root.display()
        );
    }
    if !app_dir.join("pubspec.yaml").is_file() {
        bail!(
            "Flutter app directory is invalid; workspace root: {}, Flutter app dir: {}",
            workspace_root.display(),
            app_dir.display()
        );
    }
    Ok(())
}
