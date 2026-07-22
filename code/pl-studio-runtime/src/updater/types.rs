use serde::{Deserialize, Serialize};

/// 已通过稳定清单约束验证、可交给安装流程的更新。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioUpdate {
    pub version: String,
    pub published_at: i64,
    pub notes_url: String,
    pub installer: StudioUpdateAsset,
}

/// 单个平台安装资产的可信元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioUpdateAsset {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub signature: String,
}

/// 稳定更新检查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudioUpdateCheck {
    UpToDate,
    Available(StudioUpdate),
}

/// 安装期间发送给 UI 的 typed 进度事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudioUpdateEvent {
    Started { total: u64 },
    Progress { downloaded: u64, total: u64 },
    Verifying,
    InstallerLaunched,
    Failed { code: String, message: String },
}
