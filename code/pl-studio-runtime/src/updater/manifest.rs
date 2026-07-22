use super::error::{StudioUpdateError, StudioUpdateErrorCode};
use super::types::{StudioUpdate, StudioUpdateAsset, StudioUpdateCheck};
use semver::Version;
use serde::Deserialize;
use url::Url;

pub(super) const LATEST_MANIFEST_URL: &str =
    "https://github.com/ZR233/pure-lang/releases/latest/download/latest.json";
pub(super) const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpdateManifest {
    schema_version: u32,
    version: String,
    published_at: i64,
    notes_url: String,
    platforms: UpdatePlatforms,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdatePlatforms {
    #[serde(rename = "windows-x86_64")]
    windows_x86_64: UpdateAssetManifest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateAssetManifest {
    url: String,
    size: u64,
    sha256: String,
    signature: String,
}

pub(super) fn evaluate_manifest(
    bytes: &[u8],
    current_version: &str,
) -> Result<StudioUpdateCheck, StudioUpdateError> {
    let manifest: UpdateManifest = serde_json::from_slice(bytes)
        .map_err(|error| invalid_manifest(format!("invalid update manifest JSON: {error}")))?;
    if manifest.schema_version != 1 {
        return Err(invalid_manifest(format!(
            "unsupported update schema {}",
            manifest.schema_version
        )));
    }
    let current = stable_version(current_version, "current application version")?;
    let available = stable_version(&manifest.version, "manifest version")?;
    if available <= current {
        return Ok(StudioUpdateCheck::UpToDate);
    }
    validate_notes_url(&manifest.notes_url, &available)?;
    let asset = validate_asset(manifest.platforms.windows_x86_64, &available)?;
    Ok(StudioUpdateCheck::Available(StudioUpdate {
        version: available.to_string(),
        published_at: manifest.published_at,
        notes_url: manifest.notes_url,
        installer: asset,
    }))
}

pub(super) fn validate_update(update: &StudioUpdate) -> Result<(), StudioUpdateError> {
    let version = stable_version(&update.version, "update version")?;
    validate_notes_url(&update.notes_url, &version)?;
    validate_asset(
        UpdateAssetManifest {
            url: update.installer.url.clone(),
            size: update.installer.size,
            sha256: update.installer.sha256.clone(),
            signature: update.installer.signature.clone(),
        },
        &version,
    )?;
    Ok(())
}

pub(super) fn validate_redirect_url(url: &Url) -> Result<(), StudioUpdateError> {
    validate_https_basics(url, /*allow_query*/ true)?;
    let host = url.host_str().unwrap_or_default();
    let allowed = host.eq_ignore_ascii_case("github.com")
        || host.eq_ignore_ascii_case("objects.githubusercontent.com")
        || host.eq_ignore_ascii_case("release-assets.githubusercontent.com");
    if !allowed {
        return Err(invalid_manifest(format!(
            "update redirect host is not allowed: {host}"
        )));
    }
    Ok(())
}

fn validate_asset(
    asset: UpdateAssetManifest,
    version: &Version,
) -> Result<StudioUpdateAsset, StudioUpdateError> {
    if asset.size == 0 || asset.size > MAX_INSTALLER_BYTES {
        return Err(StudioUpdateError::new(
            StudioUpdateErrorCode::DownloadTooLarge,
            format!("installer size {} is outside the allowed range", asset.size),
        ));
    }
    if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_manifest(
            "installer SHA-256 is not 64 hexadecimal characters",
        ));
    }
    let expected_name = format!("Pure-Studio-{version}-windows-x86_64-setup.exe");
    let expected_path = format!("/ZR233/pure-lang/releases/download/v{version}/{expected_name}");
    let url = parse_stable_url(&asset.url)?;
    if url.host_str() != Some("github.com") || url.path() != expected_path {
        return Err(invalid_manifest(
            "installer URL does not match version, tag, and asset name",
        ));
    }
    let signature = parse_stable_url(&asset.signature)?;
    if signature.host_str() != Some("github.com")
        || signature.path() != format!("{expected_path}.minisig")
    {
        return Err(invalid_manifest(
            "signature URL does not match installer URL",
        ));
    }
    Ok(StudioUpdateAsset {
        url: url.into(),
        size: asset.size,
        sha256: asset.sha256.to_ascii_lowercase(),
        signature: signature.into(),
    })
}

fn validate_notes_url(raw: &str, version: &Version) -> Result<(), StudioUpdateError> {
    let url = parse_stable_url(raw)?;
    let expected = format!("/ZR233/pure-lang/releases/tag/v{version}");
    if url.host_str() != Some("github.com") || url.path() != expected {
        return Err(invalid_manifest(
            "release notes URL does not match manifest version",
        ));
    }
    Ok(())
}

fn parse_stable_url(raw: &str) -> Result<Url, StudioUpdateError> {
    let url = Url::parse(raw)
        .map_err(|error| invalid_manifest(format!("invalid update URL: {error}")))?;
    validate_https_basics(&url, /*allow_query*/ false)?;
    Ok(url)
}

