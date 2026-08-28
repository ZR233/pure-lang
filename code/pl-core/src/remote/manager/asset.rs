//! Remote helper 平台选择、本地验签与原子上传。

use std::path::Path;
use std::process::Stdio;

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::SshServerProfile;
use super::ssh::ssh_command;
use crate::remote::RemoteClientError;

const HELPER_NAME: &str = "pl-remote-helper";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum HelperTarget {
    Aarch64Musl,
    X8664Musl,
}

impl HelperTarget {
    pub(super) fn from_uname(output: &str) -> Result<Self, RemoteClientError> {
        let mut lines = output.lines();
        let os = lines.next().unwrap_or_default().trim();
        let architecture = lines.next().unwrap_or_default().trim();
        if os != "Linux" {
            return Err(RemoteClientError::Protocol(format!(
                "unsupported remote operating system '{os}'"
            )));
        }
        match architecture {
            "aarch64" | "arm64" => Ok(Self::Aarch64Musl),
            "x86_64" | "amd64" => Ok(Self::X8664Musl),
            other => Err(RemoteClientError::Protocol(format!(
                "unsupported remote architecture '{other}'"
            ))),
        }
    }

    pub(super) fn triple(self) -> &'static str {
        match self {
            Self::Aarch64Musl => "aarch64-unknown-linux-musl",
            Self::X8664Musl => "x86_64-unknown-linux-musl",
        }
    }
}

pub(super) async fn verify_helper_asset(
    helper: &Path,
    public_key: Option<&str>,
) -> Result<(), RemoteClientError> {
    let checksum_path = helper.with_extension("sha256");
    let checksum = tokio::fs::read_to_string(&checksum_path)
        .await
        .map_err(|error| {
            RemoteClientError::Protocol(format!(
                "failed to read helper checksum {}: {error}",
                checksum_path.display()
            ))
        })?;
    let expected = checksum
        .split_whitespace()
        .next()
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| {
            RemoteClientError::Protocol(format!(
                "helper checksum {} is invalid",
                checksum_path.display()
            ))
        })?;
    let bytes = tokio::fs::read(helper).await.map_err(|error| {
        RemoteClientError::Protocol(format!("failed to read helper artifact: {error}"))
    })?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(RemoteClientError::Protocol(format!(
            "helper checksum mismatch for {}",
            helper.display()
        )));
    }
    if let Some(public_key) = public_key {
        verify_minisign(helper, public_key, &bytes).await?;
    }
    Ok(())
}

async fn verify_minisign(
    helper: &Path,
    public_key: &str,
    bytes: &[u8],
) -> Result<(), RemoteClientError> {
    let signature_path = helper.with_extension("minisig");
    let signature = tokio::fs::read_to_string(&signature_path)
        .await
        .map_err(|error| {
            RemoteClientError::Protocol(format!(
                "failed to read helper signature {}: {error}",
                signature_path.display()
            ))
        })?;
    let public_key = PublicKey::decode(public_key).map_err(|error| {
        RemoteClientError::Protocol(format!("invalid remote helper Minisign key: {error}"))
    })?;
    let signature = Signature::decode(&signature).map_err(|error| {
        RemoteClientError::Protocol(format!("invalid remote helper signature: {error}"))
    })?;
    let mut verifier = public_key.verify_stream(&signature).map_err(|error| {
        RemoteClientError::Protocol(format!(
            "failed to initialize helper signature verification: {error}"
        ))
    })?;
    verifier.update(bytes);
    verifier.finalize().map_err(|error| {
        RemoteClientError::Protocol(format!("remote helper signature rejected: {error}"))
    })
}

pub(super) async fn upload_helper(
    profile: &SshServerProfile,
    password: Option<&str>,
    helper: &Path,
) -> Result<String, RemoteClientError> {
    let bytes = tokio::fs::read(helper).await.map_err(|error| {
        RemoteClientError::Protocol(format!("failed to read helper artifact: {error}"))
    })?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let version = env!("CARGO_PKG_VERSION");
    let directory = format!("$HOME/.pure/remote-helper/{version}/{}", &digest[..16]);
    let path = format!("{directory}/{HELPER_NAME}");
    let temporary = format!("{path}.tmp");
    let script = format!(
        "umask 077; mkdir -p {directory} && cat > {temporary} && chmod 700 {temporary} && mv -f {temporary} {path}"
    );
    let mut prepared = ssh_command(profile, password)?;
    prepared
        .command
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = prepared.command.spawn().map_err(|error| {
        RemoteClientError::Protocol(format!("failed to start ssh upload: {error}"))
    })?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        RemoteClientError::Protocol("ssh upload process has no stdin".to_string())
    })?;
    stdin.write_all(&bytes).await.map_err(|error| {
        RemoteClientError::Protocol(format!("failed to upload helper: {error}"))
    })?;
    drop(stdin);
    let output = child.wait_with_output().await.map_err(|error| {
        RemoteClientError::Protocol(format!("failed to wait for helper upload: {error}"))
    })?;
    if !output.status.success() {
        return Err(RemoteClientError::Protocol(format!(
            "helper upload failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_mapping_is_exhaustive() {
        assert_eq!(
            HelperTarget::from_uname("Linux\naarch64\n").expect("aarch64"),
            HelperTarget::Aarch64Musl
        );
        assert!(HelperTarget::from_uname("Linux\narmv7\n").is_err());
        assert!(HelperTarget::from_uname("Darwin\naarch64\n").is_err());
    }

    #[tokio::test]
    async fn helper_checksum_rejects_tampering() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let helper = directory.path().join("pl-remote-helper");
        tokio::fs::write(&helper, b"helper")
            .await
            .expect("write helper");
        let digest = format!("{:x}", Sha256::digest(b"helper"));
        tokio::fs::write(
            helper.with_extension("sha256"),
            format!("{digest}  pl-remote-helper\n"),
        )
        .await
        .expect("write checksum");
        verify_helper_asset(&helper, None)
            .await
            .expect("valid checksum");

        tokio::fs::write(&helper, b"tampered")
            .await
            .expect("tamper helper");
        assert!(verify_helper_asset(&helper, None).await.is_err());
    }
}
