//! Network execution only. The invocation owner decides whether an operation can be retried.
use eventsource_stream::Eventsource;
use futures::{StreamExt, stream::BoxStream};
use pl_protocol::{PureError, Result};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use std::collections::HashMap;

use super::openai::sse::SseStreamEvent;
use super::provider_error::provider_stream_failure;
pub(crate) use super::provider_error::reqwest_error_to_pure;

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
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(reqwest_error_to_pure)?;
        let remaining = 16_384_usize.saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if body.len() == 16_384 {
            break;
        }
    }
    Err(response_error(
        status,
        &headers,
        &body,
        pl_protocol::ProviderFailureStage::Request,
    ))
}

pub(crate) fn response_error(
    status: u16,
    headers: &HeaderMap,
    body: &[u8],
    stage: pl_protocol::ProviderFailureStage,
) -> PureError {
    let request_id = headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_owned();
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
        format!("HTTP {status}, request_id={request_id}: {message}"),
    );
    if let PureError::Provider(failure) = &mut error {
        failure.context.request_id = Some(super::provider_error::redact_secret_like_values(
            &request_id,
        ));
        if matches!(status, 400 | 404) && matches!(code, Some("file_not_found" | "file_expired")) {
            failure.context.recovery = pl_protocol::ProviderRecovery::RefreshAttachments;
        }
    }
    if let PureError::Provider(failure) = &mut error {
        failure.context.stage = stage;
    }
    error
}

pub(crate) async fn sse(
    request: reqwest::RequestBuilder,
) -> Result<BoxStream<'static, Result<SseStreamEvent>>> {
    let response = checked(request.send().await.map_err(reqwest_error_to_pure)?).await?;
    let stream = response.bytes_stream().eventsource();
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
                Err(eventsource_stream::EventStreamError::Transport(error)) => {
                    Some(Err(reqwest_error_to_pure(error)))
                }
                Err(error) => Some(Err(PureError::Protocol(format!(
                    "invalid SSE framing: {error}"
                )))),
            }
        })
        .boxed())
}
