use async_openai::error::OpenAIError;
use pl_protocol::{ProviderFailure, ProviderFailureKind, PureError, RetryDisposition};

#[derive(Debug, Clone, Copy)]
pub(super) struct ProviderFailureMetadata<'a> {
    pub code: Option<&'a str>,
    pub http_status: Option<u16>,
    pub retry_after_ms: Option<u64>,
}

impl ProviderFailureMetadata<'_> {
    pub fn is_retryable(self) -> bool {
        self.http_status.is_some_and(retryable_provider_status)
            || self.code.is_some_and(retryable_provider_code)
    }

    pub fn into_transient(self, message: String) -> PureError {
        PureError::provider_failure(ProviderFailure {
            kind: provider_failure_kind(self.code, self.http_status, true),
            code: self.code.map(ToString::to_string),
            http_status: self.http_status,
            message,
            retry: RetryDisposition::Retryable {
                retry_after_ms: self.retry_after_ms,
            },
        })
    }

    pub fn into_permanent(self, message: String) -> PureError {
        PureError::provider_failure(ProviderFailure {
            kind: provider_failure_kind(self.code, self.http_status, false),
            code: self.code.map(ToString::to_string),
            http_status: self.http_status,
            message,
            retry: RetryDisposition::Permanent,
        })
    }
}

pub(crate) fn openai_error_to_pure(error: OpenAIError) -> PureError {
    match error {
        OpenAIError::ApiError(api_error) => {
            let status = api_error.status_code.as_u16();
            let metadata = ProviderFailureMetadata {
                code: api_error.api_error.code.as_deref(),
                http_status: Some(status),
                retry_after_ms: None,
            };
            let detail = redact_secret_like_values(&format!("API error {api_error}"));
            if metadata.is_retryable() {
                metadata.into_transient(detail)
            } else {
                metadata.into_permanent(detail)
            }
        }
        OpenAIError::Reqwest(error) => reqwest_error_to_pure(error),
        OpenAIError::JSONDeserialize(error, content) => {
            protocol_failure(redact_secret_like_values(&format!("{error}: {content}")))
        }
        OpenAIError::StreamError(error) => stream_error_to_pure(&error),
        OpenAIError::InvalidArgument(message) => configuration_failure(message),
        OpenAIError::FileSaveError(message) | OpenAIError::FileReadError(message) => {
            PureError::Io(std::io::Error::other(message))
        }
    }
}

pub(crate) fn provider_stream_failure(
    code: Option<&str>,
    http_status: Option<u16>,
    retry_after_ms: Option<u64>,
    message: String,
) -> PureError {
    let metadata = ProviderFailureMetadata {
        code,
        http_status,
        retry_after_ms,
    };
    let message = redact_secret_like_values(&message);
    if metadata.is_retryable() {
        metadata.into_transient(message)
    } else {
        metadata.into_permanent(message)
    }
}

/// SSE 流中断：传输层根因（连接被掐断/响应体解码失败）按瞬态处理并允许重试；
/// 纯协议解析错误仍保持 Permanent。
fn stream_error_to_pure(error: &async_openai::error::StreamError) -> PureError {
    match error {
        async_openai::error::StreamError::EventStream(detail) => {
            let detail = redact_secret_like_values(detail);
            if eventstream_transport_failure(&detail) {
                PureError::transient_model_failure(detail, None, None, None)
            } else {
                protocol_failure(detail)
            }
        }
        async_openai::error::StreamError::UnknownEvent(_) => {
            protocol_failure(redact_secret_like_values(&error.to_string()))
        }
    }
}

/// eventsource_stream 的错误只剩字符串；按已知传输层签名识别瞬时网络中断。
fn eventstream_transport_failure(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    [
        "transport error",
        "error decoding response body",
        "error reading a body",
        "connection closed",
        "connection reset",
        "broken pipe",
        "unexpected eof",
        "incomplete message",
    ]
    .iter()
    .any(|signature| lower.contains(signature))
}

fn reqwest_error_to_pure(error: reqwest::Error) -> PureError {
    let detail = redact_secret_like_values(&error.to_string());
    if error.is_timeout() || error.is_connect() || response_start_connection_closed(&error) {
        return PureError::transient_model_failure(
            detail,
            None,
            None,
            error.status().map(|status| status.as_u16()),
        );
    }
    if error.is_builder() || error.is_redirect() {
        configuration_failure(detail)
    } else if error.is_decode() || error.is_body() {
        // 响应体解码失败多由连接中途断开引起（与 ConnectionReset 同性质），
        // 只有确认无传输层根因时才视为协议错误。
        if response_start_connection_closed(&error) {
            PureError::transient_model_failure(
                detail,
                None,
                None,
                error.status().map(|status| status.as_u16()),
            )
        } else {
            protocol_failure(detail)
        }
    } else {
        PureError::provider_failure(ProviderFailure {
            kind: ProviderFailureKind::Unknown,
            code: None,
            http_status: error.status().map(|status| status.as_u16()),
            message: detail,
            retry: RetryDisposition::Permanent,
        })
    }
}

