use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioVersionBump {
    Patch,
    Minor,
    Major,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioVersion {
    pub(crate) release: Version,
    pub(crate) build_number: u64,
}

#[derive(Debug, Deserialize)]
struct FlutterPubspec {
    version: String,
}

pub(crate) fn read(app_dir: &Path) -> Result<StudioVersion> {
    let path = app_dir.join("pubspec.yaml");
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read Flutter pubspec: {}", path.display()))?;
    parse(&content)
}

pub(crate) fn prepare(app_dir: &Path, bump: StudioVersionBump) -> Result<StudioVersion> {
    let path = app_dir.join("pubspec.yaml");
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read Flutter pubspec: {}", path.display()))?;
    let (content, version) = prepare_content(&content, bump)?;
    fs::write(&path, content)
        .with_context(|| format!("failed to update Flutter pubspec: {}", path.display()))?;
    Ok(version)
}

impl StudioVersion {
    pub(crate) fn pubspec_value(&self) -> String {
        format!("{}+{}", self.release, self.build_number)
    }

    fn bumped(&self, bump: StudioVersionBump) -> Result<Self> {
        let (major, minor, patch) = match bump {
            StudioVersionBump::Patch => (
                self.release.major,
                self.release.minor,
                self.release
                    .patch
                    .checked_add(1)
                    .context("Studio patch version overflow")?,
            ),
            StudioVersionBump::Minor => (
                self.release.major,
                self.release
                    .minor
                    .checked_add(1)
                    .context("Studio minor version overflow")?,
                0,
            ),
            StudioVersionBump::Major => (
                self.release
                    .major
                    .checked_add(1)
                    .context("Studio major version overflow")?,
                0,
                0,
            ),
        };
        let build_number = self
            .build_number
            .checked_add(1)
            .context("Studio build number overflow")?;
        Ok(Self {
            release: Version::new(major, minor, patch),
            build_number,
        })
    }
}

fn parse(content: &str) -> Result<StudioVersion> {
    let pubspec: FlutterPubspec =
        serde_norway::from_str(content).context("failed to parse Flutter pubspec.yaml")?;
    let (base, build) = pubspec
        .version
        .split_once('+')
        .context("pubspec version must use x.y.z+build format")?;
    if build.is_empty() || !build.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("pubspec build number must contain decimal digits");
    }
    let build_number = build
        .parse::<u64>()
        .context("pubspec build number must contain decimal digits")?;
    let version = Version::parse(base)
        .with_context(|| format!("invalid pubspec application version: {base}"))?;
    if base.starts_with('v') || !version.pre.is_empty() || !version.build.is_empty() {
        bail!("pubspec application version must be stable SemVer");
    }
    Ok(StudioVersion {
        release: version,
        build_number,
    })
}

fn prepare_content(content: &str, bump: StudioVersionBump) -> Result<(String, StudioVersion)> {
    let version = parse(content)?.bumped(bump)?;
    let mut version_lines = 0_u8;
    let mut updated = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix("\r\n")
            .map(|body| (body, "\r\n"))
            .or_else(|| line.strip_suffix('\n').map(|body| (body, "\n")))
            .unwrap_or((line, ""));
        if body.starts_with("version:") {
            version_lines = version_lines.saturating_add(1);
            updated.push_str("version: ");
            updated.push_str(&version.pubspec_value());
            if let Some(comment) = body.find(" #").map(|index| &body[index..]) {
                updated.push_str(comment);
            }
        } else {
            updated.push_str(body);
        }
        updated.push_str(newline);
    }
    if version_lines != 1 {
        bail!("pubspec.yaml must contain exactly one top-level version field");
    }
    Ok((updated, version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_pubspec_version_as_single_source_of_truth() -> Result<()> {
        assert_eq!(
            parse("name: studio\nversion: 2.3.4+19\n")?,
            StudioVersion {
                release: Version::parse("2.3.4")?,
                build_number: 19,
            }
        );
        Ok(())
    }

    #[test]
    fn prepares_patch_minor_and_major_versions() -> Result<()> {
        for (bump, expected) in [
            (StudioVersionBump::Patch, "1.2.4+8"),
            (StudioVersionBump::Minor, "1.3.0+8"),
            (StudioVersionBump::Major, "2.0.0+8"),
        ] {
            let content = "name: studio\r\nversion: 1.2.3+7\r\npublish_to: none\r\n";
            let (updated, version) = prepare_content(content, bump)?;
            assert_eq!(
                updated,
                format!("name: studio\r\nversion: {expected}\r\npublish_to: none\r\n")
            );
            assert_eq!(version.pubspec_value(), expected);
        }
        Ok(())
    }

    #[test]
    fn prepare_preserves_an_inline_version_comment() -> Result<()> {
        let content = "version: 1.2.3+7 # Windows build version\n";
        let (updated, version) = prepare_content(content, StudioVersionBump::Patch)?;
        assert_eq!(updated, "version: 1.2.4+8 # Windows build version\n");
        assert_eq!(version.pubspec_value(), "1.2.4+8");
        Ok(())
    }

    #[test]
    fn prepare_updates_the_flutter_pubspec_file() -> Result<()> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let app_dir = std::env::temp_dir().join(format!(
            "pure-studio-version-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&app_dir)?;
        let pubspec = app_dir.join("pubspec.yaml");
        fs::write(&pubspec, "name: studio\nversion: 3.4.5+9\n")?;

        let version = prepare(&app_dir, StudioVersionBump::Minor)?;

        assert_eq!(version.pubspec_value(), "3.5.0+10");
        assert_eq!(
            fs::read_to_string(&pubspec)?,
            "name: studio\nversion: 3.5.0+10\n"
        );
        fs::remove_dir_all(app_dir)?;
        Ok(())
    }

    #[test]
    fn rejects_missing_invalid_or_duplicate_versions() {
        assert!(parse("version: 1.2.3\n").is_err());
        assert!(parse("version: 1.2.3+beta\n").is_err());
        assert!(parse("version: 1.2.3++2\n").is_err());
        assert!(parse("version: 1.2.3-rc.1+2\n").is_err());
        assert!(
            prepare_content(
                "version: 1.2.3+1\nversion: 1.2.4+2\n",
                StudioVersionBump::Patch
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_component_and_build_number_overflow() {
        assert!(
            prepare_content(
                "version: 1.2.18446744073709551615+1\n",
                StudioVersionBump::Patch
            )
            .is_err()
        );
        assert!(
            prepare_content(
                "version: 18446744073709551615.2.3+1\n",
                StudioVersionBump::Major
            )
            .is_err()
        );
        assert!(
            prepare_content(
                "version: 1.2.3+18446744073709551615\n",
                StudioVersionBump::Minor
            )
            .is_err()
        );
    }
}
