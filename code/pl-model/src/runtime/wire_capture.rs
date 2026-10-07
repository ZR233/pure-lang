//! 验收专用的最终 provider wire 请求捕获。
//!
//! 只有测试/xtask harness 显式设置 `ANYWORK_WIRE_CAPTURE_DIR` 时写盘；
//! 默认生产路径不记录 prompt，也从不接触认证头。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use futures::stream::BoxStream;
use pl_protocol::{PureError, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use super::openai::OpenAiRequestBody;
use super::openai::sse::SseStreamEvent;
use crate::completion::CompletionTraceContext;

const CAPTURE_DIRECTORY_ENV: &str = "ANYWORK_WIRE_CAPTURE_DIR";
static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

static CAPTURE_RUN_ID: OnceLock<String> = OnceLock::new();
type CaptureRequestIds = BTreeMap<(String, String, String), u64>;
static REQUEST_IDS: OnceLock<Mutex<CaptureRequestIds>> = OnceLock::new();
fn run_id() -> &'static str {
    CAPTURE_RUN_ID.get_or_init(|| {
        format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    })
}
fn trace_key(trace: &CompletionTraceContext) -> (String, String, String) {
    (
        trace.session_id.clone(),
        trace.turn_id.clone(),
        trace.inference_id.clone(),
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireCapture<'a> {
    schema_version: u32,
    run_id: &'a str,
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
    run_id: &'a str,
    capture_id: u64,
    protocol: &'a str,
    stage: &'a str,
    elapsed_millis: u128,
    captured_at_unix_millis: u128,
}

/// Captures only recognized numeric usage fields, never provider content or headers.
pub(crate) async fn capture_usage(
    event: &SseStreamEvent,
    trace: Option<&CompletionTraceContext>,
) -> Result<()> {
    let Some(directory) = std::env::var_os(CAPTURE_DIRECTORY_ENV).map(PathBuf::from) else {
        return Ok(());
    };
    let response = event.response.as_ref();
    let response_usage = response
        .and_then(|response| response.get("usage"))
        .and_then(super::openai::usage::ProviderTokenUsage::from_value);
    let Some(usage) = event.usage.as_ref().or(response_usage.as_ref()) else {
        return Ok(());
    };
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct UsageReceipt<'a> {
        schema_version: u32,
        run_id: &'a str,
        request_capture_id: Option<u64>,
        session_id: Option<&'a str>,
        turn_id: Option<&'a str>,
        inference_id: Option<&'a str>,
        response_id: Option<&'a str>,
        model: Option<&'a str>,
        service_tier: Option<&'a str>,
        usage: &'a super::openai::usage::ProviderTokenUsage,
    }
    let receipt = UsageReceipt {
        schema_version: 2,
        run_id: run_id(),
        request_capture_id: trace.and_then(|trace| {
            REQUEST_IDS
                .get()?
                .lock()
                .ok()?
                .get(&trace_key(trace))
                .copied()
        }),
        session_id: trace.map(|trace| trace.session_id.as_str()),
        turn_id: trace.map(|trace| trace.turn_id.as_str()),
        inference_id: trace.map(|trace| trace.inference_id.as_str()),
        response_id: event
            .id
            .as_deref()
            .or_else(|| response.and_then(|r| r.get("id")?.as_str())),
        model: event
            .model
            .as_deref()
            .or_else(|| response.and_then(|r| r.get("model")?.as_str())),
        service_tier: response.and_then(|r| r.get("service_tier")?.as_str()),
        usage,
    };
    let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    tokio::fs::write(
        directory.join(format!("{}-usage-{sequence:06}.json", run_id())),
        bytes,
    )
    .await
    .map_err(|error| {
        PureError::ConfigError(format!("cannot write provider usage receipt: {error}"))
    })
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
    let path = directory.join(format!(
        "{}-{sequence:06}-{protocol}-{request_mode}.json",
        run_id()
    ));
    let context = HttpCaptureContext {
        capture_id: sequence,
        directory,
        protocol,
        started_at: Instant::now(),
    };
    if let Some(trace) = trace {
        REQUEST_IDS
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| PureError::ConfigError("wire capture identity lock poisoned".into()))?
            .insert(trace_key(trace), sequence);
    }
    let payload = serde_json::to_vec_pretty(&WireCapture {
        schema_version: 2,
        run_id: run_id(),
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
            "{}-{:06}-{}-{stage}.jsonl",
            run_id(),
            self.capture_id,
            self.protocol
        ));
        let mut payload = serde_json::to_vec(&TransportStageReceipt {
            schema_version: 2,
            run_id: run_id(),
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
