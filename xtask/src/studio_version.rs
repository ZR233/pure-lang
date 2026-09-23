use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct FlutterPubspec {
    version: String,
}

pub(crate) fn read(app_dir: &Path) -> Result<Version> {
    let path = app_dir.join("pubspec.yaml");
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read Flutter pubspec: {}", path.display()))?;
    parse(&content)
}

fn parse(content: &str) -> Result<Version> {
    let pubspec: FlutterPubspec =
        serde_norway::from_str(content).context("failed to parse Flutter pubspec.yaml")?;
    let raw = pubspec.version;
    let version = Version::parse(&raw)
        .with_context(|| format!("invalid pubspec application version: {raw}"))?;
    if raw.starts_with('v') || !version.pre.is_empty() || !version.build.is_empty() {
        bail!("pubspec application version must be stable x.y.z SemVer without metadata");
    }
    if version.to_string() != raw {
        bail!("pubspec application version must use canonical x.y.z SemVer");
    }
    Ok(version)
}
