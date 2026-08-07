use async_openai::error::OpenAIError;
use pl_protocol::PureError;
use std::error::Error as _;

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
        PureError::transient_model_failure(
            message,
            self.retry_after_ms,
            self.code.map(ToString::to_string),
            self.http_status,
        )
    }
}

pub(super) fn openai_error_to_pure(error: OpenAIError) -> PureError {
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
                PureError::LlmError(detail)
            }
        }
        OpenAIError::Reqwest(error) => reqwest_error_to_pure(error),
        OpenAIError::JSONDeserialize(error, content) => {
            PureError::HttpError(redact_secret_like_values(&format!("{error}: {content}")))
        }
        OpenAIError::StreamError(error) => {
            PureError::HttpError(redact_secret_like_values(&error.to_string()))
        }
        OpenAIError::InvalidArgument(message) => PureError::ConfigError(message),
        OpenAIError::FileSaveError(message) | OpenAIError::FileReadError(message) => {
            PureError::Io(std::io::Error::other(message))
        }
    }
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
    PureError::HttpError(detail)
}

fn response_start_connection_closed(error: &reqwest::Error) -> bool {
    if error.is_builder()
        || error.is_body()
        || error.is_decode()
        || error.is_redirect()
        || error.is_status()
    {
        return false;
    }
    let mut source = error.source();
    while let Some(cause) = source {
        if cause.downcast_ref::<std::io::Error>().is_some_and(|error| {
            matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
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
    matches!(status, 429 | 500..=599)
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
        assert!(matches!(error, PureError::LlmError(_)));
    }

    #[test]
    fn request_build_errors_remain_permanent() {
        let request_error = reqwest::Client::new()
            .get("://invalid-url")
            .build()
            .expect_err("invalid URL must fail while building the request");
        let error = openai_error_to_pure(OpenAIError::Reqwest(request_error));

        assert!(!error.is_transient_model_transport());
        assert!(matches!(error, PureError::HttpError(_)));
    }

    #[test]
    fn retryable_http_statuses_match_the_transport_policy() {
        for status in [429, 500, 503, 599] {
            assert!(retryable_provider_status(status), "status {status}");
        }
        for status in [400, 408, 409, 425] {
            assert!(!retryable_provider_status(status), "status {status}");
        }
    }

    fn api_error(status_code: StatusCode, code: Option<&str>) -> OpenAIError {
        OpenAIError::ApiError(ApiErrorResponse {
            status_code,
            api_error: ApiError {
                message: "Our servers are currently overloaded. Please try again later."
                    .to_string(),
                r#type: Some("server_error".to_string()),
                param: None,
                code: code.map(ToString::to_string),
            },
        })
    }
}
