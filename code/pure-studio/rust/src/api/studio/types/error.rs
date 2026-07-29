use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

static NEXT_CORRELATION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeErrorCode {
    NotInitialized,
    RuntimeStopped,
    InvalidArgument,
    NotFound,
    Busy,
    Conflict,
    StaleRevision,
    PermissionDenied,
    Cancelled,
    CancellationTooLate,
    Unavailable,
    Protocol,
    Storage,
    Update,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeError {
    pub code: BridgeErrorCode,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: String,
    pub details_json: Option<String>,
}

impl BridgeError {
    pub(crate) fn runtime_stopped() -> Self {
        Self::new(
            BridgeErrorCode::RuntimeStopped,
            "Studio runtime has stopped; restart the application",
            false,
        )
    }

    pub(crate) fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(BridgeErrorCode::InvalidArgument, message, false)
    }

    fn new(code: BridgeErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            correlation_id: next_correlation_id(),
            details_json: None,
        }
    }

    fn from_anyhow(error: anyhow::Error) -> Self {
        let diagnostic = format!("{error:#}");
        let normalized = diagnostic.to_ascii_lowercase();
        let (code, message, retryable) = if normalized.contains("not initialized") {
            (
                BridgeErrorCode::NotInitialized,
                "Studio runtime is not initialized",
                true,
            )
        } else if normalized.contains("not found") {
            (
                BridgeErrorCode::NotFound,
                "The requested Studio resource was not found",
                false,
            )
        } else if normalized.contains("permission") || normalized.contains("access denied") {
            (
                BridgeErrorCode::PermissionDenied,
                "Studio does not have permission to complete this operation",
                false,
            )
        } else if normalized.contains("busy") || normalized.contains("active turn") {
            (
                BridgeErrorCode::Busy,
                "Studio is busy with another operation",
                true,
            )
        } else if normalized.contains("revision") || normalized.contains("stale") {
            (
                BridgeErrorCode::StaleRevision,
                "Studio data changed; reload and try again",
                true,
            )
        } else if normalized.contains("cancel") {
            (
                BridgeErrorCode::Cancelled,
                "The Studio operation was cancelled",
                true,
            )
        } else if normalized.contains("sqlite")
            || normalized.contains("database")
            || normalized.contains("storage")
        {
            (
                BridgeErrorCode::Storage,
                "Studio storage is unavailable",
                true,
            )
        } else if normalized.contains("protocol") || normalized.contains("serialize") {
            (
                BridgeErrorCode::Protocol,
                "Studio received incompatible protocol data",
                false,
            )
        } else if normalized.contains("unavailable")
            || normalized.contains("connection")
            || normalized.contains("timeout")
        {
            (
                BridgeErrorCode::Unavailable,
                "A required Studio service is unavailable",
                true,
            )
        } else {
            (
                BridgeErrorCode::Internal,
                "Studio could not complete the operation",
                false,
            )
        };
        let bridge_error = Self::new(code, message, retryable);
        tracing::error!(
            correlation_id = %bridge_error.correlation_id,
            error = %diagnostic,
            "Studio bridge operation failed"
        );
        bridge_error
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} (correlation id: {})",
            self.message, self.correlation_id
        )
    }
}

impl std::error::Error for BridgeError {}

impl From<anyhow::Error> for BridgeError {
    fn from(error: anyhow::Error) -> Self {
        Self::from_anyhow(error)
    }
}

impl From<serde_json::Error> for BridgeError {
    fn from(error: serde_json::Error) -> Self {
        Self::from_anyhow(error.into())
    }
}

impl From<pl_protocol::PureError> for BridgeError {
    fn from(error: pl_protocol::PureError) -> Self {
        Self::from_anyhow(anyhow::Error::new(error))
    }
}

impl From<pl_studio_runtime::StudioUpdateError> for BridgeError {
    fn from(error: pl_studio_runtime::StudioUpdateError) -> Self {
        use pl_studio_runtime::StudioUpdateErrorCode;

        let code = error.code();
        let retryable = matches!(
            code,
            StudioUpdateErrorCode::Network
                | StudioUpdateErrorCode::RuntimeBusy
                | StudioUpdateErrorCode::InstallInProgress
                | StudioUpdateErrorCode::Io
        );
        let bridge_code = if matches!(code, StudioUpdateErrorCode::RuntimeBusy) {
            BridgeErrorCode::Busy
        } else if matches!(code, StudioUpdateErrorCode::Cancelled) {
            BridgeErrorCode::Cancelled
        } else if matches!(code, StudioUpdateErrorCode::CancellationTooLate) {
            BridgeErrorCode::CancellationTooLate
        } else {
            BridgeErrorCode::Update
        };
        let bridge_error = Self::new(
            bridge_code,
            "Studio update could not be completed",
            retryable,
        );
        tracing::error!(
            correlation_id = %bridge_error.correlation_id,
            error = %error,
            update_code = code.as_str(),
            "Studio update operation failed"
        );
        bridge_error
    }
}

fn next_correlation_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let sequence = NEXT_CORRELATION_ID.fetch_add(1, Ordering::Relaxed);
    format!("bridge-{timestamp:x}-{sequence:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_error_hides_sensitive_cause_and_has_correlation_id() {
        let error = BridgeError::from(anyhow::anyhow!(
            "provider token secret-token at C:\\private\\config.toml"
        ));

        assert_eq!(error.code, BridgeErrorCode::Internal);
        assert!(!error.message.contains("secret-token"));
        assert!(!error.message.contains("config.toml"));
        assert!(error.correlation_id.starts_with("bridge-"));
    }

    #[test]
    fn retryable_categories_are_stable() {
        let storage = BridgeError::from(anyhow::anyhow!("sqlite database locked"));
        let not_found = BridgeError::from(anyhow::anyhow!("session not found"));

        assert_eq!(storage.code, BridgeErrorCode::Storage);
        assert!(storage.retryable);
        assert_eq!(not_found.code, BridgeErrorCode::NotFound);
        assert!(!not_found.retryable);
    }

    #[test]
    fn update_cancellation_codes_remain_typed() {
        let cancelled = BridgeError::from(pl_studio_runtime::StudioUpdateError::new(
            pl_studio_runtime::StudioUpdateErrorCode::Cancelled,
            "cancelled before launch",
        ));
        let too_late = BridgeError::from(pl_studio_runtime::StudioUpdateError::new(
            pl_studio_runtime::StudioUpdateErrorCode::CancellationTooLate,
            "installer already launched",
        ));

        assert_eq!(cancelled.code, BridgeErrorCode::Cancelled);
        assert_eq!(too_late.code, BridgeErrorCode::CancellationTooLate);
        assert!(!too_late.retryable);
    }
}
