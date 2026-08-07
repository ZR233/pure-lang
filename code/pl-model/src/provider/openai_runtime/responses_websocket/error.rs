use pl_protocol::PureError;
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio_tungstenite::tungstenite::Error as TungsteniteError;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

use super::super::provider_error::{
    ProviderFailureMetadata, redact_secret_like_values, retryable_provider_status,
};

const HANDSHAKE_TIMEOUT_MESSAGE: &str = "Responses WebSocket handshake timed out after 15 seconds; check WebSocket network access or switch this provider instance to HTTP explicitly in Studio settings";

#[derive(Debug, Deserialize)]
struct WebSocketErrorDetail {
    code: Option<String>,
    message: Option<String>,
    retry_after_ms: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_u16")]
    status: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct WebSocketErrorEvent {
    code: Option<String>,
    message: Option<String>,
    retry_after_ms: Option<u64>,
    #[serde(
        default,
        alias = "status_code",
        deserialize_with = "deserialize_optional_u16"
    )]
    status: Option<u16>,
    #[serde(default)]
    error: Option<WebSocketErrorDetail>,
    #[serde(default)]
    headers: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct WebSocketTerminalEvent {
    response: WebSocketTerminalResponse,
}

#[derive(Debug, Deserialize)]
struct WebSocketTerminalResponse {
    #[serde(default)]
    error: Option<WebSocketErrorDetail>,
    #[serde(default)]
    incomplete_details: Option<WebSocketIncompleteDetails>,
}

#[derive(Debug, Deserialize)]
struct WebSocketIncompleteDetails {
    reason: Option<String>,
}

pub(super) fn handshake_error(error: TungsteniteError) -> PureError {
    if let TungsteniteError::Http(response) = &error {
        let status = response.status().as_u16();
        let detail = format!("Responses WebSocket handshake failed with HTTP {status}");
        if retryable_provider_status(status) {
            return transient(detail, retry_after_from_http_headers(response.headers()));
        }
        if status == 426 {
            return PureError::ConfigError(
                "Responses WebSocket upgrade was rejected with HTTP 426; switch this provider instance to HTTP explicitly"
                    .to_string(),
            );
        }
        return PureError::HttpError(detail);
    }

    let detail =
        redact_secret_like_values(&format!("Responses WebSocket handshake failed: {error}"));
    match error {
        TungsteniteError::Protocol(_)
        | TungsteniteError::Capacity(_)
        | TungsteniteError::Utf8(_)
        | TungsteniteError::AttackAttempt
        | TungsteniteError::Url(_)
        | TungsteniteError::HttpFormat(_) => PureError::LlmError(detail),
        TungsteniteError::ConnectionClosed
        | TungsteniteError::AlreadyClosed
        | TungsteniteError::Io(_)
        | TungsteniteError::Tls(_)
        | TungsteniteError::WriteBufferFull(_) => PureError::transient_model_transport(detail),
        TungsteniteError::Http(_) => unreachable!("HTTP handshake error handled above"),
    }
}

pub(super) fn handshake_timeout_error() -> PureError {
    PureError::transient_model_transport(HANDSHAKE_TIMEOUT_MESSAGE)
}

pub(super) fn connection_error(detail: impl AsRef<str>) -> PureError {
    PureError::transient_model_transport(redact_secret_like_values(&format!(
        "Responses WebSocket stream failed: {}",
        detail.as_ref()
    )))
}

pub(super) fn close_error(frame: Option<CloseFrame>) -> PureError {
    let Some(frame) = frame else {
        return connection_error("server closed the connection without a close frame");
    };
    let code = u16::from(frame.code);
    let detail = redact_secret_like_values(&format!(
        "Responses WebSocket server closed the connection ({code}): {}",
        frame.reason
    ));
    if matches!(code, 1002 | 1003 | 1007 | 1008 | 1009 | 1010) {
        PureError::LlmError(detail)
    } else {
        PureError::transient_model_transport(detail)
    }
}

pub(super) fn protocol_error(detail: impl AsRef<str>) -> PureError {
    PureError::LlmError(redact_secret_like_values(&format!(
        "Responses WebSocket protocol error: {}",
        detail.as_ref()
    )))
}

pub(super) fn server_error(value: &Value) -> PureError {
    let parsed = match serde_json::from_value::<WebSocketErrorEvent>(value.clone()) {
        Ok(parsed) => parsed,
        Err(error) => return protocol_error(format!("invalid server error event: {error}")),
    };
    let nested = parsed.error.as_ref();
    let code = nested
        .and_then(|error| error.code.as_deref())
        .or(parsed.code.as_deref());
    let message = nested
        .and_then(|error| error.message.as_deref())
        .or(parsed.message.as_deref())
        .unwrap_or("Responses WebSocket returned an error");
    let status = parsed
        .status
        .or_else(|| nested.and_then(|error| error.status));
    let detail = websocket_error_message(status, code, message);
    let retry_after_ms = nested
        .and_then(|error| error.retry_after_ms)
        .or(parsed.retry_after_ms)
        .or_else(|| retry_after_from_json_headers(&parsed.headers));
    let metadata = ProviderFailureMetadata {
        code,
        http_status: status,
        retry_after_ms,
    };

    if metadata.is_retryable() {
        return metadata.into_transient(detail);
    }
    match status {
        Some(_) => PureError::HttpError(detail),
        None => PureError::LlmError(detail),
    }
}

