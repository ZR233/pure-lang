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
    let (base, build) = pubspec
        .version
        .split_once('+')
        .context("pubspec version must use x.y.z+build format")?;
    if build.is_empty() || !build.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("pubspec build number must contain decimal digits");
    }
    let version = Version::parse(base)
        .with_context(|| format!("invalid pubspec application version: {base}"))?;
    if base.starts_with('v') || !version.pre.is_empty() || !version.build.is_empty() {
        bail!("pubspec application version must be stable SemVer");
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_pubspec_version_as_single_source_of_truth() -> Result<()> {
        assert_eq!(
            parse("name: studio\nversion: 2.3.4+19\n")?,
            Version::parse("2.3.4")?
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_or_invalid_build_number() {
        assert!(parse("version: 1.2.3\n").is_err());
        assert!(parse("version: 1.2.3+beta\n").is_err());
        assert!(parse("version: 1.2.3-rc.1+2\n").is_err());
    }
}