fn validate_https_basics(url: &Url, allow_query: bool) -> Result<(), StudioUpdateError> {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || (!allow_query && url.query().is_some())
        || url.host_str().is_none()
    {
        return Err(invalid_manifest(
            "update URL violates the HTTPS origin policy",
        ));
    }
    Ok(())
}

fn stable_version(raw: &str, label: &str) -> Result<Version, StudioUpdateError> {
    let version = Version::parse(raw)
        .map_err(|error| invalid_manifest(format!("invalid {label}: {error}")))?;
    if !version.pre.is_empty() || !version.build.is_empty() || raw.starts_with('v') {
        return Err(invalid_manifest(format!("{label} must be stable SemVer")));
    }
    Ok(version)
}

fn invalid_manifest(message: impl Into<String>) -> StudioUpdateError {
    StudioUpdateError::new(StudioUpdateErrorCode::InvalidManifest, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn manifest(version: &str, url: &str, signature: &str, size: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "version": version,
            "publishedAt": 100,
            "notesUrl": format!("https://github.com/ZR233/pure-lang/releases/tag/v{version}"),
            "platforms": {
                "windows-x86_64": {
                    "url": url,
                    "size": size,
                    "sha256": "ab".repeat(32),
                    "signature": signature,
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn accepts_newer_stable_release() {
        let url = "https://github.com/ZR233/pure-lang/releases/download/v1.2.0/Pure-Studio-1.2.0-windows-x86_64-setup.exe";
        let result = evaluate_manifest(
            &manifest("1.2.0", url, &format!("{url}.minisig"), 42),
            "1.1.9",
        )
        .unwrap();
        assert!(matches!(result, StudioUpdateCheck::Available(_)));
    }

    #[test]
    fn treats_same_version_and_downgrade_as_up_to_date() {
        let url = "https://github.com/ZR233/pure-lang/releases/download/v1.0.0/Pure-Studio-1.0.0-windows-x86_64-setup.exe";
        let bytes = manifest("1.0.0", url, &format!("{url}.minisig"), 42);
        assert_eq!(
            evaluate_manifest(&bytes, "1.0.0").unwrap(),
            StudioUpdateCheck::UpToDate
        );
        assert_eq!(
            evaluate_manifest(&bytes, "2.0.0").unwrap(),
            StudioUpdateCheck::UpToDate
        );
    }

    #[test]
    fn rejects_unknown_schema_prerelease_and_url_escape() {
        let url = "https://example.com/releases/download/v1.2.0/Pure-Studio-1.2.0-windows-x86_64-setup.exe";
        let bytes = manifest("1.2.0", url, &format!("{url}.minisig"), 42);
        assert!(evaluate_manifest(&bytes, "1.0.0").is_err());

        let url = "https://github.com/ZR233/pure-lang/releases/download/v1.2.0-rc.1/Pure-Studio-1.2.0-rc.1-windows-x86_64-setup.exe";
        assert!(
            evaluate_manifest(
                &manifest("1.2.0-rc.1", url, &format!("{url}.minisig"), 42),
                "1.0.0"
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_oversized_installer() {
        let url = "https://github.com/ZR233/pure-lang/releases/download/v1.2.0/Pure-Studio-1.2.0-windows-x86_64-setup.exe";
        let error = evaluate_manifest(
            &manifest(
                "1.2.0",
                url,
                &format!("{url}.minisig"),
                MAX_INSTALLER_BYTES + 1,
            ),
            "1.0.0",
        )
        .unwrap_err();
        assert_eq!(error.code(), StudioUpdateErrorCode::DownloadTooLarge);
    }

    #[test]
    fn rejects_unknown_manifest_fields() {
        let url = "https://github.com/ZR233/pure-lang/releases/download/v1.2.0/Pure-Studio-1.2.0-windows-x86_64-setup.exe";
        let mut value: serde_json::Value =
            serde_json::from_slice(&manifest("1.2.0", url, &format!("{url}.minisig"), 42)).unwrap();
        value["unexpected"] = serde_json::json!(true);
        assert!(evaluate_manifest(&serde_json::to_vec(&value).unwrap(), "1.0.0").is_err());
    }

    #[test]
    fn redirect_policy_allows_github_cdn_and_rejects_origin_escape() {
        for allowed in [
            "https://github.com/ZR233/pure-lang/releases/download/v1.2.0/setup.exe",
            "https://objects.githubusercontent.com/github-production-release-asset/file?x=1",
            "https://release-assets.githubusercontent.com/github-production-release-asset/file?x=1",
        ] {
            assert!(validate_redirect_url(&Url::parse(allowed).unwrap()).is_ok());
        }
        for denied in [
            "http://github.com/ZR233/pure-lang/releases/download/v1.2.0/setup.exe",
            "https://github.com:444/ZR233/pure-lang/releases/download/v1.2.0/setup.exe",
            "https://user@github.com/ZR233/pure-lang/releases/download/v1.2.0/setup.exe",
            "https://raw.githubusercontent.com/ZR233/pure-lang/main/setup.exe",
            "https://github.com.example.test/setup.exe",
            "https://example.test/setup.exe",
        ] {
            assert!(validate_redirect_url(&Url::parse(denied).unwrap()).is_err());
        }
    }
}
