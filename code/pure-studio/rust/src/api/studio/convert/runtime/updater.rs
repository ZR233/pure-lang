//! Canonical updater state projection.

use crate::api::studio::types::*;
use pl_studio_runtime::{StudioUpdate, StudioUpdateStateSnapshot};

pub(crate) fn bridge_update_state(state: StudioUpdateStateSnapshot) -> BridgeUpdaterStateSnapshot {
    let revision = state.revision();
    let updated_at = state.updated_at();
    match state {
        StudioUpdateStateSnapshot::Disabled(_) => {
            BridgeUpdaterStateSnapshot::Disabled(BridgeDisabledUpdaterState {
                revision,
                updated_at,
            })
        }
        StudioUpdateStateSnapshot::Idle(_) => {
            BridgeUpdaterStateSnapshot::Idle(BridgeIdleUpdaterState {
                revision,
                updated_at,
            })
        }
        StudioUpdateStateSnapshot::Checking(value) => {
            BridgeUpdaterStateSnapshot::Checking(BridgeCheckingUpdaterState {
                revision,
                operation_id: value.operation_id().to_string(),
                started_at: value.started_at(),
            })
        }
        StudioUpdateStateSnapshot::UpToDate(value) => {
            BridgeUpdaterStateSnapshot::UpToDate(BridgeUpToDateUpdaterState {
                revision,
                checked_at: value.checked_at(),
            })
        }
        StudioUpdateStateSnapshot::Available(value) => {
            BridgeUpdaterStateSnapshot::Available(BridgeAvailableUpdaterState {
                revision,
                checked_at: value.checked_at(),
                update: bridge_update(value.update()),
            })
        }
        StudioUpdateStateSnapshot::Downloading(value) => {
            BridgeUpdaterStateSnapshot::Downloading(BridgeDownloadingUpdaterState {
                revision,
                updated_at,
                update: bridge_update(value.update()),
                downloaded: value.downloaded(),
                total: value.total(),
            })
        }
        StudioUpdateStateSnapshot::Verifying(value) => {
            BridgeUpdaterStateSnapshot::Verifying(BridgeVerifyingUpdaterState {
                revision,
                updated_at,
                update: bridge_update(value.update()),
                downloaded: value.downloaded(),
                total: value.total(),
            })
        }
        StudioUpdateStateSnapshot::InstallerLaunched(value) => {
            BridgeUpdaterStateSnapshot::InstallerLaunched(BridgeInstallerLaunchedUpdaterState {
                revision,
                launched_at: value.launched_at(),
                update: bridge_update(value.update()),
            })
        }
        StudioUpdateStateSnapshot::CheckFailed(value) => {
            BridgeUpdaterStateSnapshot::CheckFailed(BridgeCheckFailedUpdaterState {
                revision,
                failed_at: value.failed_at(),
                error: bridge_error(value.error()),
            })
        }
        StudioUpdateStateSnapshot::InstallFailed(value) => {
            BridgeUpdaterStateSnapshot::InstallFailed(BridgeInstallFailedUpdaterState {
                revision,
                failed_at: value.failed_at(),
                update: bridge_update(value.update()),
                error: bridge_error(value.error()),
            })
        }
    }
}

fn bridge_error(error: &pl_protocol::StateError) -> BridgeStateError {
    BridgeStateError {
        code: error.code.clone(),
        message: error.message.clone(),
        retryable: error.retryable,
    }
}

fn bridge_update(update: &StudioUpdate) -> BridgeVerifiedUpdateSummary {
    BridgeVerifiedUpdateSummary {
        version: update.version.clone(),
        published_at: update.published_at,
        notes_url: update.notes_url.clone(),
    }
}
