//! Concrete FRB updater state union.

use serde::{Deserialize, Serialize};

use super::runtime::BridgeStateError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeUpdaterStateSnapshot {
    Disabled(BridgeDisabledUpdaterState),
    Idle(BridgeIdleUpdaterState),
    Checking(BridgeCheckingUpdaterState),
    UpToDate(BridgeUpToDateUpdaterState),
    Available(BridgeAvailableUpdaterState),
    Downloading(BridgeDownloadingUpdaterState),
    Verifying(BridgeVerifyingUpdaterState),
    InstallerLaunched(BridgeInstallerLaunchedUpdaterState),
    CheckFailed(BridgeCheckFailedUpdaterState),
    InstallFailed(BridgeInstallFailedUpdaterState),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeDisabledUpdaterState {
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeIdleUpdaterState {
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCheckingUpdaterState {
    pub revision: u64,
    pub operation_id: String,
    pub started_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeUpToDateUpdaterState {
    pub revision: u64,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAvailableUpdaterState {
    pub revision: u64,
    pub checked_at: i64,
    pub update: BridgeVerifiedUpdateSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeDownloadingUpdaterState {
    pub revision: u64,
    pub updated_at: i64,
    pub update: BridgeVerifiedUpdateSummary,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeVerifyingUpdaterState {
    pub revision: u64,
    pub updated_at: i64,
    pub update: BridgeVerifiedUpdateSummary,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInstallerLaunchedUpdaterState {
    pub revision: u64,
    pub launched_at: i64,
    pub update: BridgeVerifiedUpdateSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCheckFailedUpdaterState {
    pub revision: u64,
    pub failed_at: i64,
    pub error: BridgeStateError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInstallFailedUpdaterState {
    pub revision: u64,
    pub failed_at: i64,
    pub update: BridgeVerifiedUpdateSummary,
    pub error: BridgeStateError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeVerifiedUpdateSummary {
    pub version: String,
    pub published_at: i64,
    pub notes_url: String,
}
