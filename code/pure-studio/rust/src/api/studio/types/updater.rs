/// Studio 稳定更新检查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeStudioUpdateCheckDto {
    UpToDate,
    Available { update: BridgeStudioUpdateDto },
}

/// 已通过 Rust 清单验证的更新，Dart 不接触 raw JSON。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeStudioUpdateDto {
    pub version: String,
    pub published_at: i64,
    pub notes_url: String,
    pub installer_url: String,
    pub installer_size: u64,
    pub installer_sha256: String,
    pub installer_signature_url: String,
}

/// Studio 安装进度事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeStudioUpdateEventDto {
    Started { total: u64 },
    Progress { downloaded: u64, total: u64 },
    Verifying,
    InstallerLaunched,
    Failed { code: String, message: String },
}
