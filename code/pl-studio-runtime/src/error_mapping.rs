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

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct CapturedLog(Arc<Mutex<Vec<u8>>>);

    struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLog {
        type Writer = CapturedWriter;

        fn make_writer(&'a self) -> Self::Writer {
            CapturedWriter(self.0.clone())
        }
    }

    impl Write for CapturedWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn unclassified_error_is_redacted() {
        let captured = CapturedLog::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(captured.clone())
            .finish();
        let error = tracing::subscriber::with_default(subscriber, || {
            studio_error_from_anyhow(anyhow::anyhow!(
                "provider token secret-token at /private/config.toml"
            ))
        });
        let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();

        assert_eq!(error.code, StudioErrorCode::Internal);
        assert!(!error.message.contains("secret-token"));
        assert!(!error.message.contains("config.toml"));
        assert!(log.contains(&error.correlation_id));
        assert!(log.contains("operation=\"studioRuntime\""));
        assert!(!log.contains("secret-token"));
        assert!(!log.contains("config.toml"));
    }

    #[test]
    fn typed_storage_error_keeps_its_category_through_context() {
        let source = StudioDatabaseError::UnsupportedSchema {
            found: 11,
            supported: 10,
        };
        let error = anyhow::Error::new(source).context("runtime startup failed");

        assert_eq!(
            studio_error_from_anyhow(error).code,
            StudioErrorCode::Storage
        );
    }
}
