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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StudioUpdate, StudioUpdateAsset};

    fn update(version: &str) -> StudioUpdate {
        StudioUpdate {
            version: version.to_string(),
            published_at: 10,
            notes_url: "https://example.invalid/notes".to_string(),
            installer: StudioUpdateAsset {
                url: "https://example.invalid/installer.exe".to_string(),
                size: 100,
                sha256: "sha256".to_string(),
                signature: "signature".to_string(),
            },
        }
    }

    fn next(
        state: &StudioUpdateStateSnapshot,
        command: StudioUpdateCommand,
    ) -> StudioUpdateStateSnapshot {
        state.decide(command).unwrap().next_state
    }

    #[test]
    fn complete_update_path_uses_exact_state_payloads() {
        let idle = StudioUpdateStateSnapshot::idle(1);
        let checking = next(
            &idle,
            StudioUpdateCommand::BeginCheck {
                expected_revision: 0,
                operation_id: "check-1".to_string(),
                started_at: 2,
            },
        );
        let available = next(
            &checking,
            StudioUpdateCommand::FinishAvailable {
                expected_revision: 1,
                checked_at: 3,
                update: update("2.0.0"),
            },
        );
        let downloading = next(
            &available,
            StudioUpdateCommand::BeginDownload {
                expected_revision: 2,
                updated_at: 4,
                update: update("2.0.0"),
                total: 100,
            },
        );
        let downloading = next(
            &downloading,
            StudioUpdateCommand::ReportDownload {
                expected_revision: 3,
                updated_at: 5,
                downloaded: 100,
                total: 100,
            },
        );
        let verifying = next(
            &downloading,
            StudioUpdateCommand::BeginVerify {
                expected_revision: 4,
                updated_at: 6,
            },
        );
        let launched = next(
            &verifying,
            StudioUpdateCommand::MarkInstallerLaunched {
                expected_revision: 5,
                launched_at: 7,
            },
        );

        assert_eq!(launched.kind(), StudioUpdateStateKind::InstallerLaunched);
        assert_eq!(launched.revision(), 6);
        assert_eq!(launched.update(), Some(&update("2.0.0")));
        assert_eq!(
            serde_json::from_str::<StudioUpdateStateSnapshot>(
                &serde_json::to_string(&launched).unwrap()
            )
            .unwrap(),
            launched
        );
    }

    #[test]
    fn stale_illegal_and_mismatched_commands_are_rejected() {
        let idle = StudioUpdateStateSnapshot::idle(1);
        assert!(matches!(
            idle.decide(StudioUpdateCommand::BeginCheck {
                expected_revision: 9,
                operation_id: "late".to_string(),
                started_at: 2,
            }),
            Err(StudioUpdateTransitionError::StaleRevision { .. })
        ));
        assert!(matches!(
            idle.decide(StudioUpdateCommand::BeginVerify {
                expected_revision: 0,
                updated_at: 2,
            }),
            Err(StudioUpdateTransitionError::IllegalTransition { .. })
        ));

        let checking = next(
            &idle,
            StudioUpdateCommand::BeginCheck {
                expected_revision: 0,
                operation_id: "check-1".to_string(),
                started_at: 2,
            },
        );
        let available = next(
            &checking,
            StudioUpdateCommand::FinishAvailable {
                expected_revision: 1,
                checked_at: 3,
                update: update("2.0.0"),
            },
        );
        assert!(matches!(
            available.decide(StudioUpdateCommand::BeginDownload {
                expected_revision: 2,
                updated_at: 4,
                update: update("3.0.0"),
                total: 100,
            }),
            Err(StudioUpdateTransitionError::CorrelationMismatch { .. })
        ));
    }

    #[test]
    fn invalid_download_progress_is_rejected() {
        let checking = next(
            &StudioUpdateStateSnapshot::idle(1),
            StudioUpdateCommand::BeginCheck {
                expected_revision: 0,
                operation_id: "check-1".to_string(),
                started_at: 2,
            },
        );
        let available = next(
            &checking,
            StudioUpdateCommand::FinishAvailable {
                expected_revision: 1,
                checked_at: 3,
                update: update("2.0.0"),
            },
        );
        let downloading = next(
            &available,
            StudioUpdateCommand::BeginDownload {
                expected_revision: 2,
                updated_at: 4,
                update: update("2.0.0"),
                total: 100,
            },
        );
        assert!(matches!(
            downloading.decide(StudioUpdateCommand::ReportDownload {
                expected_revision: 3,
                updated_at: 5,
                downloaded: 101,
                total: 100,
            }),
            Err(StudioUpdateTransitionError::InvalidPayload { .. })
        ));
    }

    #[test]
    fn legacy_or_incomplete_json_is_rejected() {
        assert!(
            serde_json::from_str::<StudioUpdateStateSnapshot>(
                r#"{"phase":"available","version":"2.0.0"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<StudioUpdateStateSnapshot>(
                r#"{"kind":"available","data":{"revision":1,"checkedAt":2}}"#
            )
            .is_err()
        );
    }
}
