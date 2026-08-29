//! Remote helper build and signed release-asset materialization.

mod assets;
mod build;

use std::path::{Path, PathBuf};

use anyhow::Result;
use semver::Version;

use crate::cli::BuildRemoteHelperOptions;

pub(crate) const AARCH64_TARGET: &str = "aarch64-unknown-linux-musl";
pub(crate) const X86_64_TARGET: &str = "x86_64-unknown-linux-musl";
pub(crate) const SUPPORTED_TARGETS: [&str; 2] = [AARCH64_TARGET, X86_64_TARGET];
pub(crate) const HELPER_FILE_NAME: &str = "pl-remote-helper";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeHelperPaths {
    aarch64: PathBuf,
    x86_64: PathBuf,
}

impl RuntimeHelperPaths {
    pub(crate) fn new(aarch64: PathBuf, x86_64: PathBuf) -> Self {
        Self { aarch64, x86_64 }
    }

    pub(crate) fn aarch64(&self) -> &Path {
        &self.aarch64
    }

    pub(crate) fn x86_64(&self) -> &Path {
        &self.x86_64
    }
}

pub(crate) fn build(options: BuildRemoteHelperOptions) -> Result<()> {
    build::build(options)
}

pub(crate) fn build_all_for_run(workspace_root: &Path) -> Result<RuntimeHelperPaths> {
    build::build_targets(workspace_root, &SUPPORTED_TARGETS)?;
    Ok(RuntimeHelperPaths::new(
        local_helper_path(workspace_root, AARCH64_TARGET),
        local_helper_path(workspace_root, X86_64_TARGET),
    ))
}

pub(crate) fn install_release_bundle(
    workspace_root: &Path,
    version: &Version,
    bundle_root: &Path,
) -> Result<()> {
    assets::install_release_bundle(workspace_root, version, bundle_root)
}

pub(crate) fn stage_release_assets(
    workspace_root: &Path,
    version: &Version,
    release_dir: &Path,
) -> Result<()> {
    assets::stage_release_assets(workspace_root, version, release_dir)
}

pub(crate) fn release_asset_name(version: &Version, target: &str) -> String {
    format!("Pure-Remote-Helper-{version}-{target}")
}

pub(crate) fn local_helper_path(workspace_root: &Path, target: &str) -> PathBuf {
    workspace_root
        .join("dist")
        .join("remote-helper")
        .join(target)
        .join(HELPER_FILE_NAME)
}
