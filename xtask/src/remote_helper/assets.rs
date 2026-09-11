//! GUI bridge embedding 前的 helper 资产准备与校验。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

use super::{HELPER_FILE_NAME, SUPPORTED_TARGETS, local_helper_path};

const PREBUILT_DIR_ENV: &str = "PURE_REMOTE_HELPER_PREBUILT_DIR";

pub(super) fn prepare(workspace_root: &Path) -> Result<()> {
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
    }
    Ok(())
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
    let actual = format!("{:x}", Sha256::digest(fs::read(binary)?));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_validation_rejects_tampered_prebuilt_helper() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let target_dir = directory.path().join(super::super::X86_64_TARGET);
        fs::create_dir(&target_dir)?;
        let helper = target_dir.join(HELPER_FILE_NAME);
        fs::write(&helper, b"helper")?;
        fs::write(
            helper.with_extension("sha256"),
            format!("{:x}  {HELPER_FILE_NAME}\n", Sha256::digest(b"helper")),
        )?;
        assert!(
            verify_local_asset(&helper).is_err(),
            "checksum alone cannot admit an old helper"
        );
        fs::write(
            helper.with_extension("metadata.json"),
            serde_json::to_vec(&super::super::HelperMetadata {
                target: super::super::X86_64_TARGET.into(),
                worker_protocol_version:
                    pl_protocol::process_worker::PROCESS_WORKER_PROTOCOL_VERSION,
                sha256: format!("{:x}", Sha256::digest(b"helper")),
            })?,
        )?;
        verify_local_asset(&helper)?;
        fs::write(&helper, b"tampered")?;
        assert!(verify_local_asset(&helper).is_err());
        Ok(())
    }
}
