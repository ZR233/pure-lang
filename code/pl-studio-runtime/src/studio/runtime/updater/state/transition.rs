//! 更新状态机的唯一迁移入口 decide：校验命令 revision 与状态合法性，产出决策或迁移错误。

use thiserror::Error;

use super::{
    AvailableUpdateState, CheckFailedUpdateState, CheckingUpdateState, DownloadingUpdateState,
    InstallFailedUpdateState, InstallerLaunchedUpdateState, StudioUpdateCommand,
    StudioUpdateStateKind, StudioUpdateStateSnapshot, UpToDateUpdateState, VerifyingUpdateState,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StudioUpdateTransitionError {
    #[error("updater revision is stale: expected {expected}, actual {actual}, command {command:?}")]
    StaleRevision {
        expected: u64,
        actual: u64,
        command: Box<StudioUpdateCommand>,
    },
    #[error("updater in {current:?} rejects command {command:?}")]
    IllegalTransition {
        current: StudioUpdateStateKind,
        command: Box<StudioUpdateCommand>,
    },
    #[error("updater command {command:?} does not match available update {current_version}")]
    CorrelationMismatch {
        current_version: String,
        command: Box<StudioUpdateCommand>,
    },
    #[error("updater in {current:?} rejects invalid payload for command {command:?}: {detail}")]
    InvalidPayload {
        current: StudioUpdateStateKind,
        command: Box<StudioUpdateCommand>,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioUpdateTransitionDecision {
    pub next_state: StudioUpdateStateSnapshot,
    pub changed: bool,
}

impl StudioUpdateStateSnapshot {
    pub fn decide(
        &self,
        command: StudioUpdateCommand,
    ) -> Result<StudioUpdateTransitionDecision, StudioUpdateTransitionError> {
        if command.expected_revision() != self.revision() {
            return Err(StudioUpdateTransitionError::StaleRevision {
                expected: command.expected_revision(),
                actual: self.revision(),
                command: Box::new(command),
            });
        }
        let revision = self.revision().saturating_add(1);
        let next = match (self, &command) {
            (
                Self::Idle(_)
                | Self::UpToDate(_)
                | Self::Available(_)
                | Self::CheckFailed(_)
                | Self::InstallFailed(_),
                StudioUpdateCommand::BeginCheck {
                    operation_id,
                    started_at,
                    ..
                },
            ) => Self::Checking(CheckingUpdateState {
                revision,
                operation_id: operation_id.clone(),
                started_at: *started_at,
            }),
            (Self::Checking(_), StudioUpdateCommand::FinishUpToDate { checked_at, .. }) => {
                Self::UpToDate(UpToDateUpdateState {
                    revision,
                    checked_at: *checked_at,
                })
            }
            (
                Self::Checking(_),
                StudioUpdateCommand::FinishAvailable {
                    checked_at, update, ..
                },
            ) => Self::Available(AvailableUpdateState {
                revision,
                checked_at: *checked_at,
                update: update.clone(),
            }),
            (
                Self::Checking(_),
                StudioUpdateCommand::FailCheck {
                    failed_at, error, ..
                },
            ) => Self::CheckFailed(CheckFailedUpdateState {
                revision,
                failed_at: *failed_at,
                error: error.clone(),
            }),
            (Self::Available(current), StudioUpdateCommand::BeginDownload { update, .. })
                if current.update != *update =>
            {
                return Err(StudioUpdateTransitionError::CorrelationMismatch {
                    current_version: current.update.version.clone(),
                    command: Box::new(command),
                });
            }
            (Self::Available(_), StudioUpdateCommand::BeginDownload { total: 0, .. }) => {
                return Err(StudioUpdateTransitionError::InvalidPayload {
                    current: self.kind(),
                    command: Box::new(command),
                    detail: "download total must be positive".to_string(),
                });
            }
            (
                Self::Available(_),
                StudioUpdateCommand::BeginDownload {
                    updated_at,
                    update,
                    total,
                    ..
                },
            ) => Self::Downloading(DownloadingUpdateState {
                revision,
                updated_at: *updated_at,
                update: update.clone(),
                downloaded: 0,
                total: *total,
            }),
            (
                Self::Downloading(current),
                StudioUpdateCommand::ReportDownload {
                    updated_at,
                    downloaded,
                    total,
                    ..
                },
            ) if *total != current.total || *downloaded > *total => {
                return Err(StudioUpdateTransitionError::InvalidPayload {
                    current: self.kind(),
                    command: Box::new(command.clone()),
                    detail: format!(
                        "download progress {downloaded}/{total} does not match active total {}",
                        current.total
                    ),
                });
            }
            (
                Self::Downloading(current),
                StudioUpdateCommand::ReportDownload {
                    updated_at,
                    downloaded,
                    total,
                    ..
                },
            ) => Self::Downloading(DownloadingUpdateState {
                revision,
                updated_at: *updated_at,
                update: current.update.clone(),
                downloaded: *downloaded,
                total: *total,
            }),
            (Self::Downloading(current), StudioUpdateCommand::BeginVerify { updated_at, .. }) => {
                Self::Verifying(VerifyingUpdateState {
                    revision,
                    updated_at: *updated_at,
                    update: current.update.clone(),
                    downloaded: current.downloaded,
                    total: current.total,
                })
            }
            (
                Self::Verifying(current),
                StudioUpdateCommand::MarkInstallerLaunched { launched_at, .. },
            ) => Self::InstallerLaunched(InstallerLaunchedUpdateState {
                revision,
                launched_at: *launched_at,
                update: current.update.clone(),
            }),
            (
                Self::Downloading(current),
                StudioUpdateCommand::FailInstall {
                    failed_at, error, ..
                },
            ) => Self::InstallFailed(InstallFailedUpdateState {
                revision,
                failed_at: *failed_at,
                update: current.update.clone(),
                error: error.clone(),
            }),
            (
                Self::Available(current),
                StudioUpdateCommand::FailInstall {
                    failed_at, error, ..
                },
            ) => Self::InstallFailed(InstallFailedUpdateState {
                revision,
                failed_at: *failed_at,
                update: current.update.clone(),
                error: error.clone(),
            }),
            (
                Self::Verifying(current),
                StudioUpdateCommand::FailInstall {
                    failed_at, error, ..
                },
            ) => Self::InstallFailed(InstallFailedUpdateState {
                revision,
                failed_at: *failed_at,
                update: current.update.clone(),
                error: error.clone(),
            }),
            _ => {
                return Err(StudioUpdateTransitionError::IllegalTransition {
                    current: self.kind(),
                    command: Box::new(command),
                });
            }
        };
        Ok(StudioUpdateTransitionDecision {
            changed: next != *self,
            next_state: next,
        })
    }
}