pub(super) fn response_terminal_error(event: &Value) -> Option<PureError> {
    let parsed = serde_json::from_value::<WebSocketTerminalEvent>(event.clone()).ok()?;
    let error = parsed.response.error.as_ref();
    let code = error.and_then(|error| error.code.as_deref()).or_else(|| {
        parsed
            .response
            .incomplete_details
            .as_ref()
            .and_then(|details| details.reason.as_deref())
    });
    let status = error.and_then(|error| error.status);
    let metadata = ProviderFailureMetadata {
        code,
        http_status: status,
        retry_after_ms: error.and_then(|error| error.retry_after_ms),
    };
    if !metadata.is_retryable() {
        return None;
    }
    let message = error
        .and_then(|error| error.message.as_deref())
        .unwrap_or("Responses WebSocket response failed temporarily");
    Some(metadata.into_transient(websocket_error_message(status, code, message)))
}

pub(super) fn continuation_id_invalid(value: &Value) -> bool {
    let Ok(parsed) = serde_json::from_value::<WebSocketErrorEvent>(value.clone()) else {
        return false;
    };
    let nested = parsed.error.as_ref();
    let code = nested
        .and_then(|error| error.code.as_deref())
        .or(parsed.code.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let message = nested
        .and_then(|error| error.message.as_deref())
        .or(parsed.message.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();
    code.contains("previous_response")
        || message.contains("previous_response_id")
        || (message.contains("response")
            && (message.contains("not found") || message.contains("invalid")))
}

pub(super) fn continuation_retry_error() -> PureError {
    PureError::transient_model_transport(
        "Responses WebSocket previous_response_id is no longer valid; retrying with full history",
    )
}

fn transient(message: String, retry_after_ms: Option<u64>) -> PureError {
    PureError::transient_model_failure(message, retry_after_ms, None, None)
}

fn websocket_error_message(status: Option<u16>, code: Option<&str>, message: &str) -> String {
    let label = match (status, code) {
        (Some(status), Some(code)) => format!("Responses WebSocket error {code} (HTTP {status})"),
        (Some(status), None) => format!("Responses WebSocket error (HTTP {status})"),
        (None, Some(code)) => format!("Responses WebSocket error {code}"),
        (None, None) => "Responses WebSocket error".to_string(),
    };
    redact_secret_like_values(&format!("{label}: {message}"))
}

fn retry_after_from_http_headers(
    headers: &tokio_tungstenite::tungstenite::http::HeaderMap,
) -> Option<u64> {
    headers
        .get("retry-after-ms")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            headers
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(|seconds| seconds.saturating_mul(1_000))
        })
}

fn retry_after_from_json_headers(headers: &Map<String, Value>) -> Option<u64> {
    headers
        .get("retry-after-ms")
        .or_else(|| headers.get("retry_after_ms"))
        .and_then(json_u64)
        .or_else(|| {
            headers
                .get("retry-after")
                .and_then(json_u64)
                .map(|seconds| seconds.saturating_mul(1_000))
        })
}

fn json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn json_u16(value: &Value) -> Option<u16> {
    json_u64(value).and_then(|value| u16::try_from(value).ok())
}

fn deserialize_optional_u16<'de, D>(deserializer: D) -> std::result::Result<Option<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => json_u16(&value)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("status must be an unsigned 16-bit integer")),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn handshake_timeout_is_transient_and_suggests_http_mode() {
        let error = handshake_timeout_error();

        assert!(error.is_transient_model_transport());
        assert_eq!(
            error.to_string(),
            format!("transient model transport error: {HANDSHAKE_TIMEOUT_MESSAGE}")
        );
    }

    #[test]
    fn classifies_connection_limit_and_retry_after_as_transient() {
        let error = server_error(&serde_json::json!({
            "type": "error",
            "status": 400,
            "error": {
                "code": "websocket_connection_limit_reached",
                "message": "create a new connection"
            },
            "headers": { "retry-after-ms": "250" }
        }));

        assert!(error.is_transient_model_transport());
        assert_eq!(error.retry_after_ms(), Some(250));
        assert_eq!(
            error.transient_model_metadata(),
            Some((Some("websocket_connection_limit_reached"), Some(400)))
        );

        let compatible_status = server_error(&serde_json::json!({
            "type": "error",
            "status": "503",
            "error": { "message": "proxy unavailable" }
        }));
        assert!(compatible_status.is_transient_model_transport());
    }

    #[test]
    fn classifies_server_overload_without_http_status_as_transient() {
        let error = server_error(&serde_json::json!({
            "type": "error",
            "error": {
                "code": "server_is_overloaded",
                "message": "Our servers are currently overloaded. Please try again later.",
                "retry_after_ms": 750
            }
        }));

        assert!(error.is_transient_model_transport());
        assert_eq!(error.retry_after_ms(), Some(750));
        assert_eq!(
            error.transient_model_metadata(),
            Some((Some("server_is_overloaded"), None))
        );
    }

    #[test]
    fn keeps_invalid_request_non_retryable() {
        let error = server_error(&serde_json::json!({
            "type": "error",
            "status": 400,
            "error": {
                "code": "invalid_request_error",
                "message": "model does not support image inputs"
            }
        }));

        assert!(!error.is_transient_model_transport());
        assert!(error.to_string().contains("HTTP 400"));
    }

    #[test]
    fn classifies_only_transient_terminal_response_failures() {
        let transient = response_terminal_error(&serde_json::json!({
            "type": "response.failed",
            "response": {
                "error": {
                    "code": "server_error",
                    "message": "upstream unavailable"
                }
            }
        }))
        .expect("server error is transient");
        let content_filter = response_terminal_error(&serde_json::json!({
            "type": "response.incomplete",
            "response": {
                "incomplete_details": { "reason": "content_filter" }
            }
        }));

        assert!(transient.is_transient_model_transport());
        assert!(content_filter.is_none());
    }
}