fn configuration_failure(message: impl Into<String>) -> PureError {
    PureError::provider_failure(ProviderFailure {
        kind: ProviderFailureKind::Configuration,
        code: None,
        http_status: None,
        message: message.into(),
        retry: RetryDisposition::Permanent,
    })
}

fn protocol_failure(message: impl Into<String>) -> PureError {
    PureError::provider_failure(ProviderFailure {
        kind: ProviderFailureKind::Protocol,
        code: None,
        http_status: None,
        message: message.into(),
        retry: RetryDisposition::Permanent,
    })
}

fn response_start_connection_closed(error: &reqwest::Error) -> bool {
    if error.is_builder() || error.is_redirect() || error.is_status() {
        return false;
    }
    // decode/body 错误需要保留：它们可能根源于连接中断，逐层检查 source。
    let mut source: Option<&dyn std::error::Error> = Some(error);
    while let Some(cause) = source {
        if cause.downcast_ref::<std::io::Error>().is_some_and(|error| {
            matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::TimedOut
            )
        }) || cause
            .to_string()
            .to_ascii_lowercase()
            .contains("connection closed before message completed")
        {
            return true;
        }
        source = cause.source();
    }
    false
}

pub(super) fn retryable_provider_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 425 | 429 | 500..=599)
}

fn provider_failure_kind(
    code: Option<&str>,
    http_status: Option<u16>,
    retryable: bool,
) -> ProviderFailureKind {
    if matches!(http_status, Some(401))
        || code.is_some_and(|code| matches!(code, "invalid_api_key" | "authentication_error"))
    {
        return ProviderFailureKind::Authentication;
    }
    if matches!(http_status, Some(403))
        || code.is_some_and(|code| matches!(code, "permission_denied" | "insufficient_permissions"))
    {
        return ProviderFailureKind::Authorization;
    }
    if retryable {
        return if matches!(http_status, Some(429 | 500..=599))
            || code.is_some_and(retryable_provider_code)
        {
            ProviderFailureKind::Capacity
        } else {
            ProviderFailureKind::Transport
        };
    }
    if matches!(http_status, Some(400 | 404 | 405 | 422))
        || code.is_some_and(|code| {
            matches!(
                code,
                "model_not_found" | "invalid_request_error" | "unsupported_model"
            )
        })
    {
        return ProviderFailureKind::Configuration;
    }
    ProviderFailureKind::Unknown
}

fn retryable_provider_code(code: &str) -> bool {
    matches!(
        code.to_ascii_lowercase().as_str(),
        "websocket_connection_limit_reached"
            | "rate_limit_exceeded"
            | "server_error"
            | "temporarily_unavailable"
            | "service_unavailable"
            | "request_timeout"
            | "server_is_overloaded"
    )
}

pub(super) fn redact_secret_like_values(input: &str) -> String {
    input
        .split_whitespace()
        .map(redact_secret_like_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_secret_like_token(token: &str) -> String {
    let trimmed = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '.' | ',' | ';' | ':' | ')' | '(' | '"' | '\'' | '[' | ']' | '{' | '}'
        )
    });
    if !looks_like_secret_token(trimmed) {
        return token.to_string();
    }
    token.replacen(trimmed, "[REDACTED_API_KEY]", 1)
}

fn looks_like_secret_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    (lower.starts_with("sk-") || lower.starts_with("sk_"))
        && token.len() >= 12
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '*' | '.'))
}

#[cfg(test)]
mod tests {
    use async_openai::error::{ApiError, ApiErrorResponse};
    use pretty_assertions::assert_eq;
    use reqwest::StatusCode;

    use super::*;

    #[test]
    fn http_overload_preserves_retry_metadata() {
        let error = openai_error_to_pure(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            Some("server_is_overloaded"),
        ));

