use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use minisign_verify::{PublicKey, Signature};
use reqwest::blocking::Client;
use semver::Version;
use sha2::{Digest, Sha256};

use super::{HELPER_FILE_NAME, SUPPORTED_TARGETS, release_asset_name};

const RELEASE_ASSET_DIR_ENV: &str = "PURE_REMOTE_HELPER_RELEASE_DIR";
const RELEASE_DOWNLOAD_ROOT: &str = "https://github.com/ZR233/pure-lang/releases/download";
const MAX_HELPER_ASSET_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
struct HelperAsset {
    target: &'static str,
    binary: PathBuf,
    signature: PathBuf,
    digest: String,
}

pub(super) fn install_release_bundle(
    workspace_root: &Path,
    version: &Version,
    bundle_root: &Path,
) -> Result<()> {
    let assets = resolve_release_assets(workspace_root, version)?;
    for asset in assets {
        let target_dir = bundle_root.join("remote-helper").join(asset.target);
        fs::create_dir_all(&target_dir)
            .with_context(|| format!("failed to create {}", target_dir.display()))?;
        let binary = target_dir.join(HELPER_FILE_NAME);
        fs::copy(&asset.binary, &binary).with_context(|| {
            format!(
                "failed to copy signed helper from {} to {}",
                asset.binary.display(),
                binary.display()
            )
        })?;
        fs::write(
            binary.with_extension("sha256"),
            format!("{}  {HELPER_FILE_NAME}\n", asset.digest),
        )?;
        fs::copy(&asset.signature, binary.with_extension("minisig"))?;
    }
    Ok(())
}

pub(super) fn stage_release_assets(
    workspace_root: &Path,
    version: &Version,
    release_dir: &Path,
) -> Result<()> {
    let assets = resolve_release_assets(workspace_root, version)?;
    for asset in assets {
        let name = release_asset_name(version, asset.target);
        let binary = release_dir.join(&name);
        fs::copy(&asset.binary, &binary).with_context(|| {
            format!(
                "failed to stage helper from {} to {}",
                asset.binary.display(),
                binary.display()
            )
        })?;
        fs::write(
            release_dir.join(format!("{name}.sha256")),
            format!("{}  {name}\n", asset.digest),
        )?;
        fs::copy(
            &asset.signature,
            release_dir.join(format!("{name}.minisig")),
        )?;
    }
    Ok(())
}

fn resolve_release_assets(workspace_root: &Path, version: &Version) -> Result<Vec<HelperAsset>> {
    let source = match std::env::var_os(RELEASE_ASSET_DIR_ENV).filter(|value| !value.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => download_release_assets(workspace_root, version)?,
    };
    SUPPORTED_TARGETS
        .into_iter()
        .map(|target| load_asset(workspace_root, &source, version, target))
        .collect()
}

fn load_asset(
    workspace_root: &Path,
    source: &Path,
    version: &Version,
    target: &'static str,
) -> Result<HelperAsset> {
    let release_name = release_asset_name(version, target);
    let release_binary = source.join(&release_name);
    let (binary, checksum, signature) = if release_binary.is_file() {
        (
            release_binary,
            source.join(format!("{release_name}.sha256")),
            source.join(format!("{release_name}.minisig")),
        )
    } else {
        let binary = source.join(target).join(HELPER_FILE_NAME);
        let checksum = binary.with_extension("sha256");
        let signature = binary.with_extension("minisig");
        (binary, checksum, signature)
    };
    for path in [&binary, &checksum, &signature] {
        ensure!(
            path.is_file(),
            "signed remote helper asset is missing: {}",
            path.display()
        );
    }
    let digest = verify_checksum(&binary, &checksum)?;
    verify_signature(
        &workspace_root.join("code/pl-studio-runtime/src/updater/pure-studio.pub"),
        &binary,
        &signature,
    )?;
    Ok(HelperAsset {
        target,
        binary,
        signature,
        digest,
    })
}

fn download_release_assets(workspace_root: &Path, version: &Version) -> Result<PathBuf> {
    let cache_parent = workspace_root.join("target/xtask-remote-helper/releases");
    let cache = cache_parent.join(version.to_string());
    if cache.is_dir()
        && SUPPORTED_TARGETS.into_iter().all(|target| {
            let name = release_asset_name(version, target);
            [
                cache.join(&name),
                cache.join(format!("{name}.sha256")),
                cache.join(format!("{name}.minisig")),
            ]
            .into_iter()
            .all(|path| path.is_file())
        })
    {
        return Ok(cache);
    }

    fs::create_dir_all(&cache_parent)
        .with_context(|| format!("failed to create {}", cache_parent.display()))?;
    let temporary = tempfile::tempdir_in(&cache_parent)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(120))
        .user_agent("pure-studio-xtask")
        .build()
        .context("failed to create GitHub Release download client")?;
    for target in SUPPORTED_TARGETS {
        let name = release_asset_name(version, target);
        for suffix in ["", ".sha256", ".minisig"] {
            let file_name = format!("{name}{suffix}");
            let url = format!("{RELEASE_DOWNLOAD_ROOT}/v{version}/{file_name}");
            download_file(&client, &url, &temporary.path().join(file_name))?;
        }
    }
    for target in SUPPORTED_TARGETS {
        load_asset(workspace_root, temporary.path(), version, target)?;
    }
    if cache.exists() {
        fs::remove_dir_all(&cache)
            .with_context(|| format!("failed to replace stale helper cache {}", cache.display()))?;
    }
    fs::rename(temporary.keep(), &cache)
        .with_context(|| format!("failed to commit helper cache {}", cache.display()))?;
    Ok(cache)
}

