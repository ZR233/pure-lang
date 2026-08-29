//! Remote helper build and GUI bridge embedding inputs.

mod assets;
mod build;

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli::BuildRemoteHelperOptions;

pub(crate) const AARCH64_TARGET: &str = "aarch64-unknown-linux-musl";
pub(crate) const X86_64_TARGET: &str = "x86_64-unknown-linux-musl";
pub(crate) const SUPPORTED_TARGETS: [&str; 2] = [AARCH64_TARGET, X86_64_TARGET];
pub(crate) const HELPER_FILE_NAME: &str = "pl-remote-helper";

pub(crate) fn build(options: BuildRemoteHelperOptions) -> Result<()> {
    build::build(options)
}

pub(crate) fn prepare_for_embedding(workspace_root: &Path) -> Result<()> {
    assets::prepare(workspace_root)
}

pub(crate) fn local_helper_path(workspace_root: &Path, target: &str) -> PathBuf {
    workspace_root
        .join("dist")
        .join("remote-helper")
        .join(target)
        .join(HELPER_FILE_NAME)
}
