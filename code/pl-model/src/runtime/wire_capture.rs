//! 验收专用的最终 provider wire 请求捕获。
//!
//! 只有测试/xtask harness 显式设置 `PURE_STUDIO_WIRE_CAPTURE_DIR` 时写盘；
//! 默认生产路径不记录 prompt，也从不接触认证头。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use futures::stream::BoxStream;
use pl_protocol::{PureError, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use super::openai::OpenAiRequestBody;
use super::openai::sse::SseStreamEvent;
use crate::completion::CompletionTraceContext;

const CAPTURE_DIRECTORY_ENV: &str = "PURE_STUDIO_WIRE_CAPTURE_DIR";
static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireCapture<'a> {
    schema_version: u32,
    capture_id: u64,
    captured_at_unix_millis: u128,
    protocol: &'a str,
    request_mode: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inference_id: Option<&'a str>,
    wire_body: &'a Value,
}

#[derive(Debug, Clone)]
pub(super) struct HttpCaptureContext {
    capture_id: u64,
    directory: PathBuf,
    protocol: &'static str,
    started_at: Instant,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransportStageReceipt<'a> {
    schema_version: u32,
    capture_id: u64,
    protocol: &'a str,
    stage: &'a str,
    elapsed_millis: u128,
    captured_at_unix_millis: u128,
}

pub(super) async fn capture_http(
    body: &OpenAiRequestBody,
    trace: Option<&CompletionTraceContext>,
) -> Result<Option<HttpCaptureContext>> {
    let (protocol, wire_body) = match body {
        OpenAiRequestBody::Responses(body) => ("responsesHttp", Value::Object(body.clone())),
        OpenAiRequestBody::Chat(body) => ("chatCompletions", Value::Object(body.clone())),
    };
    capture(protocol, "full", &wire_body, trace).await
}

pub(super) async fn capture_responses_websocket(
    request_mode: &'static str,
    wire_body: &Map<String, Value>,
    trace: Option<&CompletionTraceContext>,
) -> Result<()> {
    capture(
        "responsesWebSocket",
        request_mode,
        &Value::Object(wire_body.clone()),
        trace,
    )
    .await
    .map(|_| ())
}

async fn capture(
    protocol: &'static str,
    request_mode: &str,
    wire_body: &Value,
    trace: Option<&CompletionTraceContext>,
) -> Result<Option<HttpCaptureContext>> {
    let Some(directory) = std::env::var_os(CAPTURE_DIRECTORY_ENV).map(PathBuf::from) else {
        return Ok(None);
    };
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| {
            PureError::ConfigError(format!(
                "failed to create wire capture directory `{}`: {error}",
                directory.display()
            ))
        })?;
    let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let path = directory.join(format!("{sequence:06}-{protocol}-{request_mode}.json"));
    let context = HttpCaptureContext {
        capture_id: sequence,
        directory,
        protocol,
        started_at: Instant::now(),
    };
    let payload = serde_json::to_vec_pretty(&WireCapture {
        schema_version: 1,
        capture_id: sequence,
        captured_at_unix_millis: unix_millis(),
        protocol,
        request_mode,
        session_id: trace.map(|trace| trace.session_id.as_str()),
        turn_id: trace.map(|trace| trace.turn_id.as_str()),
        inference_id: trace.map(|trace| trace.inference_id.as_str()),
        wire_body,
    })
    .map_err(|error| PureError::ConfigError(format!("failed to encode wire capture: {error}")))?;
    tokio::fs::write(&path, payload).await.map_err(|error| {
        PureError::ConfigError(format!(
            "failed to write wire capture `{}`: {error}",
            path.display()
        ))
    })?;
    context.record_stage("requestCaptured").await?;
    Ok(Some(context))
}

impl HttpCaptureContext {
    pub(super) async fn record_stage(&self, stage: &'static str) -> Result<()> {
        let path = self.directory.join(format!(
            "{:06}-{}-{stage}.jsonl",
            self.capture_id, self.protocol
        ));
        let mut payload = serde_json::to_vec(&TransportStageReceipt {
            schema_version: 1,
            capture_id: self.capture_id,
            protocol: self.protocol,
            stage,
            elapsed_millis: self.started_at.elapsed().as_millis(),
            captured_at_unix_millis: unix_millis(),
        })
        .map_err(|error| {
            PureError::ConfigError(format!("failed to encode transport stage: {error}"))
        })?;
        payload.push(b'\n');
        tokio::fs::write(&path, payload).await.map_err(|error| {
            PureError::ConfigError(format!(
                "failed to write transport stage `{}`: {error}",
                path.display()
            ))
        })
    }
}

pub(super) fn observe_http_stream(
    stream: BoxStream<'static, Result<SseStreamEvent>>,
    capture: Option<HttpCaptureContext>,
) -> BoxStream<'static, Result<SseStreamEvent>> {
    let state = ObservedHttpStream {
        stream,
        capture,
        saw_provider_event: false,
        terminated: false,
    };
    futures::stream::unfold(state, |mut state| async move {
        if state.terminated {
            return None;
        }
        match state.stream.next().await {
            Some(Ok(event)) => {
                if !state.saw_provider_event {
                    state.saw_provider_event = true;
                    if let Some(capture) = &state.capture
                        && let Err(error) = capture.record_stage("firstProviderEvent").await
                    {
                        state.terminated = true;
                        return Some((Err(error), state));
                    }
                }
                Some((Ok(event), state))
            }
            Some(Err(error)) => {
                state.terminated = true;
                if let Some(capture) = &state.capture
                    && let Err(capture_error) = capture.record_stage("providerStreamFailed").await
                {
                    return Some((Err(capture_error), state));
                }
                Some((Err(error), state))
            }
            None => {
                if let Some(capture) = &state.capture
                    && let Err(error) = capture.record_stage("providerStreamEnded").await
                {
                    state.terminated = true;
                    return Some((Err(error), state));
                }
                None
            }
        }
    })
    .boxed()
}

struct ObservedHttpStream {
    stream: BoxStream<'static, Result<SseStreamEvent>>,
    capture: Option<HttpCaptureContext>,
    saw_provider_event: bool,
    terminated: bool,
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_capture_serializes_trace_identity_outside_provider_body() {
        let body = serde_json::json!({"model":"fixture"});
        let trace = CompletionTraceContext {
            session_id: "session-a".into(),
            turn_id: "turn-a".into(),
            inference_id: "inference-a".into(),
        };
        let capture = WireCapture {
            schema_version: 1,
            capture_id: 7,
            captured_at_unix_millis: 9,
            protocol: "responsesHttp",
            request_mode: "full",
            session_id: Some(&trace.session_id),
            turn_id: Some(&trace.turn_id),
            inference_id: Some(&trace.inference_id),
            wire_body: &body,
        };

        assert_eq!(
            serde_json::to_value(capture).unwrap(),
            serde_json::json!({
                "schemaVersion": 1,
                "captureId": 7,
                "capturedAtUnixMillis": 9,
                "protocol": "responsesHttp",
                "requestMode": "full",
                "sessionId": "session-a",
                "turnId": "turn-a",
                "inferenceId": "inference-a",
                "wireBody": {"model":"fixture"},
            })
        );
    }
}