fn download_file(client: &Client, url: &str, destination: &Path) -> Result<()> {
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("GitHub Release asset is unavailable: {url}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_HELPER_ASSET_BYTES)
    {
        bail!("GitHub Release asset exceeds 64 MiB: {url}");
    }
    let mut output = File::create(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;
    let copied = std::io::copy(&mut response.take(MAX_HELPER_ASSET_BYTES + 1), &mut output)?;
    ensure!(
        copied <= MAX_HELPER_ASSET_BYTES,
        "GitHub Release asset exceeds 64 MiB: {url}"
    );
    output.flush()?;
    Ok(())
}

fn verify_checksum(binary: &Path, checksum: &Path) -> Result<String> {
    let checksum = fs::read_to_string(checksum)
        .with_context(|| format!("failed to read {}", checksum.display()))?;
    let mut fields = checksum.split_whitespace();
    let expected = fields.next().context("helper checksum is empty")?;
    let expected_name = fields.next().context("helper checksum has no file name")?;
    ensure!(
        fields.next().is_none(),
        "helper checksum has unexpected fields"
    );
    ensure!(
        expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "helper checksum is not a SHA-256 digest"
    );
    ensure!(
        binary.file_name().is_some_and(|name| name == expected_name),
        "helper checksum file name does not match {}",
        binary.display()
    );
    let actual = format!("{:x}", Sha256::digest(fs::read(binary)?));
    ensure!(
        actual == expected.to_ascii_lowercase(),
        "helper SHA-256 mismatch"
    );
    Ok(actual)
}

fn verify_signature(public_key: &Path, binary: &Path, signature: &Path) -> Result<()> {
    let public_key = fs::read_to_string(public_key)
        .with_context(|| format!("failed to read {}", public_key.display()))?;
    let signature = fs::read_to_string(signature)
        .with_context(|| format!("failed to read {}", signature.display()))?;
    let public_key =
        PublicKey::decode(&public_key).map_err(|error| anyhow!("invalid Minisign key: {error}"))?;
    let signature = Signature::decode(&signature)
        .map_err(|error| anyhow!("invalid Minisign signature: {error}"))?;
    let mut verifier = public_key
        .verify_stream(&signature)
        .map_err(|error| anyhow!("failed to initialize Minisign verification: {error}"))?;
    let mut input = File::open(binary)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        verifier.update(&buffer[..count]);
    }
    verifier
        .finalize()
        .map_err(|error| anyhow!("remote helper Minisign verification failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PUBLIC_KEY: &str = "untrusted comment: minisign public key 2\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

    #[test]
    fn release_asset_names_are_versioned_and_targeted() {
        let version = Version::parse("2.1.0").expect("version");
        assert_eq!(
            release_asset_name(&version, "aarch64-unknown-linux-musl"),
            "Pure-Remote-Helper-2.1.0-aarch64-unknown-linux-musl"
        );
    }

    #[test]
    fn release_layout_requires_matching_checksum_and_signature() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        let public_key = workspace
            .path()
            .join("code/pl-studio-runtime/src/updater/pure-studio.pub");
        fs::create_dir_all(public_key.parent().context("public key parent")?)?;
        fs::write(public_key, TEST_PUBLIC_KEY)?;
        let source = tempfile::tempdir()?;
        let version = Version::parse("2.1.0")?;
        let name = release_asset_name(&version, "aarch64-unknown-linux-musl");
        fs::write(source.path().join(&name), b"test")?;
        fs::write(
            source.path().join(format!("{name}.sha256")),
            format!("{:x}  {name}\n", Sha256::digest(b"test")),
        )?;
        fs::write(
            source.path().join(format!("{name}.minisig")),
            TEST_SIGNATURE,
        )?;

        let asset = load_asset(
            workspace.path(),
            source.path(),
            &version,
            "aarch64-unknown-linux-musl",
        )?;
        assert_eq!(asset.digest, format!("{:x}", Sha256::digest(b"test")));
        Ok(())
    }
}
