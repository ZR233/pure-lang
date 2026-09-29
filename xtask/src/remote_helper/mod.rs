//! Remote helper build and Flutter bundle resource staging.

mod assets;
mod build;

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli::BuildRemoteHelperOptions;

pub(crate) const AARCH64_TARGET: &str = "aarch64-unknown-linux-musl";
pub(crate) const X86_64_TARGET: &str = "x86_64-unknown-linux-musl";
pub(crate) const SUPPORTED_TARGETS: [&str; 2] = [AARCH64_TARGET, X86_64_TARGET];
pub(crate) const HELPER_FILE_NAME: &str = "pl-remote-helper";
pub(crate) const BUNDLE_DIR_ENV: &str = "ANYWORK_REMOTE_HELPER_DIR";

pub(crate) fn build(options: BuildRemoteHelperOptions) -> Result<()> {
    build::build(options)
}

/// Verifies both helper architectures and stages the compressed bundle resources.
///
/// Returns the staging directory consumed by the desktop CMake install rules.
pub(crate) fn prepare_bundle_resources(workspace_root: &Path) -> Result<std::path::PathBuf> {
    assets::prepare(workspace_root)
}

pub(crate) fn local_helper_path(workspace_root: &Path, target: &str) -> PathBuf {
    workspace_root
        .join("dist")
        .join("remote-helper")
        .join(target)
        .join(HELPER_FILE_NAME)
}

pub(crate) fn bundle_staging_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("dist").join("remote-helper-bundle")
}

/// Build-owned metadata binds the declared platform/protocol to the exact helper bytes.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HelperMetadata {
    target: String,
    worker_protocol_version: u32,
    sha256: String,
}
