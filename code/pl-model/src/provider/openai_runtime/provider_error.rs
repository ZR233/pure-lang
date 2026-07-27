use async_openai::error::OpenAIError;
use pl_protocol::PureError;

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
        OpenAIError::Reqwest(error) => {
            PureError::HttpError(redact_secret_like_values(&error.to_string()))
        }
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

pub(super) fn retryable_provider_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 425 | 429 | 500..=599)
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
