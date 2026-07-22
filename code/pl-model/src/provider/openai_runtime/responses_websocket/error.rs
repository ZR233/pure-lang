use pl_protocol::PureError;
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio_tungstenite::tungstenite::Error as TungsteniteError;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

use super::super::redact_secret_like_values;

const CONNECTION_LIMIT_CODE: &str = "websocket_connection_limit_reached";

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

pub(super) fn handshake_error(error: TungsteniteError) -> PureError {
    if let TungsteniteError::Http(response) = &error {
        let status = response.status().as_u16();
        let detail = format!("Responses WebSocket handshake failed with HTTP {status}");
        if retryable_status(status) {
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
    let parsed = serde_json::from_value::<WebSocketErrorEvent>(value.clone()).ok();
    let fallback_error = value.get("error").unwrap_or(value);
    let code = parsed
        .as_ref()
        .and_then(|event| event.error.as_ref())
        .and_then(|error| error.code.as_deref())
        .or_else(|| fallback_error.get("code").and_then(Value::as_str));
    let message = parsed
        .as_ref()
        .and_then(|event| event.error.as_ref())
        .and_then(|error| error.message.as_deref())
        .or_else(|| fallback_error.get("message").and_then(Value::as_str))
        .unwrap_or("Responses WebSocket returned an error");
    let status = parsed.as_ref().and_then(|event| {
        event
            .status
            .or_else(|| event.error.as_ref().and_then(|error| error.status))
    });
    let detail = websocket_error_message(status, code, message);
    let retry_after_ms = parsed.as_ref().and_then(|event| {
        event
            .error
            .as_ref()
            .and_then(|error| error.retry_after_ms)
            .or_else(|| retry_after_from_json_headers(&event.headers))
    });

    if code.is_some_and(retryable_code) || status.is_some_and(retryable_status) {
        return transient_provider_error(detail, retry_after_ms, code, status);
    }
    match status {
        Some(_) => PureError::HttpError(detail),
        None => PureError::LlmError(detail),
    }
}

pub(super) fn response_terminal_error(event: &Value) -> Option<PureError> {
    let response = event.get("response")?;
    let error = response.get("error");
    let code = error
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .or_else(|| {
            response
                .get("incomplete_details")
                .and_then(|details| details.get("reason"))
                .and_then(Value::as_str)
        });
    let status = error
        .and_then(|error| error.get("status"))
        .and_then(json_u16);
    if !code.is_some_and(retryable_code) && !status.is_some_and(retryable_status) {
        return None;
    }
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Responses WebSocket response failed temporarily");
    Some(transient_provider_error(
        websocket_error_message(status, code, message),
        None,
        code,
        status,
    ))
}

pub(super) fn continuation_id_invalid(value: &Value) -> bool {
    let error = value.get("error").unwrap_or(value);
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    code.contains("previous_response")
        || message.contains("previous_response_id")
        || (message.contains("response")
            && (message.contains("not found") || message.contains("invalid")))
}

fn transient(message: String, retry_after_ms: Option<u64>) -> PureError {
    PureError::transient_model_failure(message, retry_after_ms, None, None)
}

fn transient_provider_error(
    message: String,
    retry_after_ms: Option<u64>,
    code: Option<&str>,
    status: Option<u16>,
) -> PureError {
    PureError::transient_model_failure(
        message,
        retry_after_ms,
        code.map(ToString::to_string),
        status,
    )
}

fn retryable_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 425 | 429 | 500..=599)
}

fn retryable_code(code: &str) -> bool {
    matches!(
        code.to_ascii_lowercase().as_str(),
        CONNECTION_LIMIT_CODE
            | "rate_limit_exceeded"
            | "server_error"
            | "temporarily_unavailable"
            | "service_unavailable"
            | "request_timeout"
            | "server_is_overloaded"
    )
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
