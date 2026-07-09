pub(crate) fn is_provider_429_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    contains_standalone_status_code(&lower, "429")
}

/// 判断模型 provider 错误是否属于适合重试的瞬态错误。
///
/// 该 helper 只覆盖模型请求/流式传输层面的通用错误；宿主产品自己的 relay、
/// 队列、业务状态错误应在产品层继续单独判断。
pub fn is_retryable_model_error(error: &str) -> bool {
    let error = error.trim();
    if let Some(stream_error) = error.strip_prefix("model error: stream error: ") {
        return is_retryable_model_stream_error(stream_error);
    }

    if !error.starts_with("model error: request to ") {
        return false;
    }

    error.contains(" failed: error decoding response body")
        || contains_standalone_status_code(error, "429")
        || contains_standalone_status_code(error, "500")
        || contains_standalone_status_code(error, "502")
        || contains_standalone_status_code(error, "503")
        || contains_standalone_status_code(error, "504")
}

fn is_retryable_model_stream_error(error: &str) -> bool {
    error.contains("stream closed with incomplete SSE frame")
        || error.contains("stream closed before response.completed")
        || error.contains("idle timeout waiting for SSE")
        || (error.contains("response.incomplete event received")
            && error.contains("max_output_tokens"))
}

fn contains_standalone_status_code(text: &str, code: &str) -> bool {
    text.match_indices(code).any(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + code.len()..].chars().next();
        !before.is_some_and(|ch| ch.is_ascii_digit())
            && !after.is_some_and(|ch| ch.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_provider_429_error_codes() {
        assert!(is_provider_429_error(
            "API error 429 Too Many Requests: concurrency limit reached"
        ));
        assert!(is_provider_429_error("provider returned status 429"));
        assert!(is_provider_429_error("429 Too Many Requests"));
        assert!(!is_provider_429_error("Too Many Requests"));
        assert!(!is_provider_429_error(
            "API error 500 internal server error"
        ));
        assert!(!is_provider_429_error("local tool failed with code 1429"));
    }

    #[test]
    fn retryable_model_error_detects_transient_http_and_stream_failures() {
        for error in [
            "model error: request to https://example.test/v1/chat/completions returned 429 Too Many Requests",
            "model error: request to https://example.test/v1/chat/completions returned 500 Internal Server Error",
            "model error: request to https://example.test/v1/chat/completions returned 502 Bad Gateway",
            "model error: request to https://example.test/v1/chat/completions failed: error decoding response body",
            "model error: stream error: stream closed with incomplete SSE frame",
            "model error: stream error: stream closed before response.completed",
            "model error: stream error: idle timeout waiting for SSE",
            "model error: stream error: response.incomplete event received: max_output_tokens",
        ] {
            assert!(is_retryable_model_error(error), "{error}");
        }
    }

    #[test]
    fn retryable_model_error_rejects_bad_requests_and_unrelated_errors() {
        for error in [
            "model error: request to https://example.test/v1/chat/completions returned 400 Bad Request",
            "model error: stream error: response.incomplete event received: content_filter",
            "relay request timed out",
            "local tool failed with code 1500",
        ] {
            assert!(!is_retryable_model_error(error), "{error}");
        }
    }
}
