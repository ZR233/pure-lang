//! Network execution only. The invocation owner decides whether an operation can be retried.
mod sse_lines;

use eventsource_stream::Eventsource;
use futures::{StreamExt, stream::BoxStream};
use pl_protocol::{PureError, Result};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use std::collections::HashMap;

use super::openai::sse::SseStreamEvent;
pub(crate) use super::provider_error::reqwest_error_to_pure;
use super::provider_error::{provider_stream_failure, reqwest_body_error_to_pure};

pub(crate) fn headers(
    token: Option<&str>,
    provider: Option<&HashMap<String, String>>,
    model: &HashMap<String, String>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    if let Some(token) = token {
        let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| PureError::ConfigError("invalid authorization header".into()))?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
    }
    for (key, value) in provider.into_iter().flatten().chain(model) {
        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|_| PureError::ConfigError("invalid provider header name".into()))?;
        let mut value = HeaderValue::from_str(value)
            .map_err(|_| PureError::ConfigError("invalid provider header value".into()))?;
        value.set_sensitive(true);
        headers.insert(name, value);
    }
    Ok(headers)
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: Option<String>,
    message: Option<String>,
}

pub(crate) async fn checked(response: reqwest::Response) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    let mut body_failure = None;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                body_failure = Some(reqwest_body_error_to_pure(error));
                break;
            }
        };
        let remaining = 16_384_usize.saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if body.len() == 16_384 {
            break;
        }
    }
    let mut error = response_error(
        status,
        &headers,
        &body,
        pl_protocol::ProviderFailureStage::Request,
    );
    // The status and retry headers already describe the failed request. A
    // damaged error body must not turn authentication failures into retries.
    if let (Some(body_failure), PureError::Provider(failure)) = (body_failure, &mut error) {
        failure.message.push_str(&format!("; {body_failure}"));
    }
    Err(error)
}

pub(crate) fn response_error(
    status: u16,
    headers: &HeaderMap,
    body: &[u8],
    stage: pl_protocol::ProviderFailureStage,
) -> PureError {
    let request_id = response_request_id(headers);
    let retry_after = headers
        .get("retry-after-ms")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| {
            headers
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(|seconds| seconds.saturating_mul(1000))
        });
    let parsed = serde_json::from_slice::<ErrorEnvelope>(body).ok();
    let code = parsed
        .as_ref()
        .and_then(|value| value.error.code.as_deref());
    let message = parsed
        .as_ref()
        .and_then(|value| value.error.message.as_deref())
        .unwrap_or("provider returned an unstructured error response");
    let mut error = provider_stream_failure(
        code,
        Some(status),
        retry_after,
        format!(
            "HTTP {status}, request_id={}: {message}",
            request_id.as_deref().unwrap_or("unknown")
        ),
    );
    if let PureError::Provider(failure) = &mut error {
        failure.context.request_id = request_id;
        if matches!(status, 400 | 404) && matches!(code, Some("file_not_found" | "file_expired")) {
            failure.context.recovery = pl_protocol::ProviderRecovery::RefreshAttachments;
        }
    }
    if let PureError::Provider(failure) = &mut error {
        failure.context.stage = stage;
    }
    error
}

fn response_request_id(headers: &HeaderMap) -> Option<String> {
    let value = headers.get("x-request-id")?.to_str().ok()?;
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
    {
        return None;
    }
    Some(super::provider_error::redact_secret_like_values(value))
}

pub(crate) async fn sse(
    request: reqwest::RequestBuilder,
) -> Result<BoxStream<'static, Result<SseStreamEvent>>> {
    let response = checked(request.send().await.map_err(reqwest_error_to_pure)?).await?;
    let version = response.version();
    let status = response.status().as_u16();
    let request_id = response_request_id(response.headers());
    let started = std::time::Instant::now();
    let mut received_bytes = 0_u64;
    let body = response.bytes_stream().map(move |chunk| match chunk {
        Ok(chunk) => {
            received_bytes = received_bytes.saturating_add(chunk.len() as u64);
            Ok(chunk)
        }
        Err(error) => {
            let mut error = reqwest_body_error_to_pure(error);
            if let PureError::Provider(failure) = &mut error {
                failure.context.stage = pl_protocol::ProviderFailureStage::Stream;
                failure.context.request_id = request_id.clone();
                failure.http_status = Some(status);
                failure.message.push_str(&format!(
                    "; version={version:?}; received_bytes={received_bytes}; read_elapsed_ms={}; request_id={}",
                    started.elapsed().as_millis(),
                    request_id.as_deref().unwrap_or("unknown"),
                ));
            }
            Err(error)
        }
    });
    let stream = sse_lines::complete_lines(body).eventsource();
    Ok(stream
        .take_while(|event| {
            futures::future::ready(
                !event
                    .as_ref()
                    .is_ok_and(|event| event.data.trim() == "[DONE]"),
            )
        })
        .filter_map(|event| async move {
            match event {
                Ok(event) if event.data.trim().is_empty() => None,
                Ok(event) => Some(
                    serde_json::from_str(&event.data)
                        .map_err(|error| PureError::Protocol(format!("invalid SSE JSON: {error}"))),
                ),
                Err(eventsource_stream::EventStreamError::Transport(error)) => Some(Err(error)),
                Err(error) => Some(Err(PureError::Protocol(format!(
                    "invalid SSE framing: {error}"
                )))),
            }
        })
        .boxed())
}
