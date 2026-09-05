//! 更新状态对外快照：状态种类枚举、tagged 载荷表示与只读访问器。

use serde::{Deserialize, Serialize};

use crate::StudioUpdate;

use super::{
    AvailableUpdateState, CheckFailedUpdateState, CheckingUpdateState, DisabledUpdateState,
    DownloadingUpdateState, IdleUpdateState, InstallFailedUpdateState,
    InstallerLaunchedUpdateState, UpToDateUpdateState, VerifyingUpdateState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StudioUpdateStateKind {
    Disabled,
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Verifying,
    InstallerLaunched,
    CheckFailed,
    InstallFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioUpdateStateSnapshot {
    Disabled(DisabledUpdateState),
    Idle(IdleUpdateState),
    Checking(CheckingUpdateState),
    UpToDate(UpToDateUpdateState),
    Available(AvailableUpdateState),
    Downloading(DownloadingUpdateState),
    Verifying(VerifyingUpdateState),
    InstallerLaunched(InstallerLaunchedUpdateState),
    CheckFailed(CheckFailedUpdateState),
    InstallFailed(InstallFailedUpdateState),
}

impl StudioUpdateStateSnapshot {
    pub fn idle(updated_at: i64) -> Self {
        Self::Idle(IdleUpdateState::new(updated_at))
    }

    pub const fn kind(&self) -> StudioUpdateStateKind {
        match self {
            Self::Disabled(_) => StudioUpdateStateKind::Disabled,
            Self::Idle(_) => StudioUpdateStateKind::Idle,
            Self::Checking(_) => StudioUpdateStateKind::Checking,
            Self::UpToDate(_) => StudioUpdateStateKind::UpToDate,
            Self::Available(_) => StudioUpdateStateKind::Available,
            Self::Downloading(_) => StudioUpdateStateKind::Downloading,
            Self::Verifying(_) => StudioUpdateStateKind::Verifying,
            Self::InstallerLaunched(_) => StudioUpdateStateKind::InstallerLaunched,
            Self::CheckFailed(_) => StudioUpdateStateKind::CheckFailed,
            Self::InstallFailed(_) => StudioUpdateStateKind::InstallFailed,
        }
    }

    pub const fn revision(&self) -> u64 {
        match self {
            Self::Disabled(value) => value.revision,
            Self::Idle(value) => value.revision,
            Self::Checking(value) => value.revision,
            Self::UpToDate(value) => value.revision,
            Self::Available(value) => value.revision,
            Self::Downloading(value) => value.revision,
            Self::Verifying(value) => value.revision,
            Self::InstallerLaunched(value) => value.revision,
            Self::CheckFailed(value) => value.revision,
            Self::InstallFailed(value) => value.revision,
        }
    }

    pub const fn updated_at(&self) -> i64 {
        match self {
            Self::Disabled(value) => value.updated_at,
            Self::Idle(value) => value.updated_at,
            Self::Checking(value) => value.started_at,
            Self::UpToDate(value) => value.checked_at,
            Self::Available(value) => value.checked_at,
            Self::Downloading(value) => value.updated_at,
            Self::Verifying(value) => value.updated_at,
            Self::InstallerLaunched(value) => value.launched_at,
            Self::CheckFailed(value) => value.failed_at,
            Self::InstallFailed(value) => value.failed_at,
        }
    }

    pub fn update(&self) -> Option<&StudioUpdate> {
        match self {
            Self::Available(value) => Some(&value.update),
            Self::Downloading(value) => Some(&value.update),
            Self::Verifying(value) => Some(&value.update),
            Self::InstallerLaunched(value) => Some(&value.update),
            Self::InstallFailed(value) => Some(&value.update),
            Self::Disabled(_)
            | Self::Idle(_)
            | Self::Checking(_)
            | Self::UpToDate(_)
            | Self::CheckFailed(_) => None,
        }
    }
}
