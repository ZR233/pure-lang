use super::asset_name;
use crate::process;
use anyhow::{Context, Result, anyhow, bail};
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const REPOSITORY_RELEASES: &str = "https://github.com/ZR233/pure-lang/releases";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateManifest {
    schema_version: u32,
    version: String,
    published_at: i64,
    notes_url: String,
    platforms: UpdatePlatforms,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdatePlatforms {
    #[serde(rename = "windows-x86_64")]
    windows_x86_64: UpdateAsset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateAsset {
    url: String,
    size: u64,
    sha256: String,
    signature: String,
}

pub(super) fn finalize(workspace_root: &Path, release_dir: &Path, version: &Version) -> Result<()> {
    ensure_staged_assets(release_dir, version)?;
    let setup_name = asset_name(version, "setup.exe");
    let portable_name = asset_name(version, "portable.zip");
    let setup_path = release_dir.join(&setup_name);
    let portable_path = release_dir.join(&portable_name);
    let setup_hash = sha256_file(&setup_path)?;
    let portable_hash = sha256_file(&portable_path)?;

    let secret_key = env::var_os("MINISIGN_SECRET_KEY_FILE")
        .map(PathBuf::from)
        .context("MINISIGN_SECRET_KEY_FILE is required for release-gui finalize")?;
    sign_file(&secret_key, &setup_path, version)?;
    sign_file(&secret_key, &portable_path, version)?;

    let manifest = manifest_for(
        version,
        published_at()?,
        &setup_name,
        fs::metadata(&setup_path)?.len(),
        setup_hash.clone(),
    );
    let manifest_path = release_dir.join("latest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    let sums = format!("{setup_hash}  {setup_name}\n{portable_hash}  {portable_name}\n");
    fs::write(release_dir.join("SHA256SUMS.txt"), sums)?;
    verify(workspace_root, release_dir, version)
}

pub(super) fn verify(workspace_root: &Path, release_dir: &Path, version: &Version) -> Result<()> {
    ensure_staged_assets(release_dir, version)?;
    let expected = expected_files(version);
    let mut actual = fs::read_dir(release_dir)
        .with_context(|| format!("failed to read {}", release_dir.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    actual.sort();
    if actual != expected {
        bail!("release file set mismatch; expected {expected:?}, got {actual:?}");
    }

    let manifest_path = release_dir.join("latest.json");
    let manifest: UpdateManifest = serde_json::from_slice(&fs::read(&manifest_path)?)
        .with_context(|| format!("invalid update manifest: {}", manifest_path.display()))?;
    verify_manifest(&manifest, release_dir, version)?;
    verify_sums(release_dir, version)?;

    let public_key = env::var_os("MINISIGN_PUBLIC_KEY_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            workspace_root
                .join("code")
                .join("pl-studio-runtime")
                .join("src")
                .join("updater")
                .join("pure-studio.pub")
        });
    for file in [
        asset_name(version, "setup.exe"),
        asset_name(version, "portable.zip"),
    ] {
        verify_signature(&public_key, &release_dir.join(file))?;
    }
    println!("Verified stable release: {}", release_dir.display());
    Ok(())
}

fn ensure_staged_assets(release_dir: &Path, version: &Version) -> Result<()> {
    for name in [
        asset_name(version, "setup.exe"),
        asset_name(version, "portable.zip"),
    ] {
        let path = release_dir.join(name);
        if !path.is_file() {
            bail!("release asset not found: {}", path.display());
        }
    }
    Ok(())
}

fn manifest_for(
    version: &Version,
    published_at: i64,
    setup_name: &str,
    size: u64,
    sha256: String,
) -> UpdateManifest {
    let tag = format!("v{version}");
    let url = format!("{REPOSITORY_RELEASES}/download/{tag}/{setup_name}");
    UpdateManifest {
        schema_version: 1,
        version: version.to_string(),
        published_at,
        notes_url: format!("{REPOSITORY_RELEASES}/tag/{tag}"),
        platforms: UpdatePlatforms {
            windows_x86_64: UpdateAsset {
                signature: format!("{url}.minisig"),
                url,
                size,
                sha256,
            },
        },
    }
}

fn verify_manifest(manifest: &UpdateManifest, release_dir: &Path, version: &Version) -> Result<()> {
    if manifest.schema_version != 1 || manifest.version != version.to_string() {
        bail!("manifest schema or version mismatch");
    }
    let expected_name = asset_name(version, "setup.exe");
    let expected = manifest_for(
        version,
        manifest.published_at,
        &expected_name,
        manifest.platforms.windows_x86_64.size,
        manifest.platforms.windows_x86_64.sha256.clone(),
    );
    if manifest != &expected {
        bail!("manifest URLs or asset metadata do not match the fixed release contract");
    }
    let path = release_dir.join(expected_name);
    if fs::metadata(&path)?.len() != manifest.platforms.windows_x86_64.size {
        bail!("manifest size does not match installer bytes");
    }
    if sha256_file(&path)? != manifest.platforms.windows_x86_64.sha256 {
        bail!("manifest SHA-256 does not match installer bytes");
    }
    Ok(())
}

fn verify_sums(release_dir: &Path, version: &Version) -> Result<()> {
    let expected = [
        asset_name(version, "setup.exe"),
        asset_name(version, "portable.zip"),
    ]
    .into_iter()
    .map(|name| {
        Ok(format!(
            "{}  {name}",
            sha256_file(&release_dir.join(&name))?
        ))
    })
    .collect::<Result<Vec<_>>>()?
    .join("\n")
        + "\n";
    let actual = fs::read_to_string(release_dir.join("SHA256SUMS.txt"))?;
    if actual.replace("\r\n", "\n") != expected {
        bail!("SHA256SUMS.txt does not match release assets");
    }
    Ok(())
}

fn expected_files(version: &Version) -> Vec<String> {
    let setup = asset_name(version, "setup.exe");
    let portable = asset_name(version, "portable.zip");
    let mut files = vec![
        "SHA256SUMS.txt".to_string(),
        format!("{portable}.minisig"),
        portable,
        format!("{setup}.minisig"),
        setup,
        "latest.json".to_string(),
    ];
    files.sort();
    files
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?,
    );
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sign_file(secret_key: &Path, message: &Path, version: &Version) -> Result<()> {
    if !secret_key.is_file() {
        bail!("Minisign secret key not found: {}", secret_key.display());
    }
    let signature = PathBuf::from(format!("{}.minisig", message.display()));
    let args = [
        OsString::from("-S"),
        OsString::from("-s"),
        secret_key.as_os_str().to_owned(),
        OsString::from("-m"),
        message.as_os_str().to_owned(),
        OsString::from("-x"),
        signature.as_os_str().to_owned(),
        OsString::from("-t"),
        OsString::from(format!("Pure Studio {version}")),
    ];
    let mut command = Command::new("minisign");
    command.args(args);
    process::run_checked(
        &mut command,
        &format!("minisign sign {}", message.display()),
    )
}

fn verify_signature(public_key: &Path, message: &Path) -> Result<()> {
    if !public_key.is_file() {
        bail!("Minisign public key not found: {}", public_key.display());
    }
    let signature = PathBuf::from(format!("{}.minisig", message.display()));
    let public_key =
        fs::read_to_string(public_key).context("failed to read Minisign public key")?;
    let signature = fs::read_to_string(signature).context("failed to read Minisign signature")?;
    let message = File::open(message).context("failed to open signed release asset")?;
    verify_signature_reader(&public_key, &signature, message)
}

fn verify_signature_reader(
    public_key: &str,
    signature: &str,
    mut message: impl Read,
) -> Result<()> {
    let public_key =
        PublicKey::decode(public_key).map_err(|error| anyhow!("invalid Minisign key: {error}"))?;
    let signature = Signature::decode(signature)
        .map_err(|error| anyhow!("invalid Minisign signature: {error}"))?;
    let mut verifier = public_key
        .verify_stream(&signature)
        .map_err(|error| anyhow!("failed to initialize Minisign verification: {error}"))?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = message.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        verifier.update(&buffer[..count]);
    }
    verifier
        .finalize()
        .map_err(|error| anyhow!("Minisign verification failed: {error}"))
}

fn published_at() -> Result<i64> {
    if let Ok(value) = env::var("SOURCE_DATE_EPOCH") {
        return value
            .parse::<i64>()
            .context("SOURCE_DATE_EPOCH must be Unix seconds");
    }
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs();
    i64::try_from(seconds).context("current Unix timestamp does not fit i64")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::io::Cursor;

    const TEST_PUBLIC_KEY: &str = "untrusted comment: minisign public key 2\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

    #[test]
    fn manifest_uses_typed_camel_case_schema() -> Result<()> {
        let version = Version::parse("1.2.3")?;
        let manifest = manifest_for(
            &version,
            123,
            "Pure-Studio-1.2.3-windows-x86_64-setup.exe",
            456,
            "abcd".to_string(),
        );
        let json = serde_json::to_value(manifest)?;
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["publishedAt"], 123);
        assert_eq!(json["platforms"]["windows-x86_64"]["size"], 456);
        Ok(())
    }

    #[test]
    fn signature_verification_rejects_tampered_release_bytes() -> Result<()> {
        verify_signature_reader(TEST_PUBLIC_KEY, TEST_SIGNATURE, Cursor::new(b"test"))?;
        assert!(
            verify_signature_reader(TEST_PUBLIC_KEY, TEST_SIGNATURE, Cursor::new(b"tampered"))
                .is_err()
        );
        Ok(())
    }
}
