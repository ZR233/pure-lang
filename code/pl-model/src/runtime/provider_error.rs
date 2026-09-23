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
            context: Default::default(),
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
            context: Default::default(),
            kind: provider_failure_kind(self.code, self.http_status, false),
            code: self.code.map(ToString::to_string),
            http_status: self.http_status,
            message,
            retry: RetryDisposition::Permanent,
        })
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
    let mut error = if metadata.is_retryable() {
        metadata.into_transient(message)
    } else {
        metadata.into_permanent(message)
    };
    if let PureError::Provider(failure) = &mut error {
        failure.context.stage = pl_protocol::ProviderFailureStage::Stream;
    }
    error
}

pub(crate) fn reqwest_error_to_pure(error: reqwest::Error) -> PureError {
    let error = error.without_url();
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
            context: Default::default(),
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
        context: Default::default(),
        kind: ProviderFailureKind::Configuration,
        code: None,
        http_status: None,
        message: message.into(),
        retry: RetryDisposition::Permanent,
    })
}

fn protocol_failure(message: impl Into<String>) -> PureError {
    PureError::provider_failure(ProviderFailure {
        context: Default::default(),
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
