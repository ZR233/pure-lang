//! Windows stable release orchestration.

mod manifest;
mod package;

use crate::cli::{ReleaseGuiAction, ReleaseGuiOptions};
use crate::paths;
use crate::studio_version;
use anyhow::{Context, Result, bail};
use semver::Version;
use std::path::{Path, PathBuf};

const PLATFORM: &str = "windows-x86_64";

pub(crate) fn run(options: ReleaseGuiOptions) -> Result<()> {
    let version = validate_version(&options.version)?;
    let workspace_root = paths::workspace_root()?;
    ensure_pubspec_version(&workspace_root, &version)?;
    let release_dir = release_dir(&workspace_root, &version);

    match options.action {
        ReleaseGuiAction::Stage => package::stage(&workspace_root, &release_dir, &version),
        ReleaseGuiAction::Finalize => manifest::finalize(&workspace_root, &release_dir, &version),
        ReleaseGuiAction::Verify => manifest::verify(&workspace_root, &release_dir, &version),
    }
}

fn validate_version(raw: &str) -> Result<Version> {
    let version = Version::parse(raw).with_context(|| format!("invalid release version: {raw}"))?;
    if !version.pre.is_empty() || !version.build.is_empty() || raw.starts_with('v') {
        bail!("release version must be stable SemVer without v prefix or metadata: {raw}");
    }
    Ok(version)
}

fn ensure_pubspec_version(workspace_root: &Path, version: &Version) -> Result<()> {
    let actual = studio_version::read(&paths::flutter_app_dir(workspace_root))?;
    if &actual != version {
        bail!("release version {version} does not match pubspec.yaml base version {actual}");
    }
    Ok(())
}

fn release_dir(workspace_root: &Path, version: &Version) -> PathBuf {
    workspace_root
        .join("dist")
        .join("studio-release")
        .join(version.to_string())
}

fn asset_name(version: &Version, kind: &str) -> String {
    format!("Pure-Studio-{version}-{PLATFORM}-{kind}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn rejects_prerelease_and_build_metadata() {
        assert!(validate_version("1.0.0-rc.1").is_err());
        assert!(validate_version("1.0.0+1").is_err());
        assert!(validate_version("v1.0.0").is_err());
    }

    #[test]
    fn uses_fixed_release_asset_names() -> Result<()> {
        let version = validate_version("1.2.3")?;
        assert_eq!(
            asset_name(&version, "setup.exe"),
            "Pure-Studio-1.2.3-windows-x86_64-setup.exe"
        );
        assert_eq!(
            asset_name(&version, "portable.zip"),
            "Pure-Studio-1.2.3-windows-x86_64-portable.zip"
        );
        Ok(())
    }
}
