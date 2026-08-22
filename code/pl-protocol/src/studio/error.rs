//! Studio API 的稳定错误分类与脱敏错误体。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

static NEXT_CORRELATION_ID: AtomicU64 = AtomicU64::new(1);

/// Stable Studio API error categories shared by FRB and HTTP.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioErrorCode {
    NotInitialized,
    RuntimeStopped,
    InstanceBusy,
    InvalidArgument,
    NotFound,
    Busy,
    Conflict,
    StaleRevision,
    PermissionDenied,
    Cancelled,
    CancellationTooLate,
    Overloaded,
    Unavailable,
    Protocol,
    Storage,
    Update,
    Internal,
}

/// A redacted API error. Internal diagnostics are correlated through `correlation_id` only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, thiserror::Error, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[error("{message} (correlation id: {correlation_id})")]
pub struct StudioError {
    pub code: StudioErrorCode,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: String,
    #[schema(value_type = Object)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl StudioError {
    pub fn new(code: StudioErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            correlation_id: next_correlation_id(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(StudioErrorCode::InvalidArgument, message, false)
    }

    pub fn not_found(resource: &'static str) -> Self {
        Self::new(
            StudioErrorCode::NotFound,
            format!("The requested Studio {resource} was not found"),
            false,
        )
    }

    pub fn instance_busy() -> Self {
        Self::new(
            StudioErrorCode::InstanceBusy,
            "Another Studio runtime already owns this home",
            true,
        )
    }

    pub fn internal() -> Self {
        Self::new(
            StudioErrorCode::Internal,
            "Studio could not complete the operation",
            false,
        )
    }

    pub fn storage() -> Self {
        Self::new(
            StudioErrorCode::Storage,
            "Studio storage is unavailable",
            true,
        )
    }
}

fn next_correlation_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let sequence = NEXT_CORRELATION_ID.fetch_add(1, Ordering::Relaxed);
    format!("studio-{timestamp:x}-{sequence:x}")
}

pub type StudioResult<T> = Result<T, StudioError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_error_is_camel_case_and_redacted_by_construction() {
        let error = StudioError::internal();
        let value = serde_json::to_value(&error).unwrap();
        assert_eq!(value["code"], "internal");
        assert!(value.get("correlationId").is_some());
        assert!(!error.message.contains("secret"));
    }
}
