//! 更新状态机接收的命令载荷，统一携带乐观锁 revision 与业务数据。

use pl_protocol::StateError;

use crate::StudioUpdate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudioUpdateCommand {
    BeginCheck {
        expected_revision: u64,
        operation_id: String,
        started_at: i64,
    },
    FinishUpToDate {
        expected_revision: u64,
        checked_at: i64,
    },
    FinishAvailable {
        expected_revision: u64,
        checked_at: i64,
        update: StudioUpdate,
    },
    FailCheck {
        expected_revision: u64,
        failed_at: i64,
        error: StateError,
    },
    BeginDownload {
        expected_revision: u64,
        updated_at: i64,
        update: StudioUpdate,
        total: u64,
    },
    ReportDownload {
        expected_revision: u64,
        updated_at: i64,
        downloaded: u64,
        total: u64,
    },
    BeginVerify {
        expected_revision: u64,
        updated_at: i64,
    },
    MarkInstallerLaunched {
        expected_revision: u64,
        launched_at: i64,
    },
    FailInstall {
        expected_revision: u64,
        failed_at: i64,
        error: StateError,
    },
}

impl StudioUpdateCommand {
    pub(super) const fn expected_revision(&self) -> u64 {
        match self {
            Self::BeginCheck {
                expected_revision, ..
            }
            | Self::FinishUpToDate {
                expected_revision, ..
            }
            | Self::FinishAvailable {
                expected_revision, ..
            }
            | Self::FailCheck {
                expected_revision, ..
            }
            | Self::BeginDownload {
                expected_revision, ..
            }
            | Self::ReportDownload {
                expected_revision, ..
            }
            | Self::BeginVerify {
                expected_revision, ..
            }
            | Self::MarkInstallerLaunched {
                expected_revision, ..
            }
            | Self::FailInstall {
                expected_revision, ..
            } => *expected_revision,
        }
    }
}
