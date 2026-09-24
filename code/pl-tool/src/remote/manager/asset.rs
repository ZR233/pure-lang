//! Remote helper 平台选择、按需资产加载与原子上传。

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use super::SshServerProfile;
use super::ssh::{posix_remote_command, run_ssh_capture, ssh_command};
use crate::remote::RemoteClientError;

const HELPER_NAME: &str = "pl-remote-helper";

/// 可嵌入 anywork 的远端 helper 目标平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemoteHelperTarget {
    Aarch64Musl,
    X8664Musl,
}

impl RemoteHelperTarget {
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

    /// 返回目标的 canonical Rust triple。
    pub const fn triple(self) -> &'static str {
        match self {
            Self::Aarch64Musl => "aarch64-unknown-linux-musl",
            Self::X8664Musl => "x86_64-unknown-linux-musl",
        }
    }
}

/// 在 SSH 架构探测后按需提供一个解压后的 remote helper。
///
/// 实现不得提前解压未请求的架构。该方法在 core 隔离的 blocking task 中调用。
pub trait RemoteHelperAssets: std::fmt::Debug + Send + Sync {
    /// 加载指定 target 的完整 helper bytes。
    ///
    /// # Errors
    ///
    /// 资产缺失、损坏或无法解压时返回 [`RemoteClientError`]。
    fn load(&self, target: RemoteHelperTarget) -> Result<Arc<[u8]>, RemoteClientError>;
}

#[derive(Debug)]
struct FileHelperAssets {
    helpers: HashMap<RemoteHelperTarget, PathBuf>,
}

impl RemoteHelperAssets for FileHelperAssets {
    fn load(&self, target: RemoteHelperTarget) -> Result<Arc<[u8]>, RemoteClientError> {
        let helper = self.helpers.get(&target).ok_or_else(|| {
            RemoteClientError::Protocol(format!(
                "helper artifact for {} is not available",
                target.triple()
            ))
        })?;
        verify_file_helper(helper)
    }
}

pub(super) fn file_helper_assets(
    aarch64_helper: Option<PathBuf>,
    x86_64_helper: Option<PathBuf>,
) -> Option<Arc<dyn RemoteHelperAssets>> {
    let mut helpers = HashMap::new();
    if let Some(path) = aarch64_helper {
        helpers.insert(RemoteHelperTarget::Aarch64Musl, path);
    }
    if let Some(path) = x86_64_helper {
        helpers.insert(RemoteHelperTarget::X8664Musl, path);
    }
    (!helpers.is_empty()).then(|| Arc::new(FileHelperAssets { helpers }) as Arc<_>)
}

fn verify_file_helper(helper: &std::path::Path) -> Result<Arc<[u8]>, RemoteClientError> {
    let checksum_path = helper.with_extension("sha256");
    let checksum = std::fs::read_to_string(&checksum_path).map_err(|error| {
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
    let bytes = std::fs::read(helper).map_err(|error| {
        RemoteClientError::Protocol(format!("failed to read helper artifact: {error}"))
    })?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(RemoteClientError::Protocol(format!(
            "helper checksum mismatch for {}",
            helper.display()
        )));
    }
    Ok(Arc::from(bytes))
}

pub(super) async fn load_helper(
    assets: Arc<dyn RemoteHelperAssets>,
    target: RemoteHelperTarget,
) -> Result<Arc<[u8]>, RemoteClientError> {
    tokio::task::spawn_blocking(move || assets.load(target))
        .await
        .map_err(|error| {
            RemoteClientError::Protocol(format!("remote helper decompression task failed: {error}"))
        })?
}

pub(super) async fn upload_helper(
    profile: &SshServerProfile,
    ssh_config: &super::super::ssh_config::SshConfigFile,
    bytes: &[u8],
) -> Result<String, RemoteClientError> {
    let digest = hex::encode(Sha256::digest(bytes));
    let version = env!("CARGO_PKG_VERSION");
    let directory = format!(
        "\"$HOME/.anywork/remote-helper/{version}/{}\"",
        &digest[..16]
    );
    let path = format!("{directory}/{HELPER_NAME}");
    let probe = format!("if test -x {path}; then printf present; fi");
    if run_ssh_capture(profile, ssh_config, &probe).await?.trim() == "present" {
        return Ok(path);
    }
    let temporary = format!("{path}.tmp");
    let script = format!(
        "umask 077; mkdir -p {directory} && cat > {temporary} && chmod 700 {temporary} && mv -f {temporary} {path}"
    );
    let mut prepared = ssh_command(profile, ssh_config).await?;
    prepared
        .command
        .arg(posix_remote_command(&script))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = super::ssh::run_bounded_ssh(
        &mut prepared.command,
        Some(bytes),
        "helper upload",
        std::time::Duration::from_secs(120),
    )
    .await?;
    if !output.status.success() {
        return Err(RemoteClientError::Protocol(format!(
            "helper upload failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(path)
}
