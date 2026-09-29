//! Flutter bundle 资源 staging 前的 helper 资产准备、校验与压缩。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

use super::bundle_staging_dir;
use super::{HELPER_FILE_NAME, SUPPORTED_TARGETS, local_helper_path};

const PREBUILT_DIR_ENV: &str = "PURE_REMOTE_HELPER_PREBUILT_DIR";
const BUNDLED_ARCHIVE_FILE: &str = "pl-remote-helper.zst";
const BUNDLED_METADATA_FILE: &str = "pl-remote-helper.metadata.json";
const ZSTD_COMPRESSION_LEVEL: i32 = 12;

pub(super) fn prepare(workspace_root: &Path) -> Result<PathBuf> {
    match std::env::var_os(PREBUILT_DIR_ENV).filter(|value| !value.is_empty()) {
        Some(source) => install_prebuilt(workspace_root, &PathBuf::from(source))?,
        None => super::build::build_targets(workspace_root, &SUPPORTED_TARGETS)?,
    }
    for target in SUPPORTED_TARGETS {
        let binary = local_helper_path(workspace_root, target);
        verify_local_asset(&binary)?;
        #[cfg(target_os = "linux")]
        if target.starts_with(std::env::consts::ARCH) {
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(
                pl_remote_helper::client::ManagedWorker::probe(&binary)
            ).with_context(|| format!("helper preflight failed; rebuild with cargo xtask build-remote-helper --target {target}"))?;
        }
        stage_bundled_resource(workspace_root, target)?;
    }
    Ok(bundle_staging_dir(workspace_root))
}

fn install_prebuilt(workspace_root: &Path, source: &Path) -> Result<()> {
    for target in SUPPORTED_TARGETS {
        let source_binary = source.join(target).join(HELPER_FILE_NAME);
        verify_local_asset(&source_binary)?;
        let destination = local_helper_path(workspace_root, target);
        if source_binary == destination {
            continue;
        }
        let destination_dir = destination
            .parent()
            .context("remote helper destination has no parent")?;
        fs::create_dir_all(destination_dir)
            .with_context(|| format!("failed to create {}", destination_dir.display()))?;
        fs::copy(&source_binary, &destination).with_context(|| {
            format!(
                "failed to copy prebuilt helper from {} to {}",
                source_binary.display(),
                destination.display()
            )
        })?;
        fs::copy(
            source_binary.with_extension("metadata.json"),
            destination.with_extension("metadata.json"),
        )?;
        fs::copy(
            source_binary.with_extension("sha256"),
            destination.with_extension("sha256"),
        )?;
    }
    Ok(())
}

/// Compresses one verified helper into the bundle staging tree.
///
/// Files are only rewritten when their bytes change, so unchanged resources keep
/// their timestamps across repeated GUI builds.
fn stage_bundled_resource(workspace_root: &Path, target: &str) -> Result<()> {
    let binary = local_helper_path(workspace_root, target);
    let destination_dir = bundle_staging_dir(workspace_root).join(target);
    fs::create_dir_all(&destination_dir).with_context(|| {
        format!(
            "failed to create remote helper bundle directory: {}",
            destination_dir.display()
        )
    })?;
    let executable = fs::read(&binary)
        .with_context(|| format!("failed to read helper artifact: {}", binary.display()))?;
    let archive_path = destination_dir.join(BUNDLED_ARCHIVE_FILE);
    let archive = zstd::bulk::compress(&executable, ZSTD_COMPRESSION_LEVEL).with_context(|| {
        format!(
            "failed to compress remote helper {} into {}",
            binary.display(),
            archive_path.display()
        )
    })?;
    write_if_changed(&archive_path, &archive)?;
    let metadata_path = destination_dir.join(BUNDLED_METADATA_FILE);
    let metadata = fs::read(binary.with_extension("metadata.json")).with_context(|| {
        format!(
            "failed to read helper build metadata beside {}",
            binary.display()
        )
    })?;
    write_if_changed(&metadata_path, &metadata)?;
    // Round-trip the staged archive so a truncated staging file fails here, not at runtime.
    let staged_archive = fs::read(&archive_path).with_context(|| {
        format!(
            "failed to read staged remote helper: {}",
            archive_path.display()
        )
    })?;
    let restored =
        zstd::bulk::decompress(&staged_archive, executable.len()).with_context(|| {
            format!(
                "failed to decompress staged remote helper: {}",
                archive_path.display()
            )
        })?;
    ensure!(
        restored == executable,
        "staged remote helper {} does not round-trip to {}",
        archive_path.display(),
        binary.display()
    );
    println!("remote helper resource: {}", archive_path.display());
    Ok(())
}

fn write_if_changed(path: &Path, contents: &[u8]) -> Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))
}

fn verify_local_asset(binary: &Path) -> Result<()> {
    ensure!(
        binary.is_file(),
        "remote helper artifact is missing: {}",
        binary.display()
    );
    let checksum_path = binary.with_extension("sha256");
    let checksum = fs::read_to_string(&checksum_path)
        .with_context(|| format!("failed to read {}", checksum_path.display()))?;
    let expected = checksum
        .split_whitespace()
        .next()
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .context("remote helper checksum is invalid")?;
    let actual = hex::encode(Sha256::digest(fs::read(binary)?));
    ensure!(
        actual.eq_ignore_ascii_case(expected),
        "remote helper SHA-256 mismatch: {}",
        binary.display()
    );
    let metadata: super::HelperMetadata =
        serde_json::from_slice(&fs::read(binary.with_extension("metadata.json")).context(
            "helper build metadata missing; run cargo xtask build-remote-helper --all-targets",
        )?)?;
    ensure!(
        binary
            .parent()
            .and_then(Path::file_name)
            .and_then(|n| n.to_str())
            == Some(metadata.target.as_str())
            && metadata.worker_protocol_version
                == pl_protocol::process_worker::PROCESS_WORKER_PROTOCOL_VERSION
            && metadata.sha256 == actual,
        "helper build metadata mismatch; run cargo xtask build-remote-helper --all-targets"
    );
    Ok(())
}
