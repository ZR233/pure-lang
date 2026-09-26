use pl_protocol::PureError;
use pl_protocol::studio::{StudioError, StudioErrorCode};

use crate::ConfigRuntimeError;
use crate::{StudioDatabaseError, StudioUpdateError, StudioUpdateErrorCode};

/// Maps an internal runtime failure to the shared, redacted Studio API error.
///
/// Classification is based exclusively on typed error sources. Unclassified
/// failures are logged for diagnostics and cross the adapter boundary only as
/// a redacted internal error.
pub fn studio_error_from_anyhow(error: anyhow::Error) -> StudioError {
    studio_error_from_ref(&error)
}

fn studio_error_from_ref(error: &anyhow::Error) -> StudioError {
    if let Some(error) = error.downcast_ref::<StudioError>() {
        return error.clone();
    }
    if error.downcast_ref::<StudioDatabaseError>().is_some()
        || error.downcast_ref::<std::io::Error>().is_some()
    {
        return StudioError::storage();
    }
    if let Some(error) = error.downcast_ref::<ConfigRuntimeError>() {
        return match error {
            ConfigRuntimeError::StaleRevision { expected, actual } => StudioError::new(
                StudioErrorCode::StaleRevision,
                "Studio data changed; reload and try again",
                true,
            )
            .with_details(serde_json::json!({
                "expectedRevision": expected,
                "actualRevision": actual,
            })),
            ConfigRuntimeError::Core(error) => pure_error(error),
        };
    }
    if error.downcast_ref::<serde_json::Error>().is_some() {
        return StudioError::new(
            StudioErrorCode::Protocol,
            "Studio received incompatible protocol data",
            false,
        );
    }
    if let Some(error) = error.downcast_ref::<StudioUpdateError>() {
        return update_error(error);
    }
    if let Some(error) = error.downcast_ref::<pl_core::thread::ThreadError>()
        && let Some(studio_error) = thread_error(error)
    {
        return studio_error;
    }
    if let Some(error) = error.downcast_ref::<PureError>() {
        return pure_error(error);
    }

    let studio_error = StudioError::internal();
    tracing::error!(
        correlation_id = %studio_error.correlation_id,
        operation = "studioRuntime",
        error_code = "internal",
        diagnostic_bytes = error.to_string().len(),
        "unclassified Studio runtime failure"
    );
    studio_error
}

/// Maps the prompt-admission failures of a Thread to their public category.
///
/// Only variants with a distinct, actionable meaning are classified; every
/// other variant returns `None` so the caller keeps the redacted internal
/// fallback. Messages never embed the source error text or any caller context.
fn thread_error(error: &pl_core::thread::ThreadError) -> Option<StudioError> {
    match error {
        pl_core::thread::ThreadError::PendingInteraction => Some(StudioError::new(
            StudioErrorCode::Conflict,
            "Resolve the pending Studio interaction before sending another prompt",
            false,
        )),
        pl_core::thread::ThreadError::StoragePressure => Some(StudioError::new(
            StudioErrorCode::Busy,
            "Studio is still persisting earlier work; retry shortly",
            true,
        )),
        pl_core::thread::ThreadError::StorageRecoveryPending { .. } => Some(StudioError::new(
            StudioErrorCode::Busy,
            "Studio has not saved the recovered history yet; retry saving before continuing",
            true,
        )),
        pl_core::thread::ThreadError::StorageRecoveryUnverified { .. } => Some(StudioError::new(
            StudioErrorCode::Busy,
            "Studio has not verified that this save fault recovered; retry saving before continuing",
            true,
        )),
        pl_core::thread::ThreadError::Storage(_) => Some(StudioError::storage()),
        pl_core::thread::ThreadError::Closed => Some(StudioError::new(
            StudioErrorCode::RuntimeStopped,
            "Studio runtime stopped; restart the runtime to continue",
            false,
        )),
        _ => None,
    }
}

fn pure_error(error: &PureError) -> StudioError {
    match error {
        PureError::PermissionDenied(_) | PureError::SandboxError(_) => StudioError::new(
            StudioErrorCode::PermissionDenied,
            "Studio does not have permission to complete this operation",
            false,
        ),
        PureError::ConfigError(_) => StudioError::new(
            StudioErrorCode::InvalidArgument,
            "Studio received invalid configuration",
            false,
        ),
        PureError::SerdeJson(_) | PureError::Protocol(_) => StudioError::new(
            StudioErrorCode::Protocol,
            "Studio received incompatible protocol data",
            false,
        ),
        PureError::Io(_) | PureError::MemoryError(_) => StudioError::storage(),
        PureError::HttpError(_)
        | PureError::TransientModelTransport { .. }
        | PureError::ProviderCapacity { .. }
        | PureError::Provider(_) => StudioError::new(
            StudioErrorCode::Unavailable,
            "A required Studio service is unavailable",
            true,
        ),
        PureError::LlmError(_)
        | PureError::ContextOverflow(_)
        | PureError::ToolNotFound(_)
        | PureError::ToolExecutionFailed { .. }
        | PureError::AgentLimitReached { .. }
        | PureError::AgentDepthLimitReached { .. } => StudioError::internal(),
    }
}

fn update_error(error: &StudioUpdateError) -> StudioError {
    let code = error.code();
    let retryable = matches!(
        code,
        StudioUpdateErrorCode::Network
            | StudioUpdateErrorCode::RuntimeBusy
            | StudioUpdateErrorCode::InstallInProgress
            | StudioUpdateErrorCode::Io
    );
    let studio_code = match code {
        StudioUpdateErrorCode::RuntimeBusy | StudioUpdateErrorCode::InstallInProgress => {
            StudioErrorCode::Busy
        }
        StudioUpdateErrorCode::Cancelled => StudioErrorCode::Cancelled,
        StudioUpdateErrorCode::CancellationTooLate => StudioErrorCode::CancellationTooLate,
        StudioUpdateErrorCode::Network => StudioErrorCode::Unavailable,
        StudioUpdateErrorCode::InvalidManifest
        | StudioUpdateErrorCode::UnsupportedPlatform
        | StudioUpdateErrorCode::DownloadTooLarge
        | StudioUpdateErrorCode::DownloadIncomplete
        | StudioUpdateErrorCode::HashMismatch
        | StudioUpdateErrorCode::SignatureInvalid
        | StudioUpdateErrorCode::InstallerLaunchFailed
        | StudioUpdateErrorCode::Io => StudioErrorCode::Update,
    };
    StudioError::new(
        studio_code,
        "Studio update could not be completed",
        retryable,
    )
}