        assert!(error.is_transient_model_transport());
        assert_eq!(
            error.transient_model_metadata(),
            Some((Some("server_is_overloaded"), Some(503)))
        );
    }

    #[test]
    fn http_bad_request_remains_permanent() {
        let error =
            openai_error_to_pure(api_error(StatusCode::BAD_REQUEST, Some("invalid_request")));

        assert!(!error.is_transient_model_transport());
        let failure = error
            .provider_failure_ref()
            .expect("typed provider failure");
        assert_eq!(failure.kind, ProviderFailureKind::Configuration);
        assert_eq!(failure.http_status, Some(400));
        assert!(!failure.retry.is_retryable());
    }

    #[test]
    fn request_build_errors_remain_permanent() {
        let request_error = reqwest::Client::new()
            .get("://invalid-url")
            .build()
            .expect_err("invalid URL must fail while building the request");
        let error = openai_error_to_pure(OpenAIError::Reqwest(request_error));

        assert!(!error.is_transient_model_transport());
        assert_eq!(
            error.provider_failure_ref().map(|failure| failure.kind),
            Some(ProviderFailureKind::Configuration)
        );
    }

    #[test]
    fn retryable_http_statuses_match_the_transport_policy() {
        for status in [408, 409, 425, 429, 500, 503, 599] {
            assert!(retryable_provider_status(status), "status {status}");
        }
        for status in [400, 401, 403, 404, 422] {
            assert!(!retryable_provider_status(status), "status {status}");
        }
    }

    #[test]
    fn invalid_api_key_is_typed_permanent_and_redacted() {
        let secret = "sk-super-secret-provider-key";
        let error = openai_error_to_pure(api_error_with_message(
            StatusCode::UNAUTHORIZED,
            Some("invalid_api_key"),
            &format!("Invalid API key {secret}"),
        ));

        let failure = error
            .provider_failure_ref()
            .expect("typed provider failure");
        assert_eq!(failure.kind, ProviderFailureKind::Authentication);
        assert_eq!(failure.code.as_deref(), Some("invalid_api_key"));
        assert_eq!(failure.http_status, Some(401));
        assert!(!failure.retry.is_retryable());
        assert!(!failure.message.contains(secret));
        assert!(failure.message.contains("[REDACTED_API_KEY]"));
    }

    #[test]
    fn permission_and_missing_model_are_fatal_provider_kinds() {
        let forbidden =
            openai_error_to_pure(api_error(StatusCode::FORBIDDEN, Some("permission_denied")));
        assert_eq!(
            forbidden.provider_failure_ref().map(|failure| failure.kind),
            Some(ProviderFailureKind::Authorization)
        );

        let missing_model =
            openai_error_to_pure(api_error(StatusCode::NOT_FOUND, Some("model_not_found")));
        assert_eq!(
            missing_model
                .provider_failure_ref()
                .map(|failure| failure.kind),
            Some(ProviderFailureKind::Configuration)
        );
    }

    fn api_error(status_code: StatusCode, code: Option<&str>) -> OpenAIError {
        api_error_with_message(
            status_code,
            code,
            "Our servers are currently overloaded. Please try again later.",
        )
    }

    fn api_error_with_message(
        status_code: StatusCode,
        code: Option<&str>,
        message: &str,
    ) -> OpenAIError {
        OpenAIError::ApiError(ApiErrorResponse {
            status_code,
            api_error: ApiError {
                message: message.to_string(),
                r#type: Some("server_error".to_string()),
                param: None,
                code: code.map(ToString::to_string),
            },
        })
    }
    #[test]
    fn stream_transport_drop_is_transient_and_retryable() {
        let error = openai_error_to_pure(OpenAIError::StreamError(Box::new(
            async_openai::error::StreamError::EventStream(
                "Transport error: error decoding response body".to_string(),
            ),
        )));

        assert!(error.is_transient_model_transport());
        let failure = error
            .provider_failure_ref()
            .expect("typed provider failure");
        assert_eq!(failure.kind, ProviderFailureKind::Transport);
        assert!(failure.retry.is_retryable());
    }

    #[test]
    fn stream_connection_reset_signature_is_transient() {
        for detail in [
            "EventStream error: Transport error: connection reset by peer",
            "EventStream error: error reading a body from connection",
            "EventStream error: connection closed before message completed",
        ] {
            let error = openai_error_to_pure(OpenAIError::StreamError(Box::new(
                async_openai::error::StreamError::EventStream(detail.to_string()),
            )));
            assert!(
                error.is_transient_model_transport(),
                "detail must classify as transient: {detail}"
            );
        }
    }

    #[test]
    fn stream_pure_parse_failure_remains_permanent() {
        let error = openai_error_to_pure(OpenAIError::StreamError(Box::new(
            async_openai::error::StreamError::EventStream(
                "expected field `id` of type String".to_string(),
            ),
        )));

        assert!(!error.is_transient_model_transport());
        let failure = error
            .provider_failure_ref()
            .expect("typed provider failure");
        assert_eq!(failure.kind, ProviderFailureKind::Protocol);
        assert!(!failure.retry.is_retryable());
    }
}
