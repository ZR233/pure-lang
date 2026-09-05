use std::collections::HashMap;

use futures::StreamExt;
use pl_protocol::{PureError, Result};
use serde_json::{Map, Value};
use tokio::sync::OwnedMutexGuard;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};

use crate::completion::CompletionTraceContext;
use crate::completion::stream::OpenAiRawEventStream;
use crate::provider::RESPONSES_WEBSOCKET_DIALECT;
use crate::runtime::ModelSession;
use crate::runtime::openai::sse::SseStreamEvent;
use crate::runtime::session::{ResponsesWebSocketConnection, ResponsesWebSocketSession};
use crate::runtime::transport_policy::{
    RESPONSES_WEBSOCKET_CONNECT_TIMEOUT, RESPONSES_WEBSOCKET_IDLE_TIMEOUT,
    RESPONSES_WEBSOCKET_SEND_TIMEOUT,
};

mod dialer;
pub(crate) mod error;
mod state;

use error::{
    close_error, connection_error, continuation_id_invalid, continuation_retry_error,
    handshake_error, handshake_timeout_error, protocol_error, response_terminal_error,
    server_error,
};
use state::{
    ClosedResponsesStream, CompletedResponsesStream, FailedResponsesStream, ResponsesStreamState,
};

pub(super) struct StreamResponsesInput<'a> {
    pub api_base: String,
    pub token: Option<String>,
    pub provider_headers: Option<&'a HashMap<String, String>>,
    pub model_headers: &'a HashMap<String, String>,
    pub connection_key: u64,
    pub model_session: ModelSession,
    pub body: Map<String, Value>,
    pub trace: Option<CompletionTraceContext>,
}

pub(super) async fn stream_responses(
    input: StreamResponsesInput<'_>,
) -> Result<OpenAiRawEventStream> {
    let StreamResponsesInput {
        api_base,
        token,
        provider_headers,
        model_headers,
        connection_key,
        model_session,
        mut body,
        trace,
    } = input;
    normalize_websocket_request_body(&mut body);

    let mut guard = model_session.lock_responses_websocket().await;
    if guard.connection_key != Some(connection_key) || guard.connection.is_none() {
        guard.invalidate();
        guard.connection =
            Some(connect(&api_base, token.as_deref(), provider_headers, model_headers).await?);
        guard.connection_key = Some(connection_key);
    }

    let (mut wire_body, used_continuation, fallback_reason) =
        match incremental_request(&guard, &body) {
            Ok(incremental) => (incremental, true, None),
            Err(reason) => (body.clone(), false, Some(reason)),
        };
    if used_continuation {
        model_session.record_continuation_attempt();
    }
    let input = body.get("input");
    let input_items = input.and_then(Value::as_array).map_or(0, Vec::len);
    let context_bytes = input
        .and_then(|input| serde_json::to_vec(input).ok())
        .map_or(0, |input| input.len());
    let wire_input_items = wire_body
        .get("input")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let (previous_prefix_items, first_differing_index) = fallback_reason
        .and_then(IncrementalRequestFallbackReason::prefix_mismatch)
        .map_or((0, 0), |diagnostics| {
            (
                diagnostics.previous_prefix_items,
                diagnostics.first_differing_index,
            )
        });
    let request_mode = if used_continuation {
        "incremental"
    } else {
        "full"
    };
    let request_text = response_create_text(&mut wire_body)?;
    super::wire_capture::capture_responses_websocket(request_mode, &wire_body, trace.as_ref())
        .await?;
    tracing::debug!(
        request_mode,
        fallback_reason = fallback_reason.map_or("none", IncrementalRequestFallbackReason::as_str),
        input_items,
        wire_input_items,
        context_bytes,
        request_bytes = request_text.len(),
        previous_prefix_items,
        first_differing_index,
        "Responses WebSocket request prepared"
    );
    if let Err(error) = send_request(&mut guard, &request_text).await {
        guard.invalidate();
        return Err(error);
    }

    let state = WebSocketEventState {
        guard,
        state: ResponsesStreamState::open(),
        used_continuation,
        events_emitted: false,
        full_request: body,
        model_session,
    };
    Ok(futures::stream::unfold(state, |mut state| async move {
        match &state.state {
            ResponsesStreamState::Open(_) => {}
            ResponsesStreamState::Completed(_) => {
                state.finish_completed_response();
                return None;
            }
            ResponsesStreamState::Failed(failed) => {
                tracing::debug!(
                    detail = failed.detail(),
                    "Responses WebSocket stream stopped"
                );
                return None;
            }
            ResponsesStreamState::Closed(_) => return None,
        }
        let event = state.next_event().await;
        Some((event, state))
    })
    .boxed())
}

fn normalize_websocket_request_body(body: &mut Map<String, Value>) {
    body.remove("previous_response_id");
    // WS continuation 只属于当前物理连接；供应商会拒绝把该响应持久化到服务端。
    body.insert("store".to_string(), Value::Bool(false));
    // Responses WebSocket v2 的 `response.create` schema 要求显式携带 tools；
    // HTTP Responses 接受省略空工具列表，但兼容 WS 代理可能直接关闭连接。
    body.entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
}

fn response_create_text(body: &mut Map<String, Value>) -> Result<String> {
    body.insert(
        "type".to_string(),
        Value::String("response.create".to_string()),
    );
    serde_json::to_string(body).map_err(|error| PureError::ConfigError(error.to_string()))
}

fn incremental_request(
    session: &ResponsesWebSocketSession,
    current: &Map<String, Value>,
) -> std::result::Result<Map<String, Value>, IncrementalRequestFallbackReason> {
    let previous = session
        .last_request
        .as_ref()
        .ok_or(IncrementalRequestFallbackReason::MissingPreviousRequest)?;
    let response_id = session
        .last_response_id
        .as_deref()
        .ok_or(IncrementalRequestFallbackReason::MissingPreviousResponseId)?;
    if request_properties(previous) != request_properties(current) {
        return Err(IncrementalRequestFallbackReason::RequestPropertiesChanged);
    }
    let mut previous_items = previous
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .ok_or(IncrementalRequestFallbackReason::PreviousInputNotArray)?;
    previous_items.extend(session.last_response_items.iter().cloned());
    let current_items = current
        .get("input")
        .and_then(Value::as_array)
        .ok_or(IncrementalRequestFallbackReason::CurrentInputNotArray)?;
    if !current_items.starts_with(&previous_items) {
        let first_differing_index = previous_items
            .iter()
            .zip(current_items)
            .position(|(previous, current)| previous != current)
            .unwrap_or_else(|| previous_items.len().min(current_items.len()));
        return Err(IncrementalRequestFallbackReason::InputPrefixMismatch {
            previous_prefix_items: previous_items.len(),
            first_differing_index,
        });
    }
    let mut incremental = current.clone();
    incremental.insert(
        "input".to_string(),
        Value::Array(current_items[previous_items.len()..].to_vec()),
    );
    incremental.insert(
        "previous_response_id".to_string(),
        Value::String(response_id.to_string()),
    );
    Ok(incremental)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncrementalRequestFallbackReason {
    MissingPreviousRequest,
    MissingPreviousResponseId,
    RequestPropertiesChanged,
    PreviousInputNotArray,
    CurrentInputNotArray,
    InputPrefixMismatch {
        previous_prefix_items: usize,
        first_differing_index: usize,
    },
}

impl IncrementalRequestFallbackReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::MissingPreviousRequest => "missingPreviousRequest",
            Self::MissingPreviousResponseId => "missingPreviousResponseId",
            Self::RequestPropertiesChanged => "requestPropertiesChanged",
            Self::PreviousInputNotArray => "previousInputNotArray",
            Self::CurrentInputNotArray => "currentInputNotArray",
            Self::InputPrefixMismatch { .. } => "inputPrefixMismatch",
        }
    }

    const fn prefix_mismatch(self) -> Option<InputPrefixMismatchDiagnostics> {
        match self {
            Self::InputPrefixMismatch {
                previous_prefix_items,
                first_differing_index,
            } => Some(InputPrefixMismatchDiagnostics {
                previous_prefix_items,
                first_differing_index,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputPrefixMismatchDiagnostics {
    previous_prefix_items: usize,
    first_differing_index: usize,
}

fn request_properties(body: &Map<String, Value>) -> Map<String, Value> {
    let mut properties = body.clone();
    properties.remove("input");
    properties.remove("previous_response_id");
    properties.remove("type");
    properties
}

async fn connect(
    api_base: &str,
    token: Option<&str>,
    provider_headers: Option<&HashMap<String, String>>,
    model_headers: &HashMap<String, String>,
) -> Result<ResponsesWebSocketConnection> {
    let url = responses_websocket_url(api_base)?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|error| PureError::HttpError(error.to_string()))?;
    request.headers_mut().insert(
        HeaderName::from_static("openai-beta"),
        HeaderValue::from_static(RESPONSES_WEBSOCKET_DIALECT),
    );
    if let Some(token) = token {
        let value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|error| PureError::HttpError(error.to_string()))?;
        request.headers_mut().insert(AUTHORIZATION, value);
    }
    if let Some(headers) = provider_headers {
        insert_headers(request.headers_mut(), headers)?;
    }
    insert_headers(request.headers_mut(), model_headers)?;

    let (connection, _) = timeout(
        RESPONSES_WEBSOCKET_CONNECT_TIMEOUT,
        dialer::connect(request, &url),
    )
    .await
    .map_err(|_| handshake_timeout_error())?
    .map_err(handshake_error)?;
    Ok(ResponsesWebSocketConnection::new(connection))
}

fn responses_websocket_url(api_base: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(api_base.trim_end_matches('/'))
        .map_err(|error| PureError::ConfigError(format!("invalid provider base URL: {error}")))?;
    let websocket_scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        "wss" => "wss",
        "ws" => "ws",
        scheme => {
            return Err(PureError::ConfigError(format!(
                "unsupported provider URL scheme for WebSocket: {scheme}"
            )));
        }
    };
    url.set_scheme(websocket_scheme).map_err(|()| {
        PureError::ConfigError("failed to construct Responses WebSocket URL".to_string())
    })?;
    let path = format!("{}/responses", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

fn insert_headers(
    target: &mut tokio_tungstenite::tungstenite::http::HeaderMap,
    headers: &HashMap<String, String>,
) -> Result<()> {
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| PureError::HttpError(error.to_string()))?;
        let value = HeaderValue::from_str(value)
            .map_err(|error| PureError::HttpError(error.to_string()))?;
        target.insert(name, value);
    }
    Ok(())
}

async fn send_request(
    session: &mut OwnedMutexGuard<ResponsesWebSocketSession>,
    request_text: &str,
) -> Result<()> {
    let connection = session
        .connection
        .as_mut()
        .ok_or_else(|| connection_error("connection is unavailable"))?;
    timeout(
        RESPONSES_WEBSOCKET_SEND_TIMEOUT,
        connection.send(Message::Text(request_text.to_string().into())),
    )
    .await
    .map_err(|_| {
        PureError::transient_model_transport("Responses WebSocket send timed out after 15 seconds")
    })?
}

struct WebSocketEventState {
    guard: OwnedMutexGuard<ResponsesWebSocketSession>,
    state: ResponsesStreamState,
    used_continuation: bool,
    events_emitted: bool,
    full_request: Map<String, Value>,
    model_session: ModelSession,
}

impl WebSocketEventState {
    async fn next_event(&mut self) -> Result<SseStreamEvent> {
        loop {
            let next = {
                let connection = self
                    .guard
                    .connection
                    .as_mut()
                    .ok_or_else(|| connection_error("connection is unavailable"))?;
                timeout(RESPONSES_WEBSOCKET_IDLE_TIMEOUT, connection.next()).await
            };
            let message = match next {
                Ok(Some(Ok(message))) => message,
                Ok(Some(Err(error))) => return Err(self.invalidate_with_connection_error(error)),
                Ok(None) => {
                    return Err(self.invalidate_with_connection_error(connection_error(
                        "connection closed before a terminal response event",
                    )));
                }
                Err(_) => {
                    return Err(self.invalidate_with_connection_error(connection_error(
                        "idle timeout waiting for a response event",
                    )));
                }
            };
            match message {
                Message::Text(text) => {
                    let value: Value = match serde_json::from_str(text.as_str()) {
                        Ok(value) => value,
                        Err(error) => {
                            let error = protocol_error(format!("invalid JSON event: {error}"));
                            return Err(self.fail(error));
                        }
                    };
                    if value.get("type").and_then(Value::as_str) == Some("error") {
                        if !self.events_emitted
                            && self.used_continuation
                            && continuation_id_invalid(&value)
                        {
                            self.model_session.record_continuation_invalid();
                            return Err(self.fail(continuation_retry_error()));
                        }
                        let error = server_error(&value);
                        return Err(self.fail(error));
                    }
                    if matches!(
                        value.get("type").and_then(Value::as_str),
                        Some("response.failed" | "response.incomplete")
                    ) && let Some(error) = response_terminal_error(&value)
                    {
                        return Err(self.fail(error));
                    }
                    let event: SseStreamEvent = match serde_json::from_value(value) {
                        Ok(event) => event,
                        Err(error) => {
                            let error = protocol_error(error.to_string());
                            return Err(self.fail(error));
                        }
                    };
                    match event.kind.as_str() {
                        "response.completed" => {
                            self.state = ResponsesStreamState::Completed(Box::new(
                                CompletedResponsesStream::new(event.clone()),
                            ));
                        }
                        "response.failed" | "response.incomplete" => {
                            self.guard.invalidate();
                            self.state = ResponsesStreamState::Failed(FailedResponsesStream::new(
                                "provider returned terminal response",
                            ));
                        }
                        _ => {}
                    }
                    self.events_emitted = true;
                    return Ok(event);
                }
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
                Message::Binary(_) => {
                    let error = protocol_error("unexpected binary event");
                    return Err(self.fail(error));
                }
                Message::Close(frame) => {
                    let error = close_error(frame);
                    return Err(self.fail(error));
                }
            }
        }
    }

    fn invalidate_with_connection_error(&mut self, error: PureError) -> PureError {
        self.fail(error)
    }

    fn fail(&mut self, error: PureError) -> PureError {
        self.guard.invalidate();
        self.state = ResponsesStreamState::Failed(FailedResponsesStream::new(error.to_string()));
        error
    }

    fn commit_completed_response(&mut self, event: &SseStreamEvent) {
        let response = event.response.as_ref();
        let response_id = response
            .and_then(|response| response.get("id"))
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(ToString::to_string);
        let response_items = response
            .and_then(|response| response.get("output"))
            .and_then(Value::as_array)
            .map(|items| canonical_response_history_items(items))
            .unwrap_or_default();
        if response_id.is_some() {
            self.guard.last_request = Some(self.full_request.clone());
            self.guard.last_response_id = response_id;
            self.guard.last_response_items = response_items;
        } else {
            self.guard.last_request = None;
            self.guard.last_response_id = None;
            self.guard.last_response_items.clear();
        }
    }

    fn finish_completed_response(&mut self) {
        let ResponsesStreamState::Completed(completed) = &self.state else {
            return;
        };
        let event = completed.event().clone();
        self.commit_completed_response(&event);
        if self.used_continuation {
            self.model_session.record_continuation_used();
        }
        self.state = ResponsesStreamState::Closed(ClosedResponsesStream::new());
    }
}

fn canonical_response_history_items(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .filter_map(|item| {
            let object = item.as_object()?;
            match object.get("type").and_then(Value::as_str)? {
                "message" => {
                    let content = object
                        .get("content")
                        .and_then(Value::as_array)?
                        .iter()
                        .filter_map(|part| {
                            let part = part.as_object()?;
                            let kind = part.get("type").and_then(Value::as_str)?;
                            let text = part.get("text").and_then(Value::as_str)?;
                            matches!(kind, "output_text" | "input_text")
                                .then(|| serde_json::json!({ "type": "output_text", "text": text }))
                        })
                        .collect::<Vec<_>>();
                    Some(serde_json::json!({
                        "type": "message",
                        "role": object
                            .get("role")
                            .and_then(Value::as_str)
                            .unwrap_or("assistant"),
                        "content": content,
                    }))
                }
                "function_call" => Some(serde_json::json!({
                    "type": "function_call",
                    "name": object.get("name")?,
                    "arguments": object.get("arguments")?,
                    "call_id": object.get("call_id")?,
                })),
                "custom_tool_call" => Some(serde_json::json!({
                    "type": "custom_tool_call",
                    "name": object.get("name")?,
                    "input": object.get("input")?,
                    "call_id": object.get("call_id")?,
                })),
                "reasoning" | "compaction" | "web_search_call" => None,
                _ => None,
            }
        })
        .collect()
}

impl Drop for WebSocketEventState {
    fn drop(&mut self) {
        if !matches!(self.state, ResponsesStreamState::Closed(_)) {
            self.guard.invalidate();
        }
    }
}
#[cfg(test)]
mod request_tests {
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value};

    use super::{
        IncrementalRequestFallbackReason, canonical_response_history_items, incremental_request,
        normalize_websocket_request_body, responses_websocket_url,
    };
    use crate::runtime::session::ResponsesWebSocketSession;

    #[test]
    fn websocket_request_keeps_explicit_empty_tools_for_v2_schema() {
        let mut body = Map::from_iter([
            (
                "previous_response_id".to_string(),
                serde_json::json!("stale"),
            ),
            ("store".to_string(), Value::Bool(true)),
        ]);

        normalize_websocket_request_body(&mut body);

        assert_eq!(body["tools"], serde_json::json!([]));
        assert_eq!(body["store"], Value::Bool(false));
        assert!(!body.contains_key("previous_response_id"));
    }

    #[test]
    fn builds_responses_websocket_url_without_losing_base_path() {
        assert_eq!(
            responses_websocket_url("https://api.openai.com/v1/")
                .unwrap()
                .as_str(),
            "wss://api.openai.com/v1/responses"
        );
        assert_eq!(
            responses_websocket_url("http://127.0.0.1:8080/proxy/v1")
                .unwrap()
                .as_str(),
            "ws://127.0.0.1:8080/proxy/v1/responses"
        );
    }

    #[test]
    fn canonical_history_ignores_reasoning_and_provider_owned_fields() {
        let output = vec![
            serde_json::json!({ "type": "reasoning", "id": "reasoning-1" }),
            serde_json::json!({
                "type": "message",
                "id": "message-1",
                "status": "completed",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "ok",
                    "annotations": [],
                }],
            }),
        ];

        assert_eq!(
            canonical_response_history_items(&output),
            vec![serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "ok" }],
            })]
        );
    }

    #[test]
    fn incremental_request_sends_only_the_strict_suffix() {
        let session = ResponsesWebSocketSession {
            last_request: Some(Map::from_iter([
                ("model".to_string(), serde_json::json!("gpt-test")),
                (
                    "input".to_string(),
                    serde_json::json!([{"role":"user","content":"a"}]),
                ),
            ])),
            last_response_id: Some("response-1".to_string()),
            last_response_items: vec![serde_json::json!({"role":"assistant","content":"b"})],
            ..ResponsesWebSocketSession::default()
        };
        let current = Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            (
                "input".to_string(),
                serde_json::json!([
                    {"role":"user","content":"a"},
                    {"role":"assistant","content":"b"},
                    {"role":"user","content":"c"}
                ]),
            ),
        ]);

        let incremental = incremental_request(&session, &current).unwrap();
        assert_eq!(
            incremental["input"],
            serde_json::json!([{"role":"user","content":"c"}])
        );
        assert_eq!(incremental["previous_response_id"], "response-1");
    }

    #[test]
    fn incremental_request_reports_prefix_mismatch() {
        let session = ResponsesWebSocketSession {
            last_request: Some(Map::from_iter([
                ("model".to_string(), serde_json::json!("gpt-test")),
                ("input".to_string(), serde_json::json!(["old-tail"])),
            ])),
            last_response_id: Some("response-1".to_string()),
            ..ResponsesWebSocketSession::default()
        };
        let current = Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            ("input".to_string(), serde_json::json!(["new-tail"])),
        ]);

        assert_eq!(
            incremental_request(&session, &current).unwrap_err(),
            IncrementalRequestFallbackReason::InputPrefixMismatch {
                previous_prefix_items: 1,
                first_differing_index: 0,
            }
        );
    }

    #[test]
    fn continuation_reuses_when_request_tools_unchanged_and_native_context_appended() {
        let tools = serde_json::json!([
            {"type": "function", "name": "exec"},
            {"type": "programmatic_tool_calling"}
        ]);
        let user = serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "list the git tools"}],
        });
        let program = serde_json::json!({
            "type": "program",
            "id": "program-1",
        });
        let program_output = serde_json::json!({
            "type": "program_output",
            "id": "program-output-1",
        });
        let next_user = serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "run git status"}],
        });
        let session = ResponsesWebSocketSession {
            last_request: Some(Map::from_iter([
                ("model".to_string(), serde_json::json!("gpt-test")),
                ("tools".to_string(), tools.clone()),
                ("input".to_string(), serde_json::json!([user.clone()])),
            ])),
            last_response_id: Some("response-1".to_string()),
            last_response_items: vec![program.clone(), program_output.clone()],
            ..ResponsesWebSocketSession::default()
        };
        let current = Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            ("tools".to_string(), tools),
            (
                "input".to_string(),
                serde_json::json!([user, program, program_output, next_user,]),
            ),
        ]);

        let incremental = incremental_request(&session, &current).unwrap();
        assert_eq!(
            incremental["input"],
            serde_json::json!([next_user]),
            "request tools 未变时，session 上下文追加的原生 item 之后 continuation 只发送严格后缀"
        );
        assert_eq!(incremental["previous_response_id"], "response-1");
    }

    #[test]
    fn continuation_falls_back_when_request_tools_change() {
        let user = serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "continue"}],
        });
        let session = ResponsesWebSocketSession {
            last_request: Some(Map::from_iter([
                ("model".to_string(), serde_json::json!("gpt-test")),
                (
                    "tools".to_string(),
                    serde_json::json!([
                        {"type": "function", "name": "exec"}
                    ]),
                ),
                ("input".to_string(), serde_json::json!([user.clone()])),
            ])),
            last_response_id: Some("response-1".to_string()),
            last_response_items: Vec::new(),
            ..ResponsesWebSocketSession::default()
        };
        let current = Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            (
                "tools".to_string(),
                serde_json::json!([
                    {"type": "function", "name": "exec"},
                    {"type": "function", "name": "read_file"}
                ]),
            ),
            ("input".to_string(), serde_json::json!([user])),
        ]);

        assert_eq!(
            incremental_request(&session, &current).unwrap_err(),
            IncrementalRequestFallbackReason::RequestPropertiesChanged
        );
    }

    #[test]
    fn incremental_request_requires_working_context_to_keep_its_turn_anchor() {
        let user = serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "implement"}],
        });
        let working_context = serde_json::json!({
            "type": "message",
            "role": "developer",
            "content": [{"type": "input_text", "text": "# Current working context"}],
        });
        let tool_call = serde_json::json!({
            "type": "function_call",
            "name": "read_file",
            "arguments": "{\"path\":\"src/lib.rs\"}",
            "call_id": "call-1",
        });
        let tool_result = serde_json::json!({
            "type": "function_call_output",
            "call_id": "call-1",
            "output": "ok",
        });
        let session = ResponsesWebSocketSession {
            last_request: Some(Map::from_iter([
                ("model".to_string(), serde_json::json!("gpt-test")),
                (
                    "input".to_string(),
                    Value::Array(vec![user.clone(), working_context.clone()]),
                ),
            ])),
            last_response_id: Some("response-1".to_string()),
            last_response_items: vec![tool_call.clone()],
            ..ResponsesWebSocketSession::default()
        };
        let anchored = Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            (
                "input".to_string(),
                Value::Array(vec![
                    user.clone(),
                    working_context.clone(),
                    tool_call.clone(),
                    tool_result.clone(),
                ]),
            ),
        ]);
        let relocated = Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            (
                "input".to_string(),
                Value::Array(vec![user, tool_call, tool_result.clone(), working_context]),
            ),
        ]);

        let incremental = incremental_request(&session, &anchored).unwrap();
        assert_eq!(incremental["input"], Value::Array(vec![tool_result]));
        assert_eq!(incremental["previous_response_id"], "response-1");
        assert_eq!(
            incremental_request(&session, &relocated).unwrap_err(),
            IncrementalRequestFallbackReason::InputPrefixMismatch {
                previous_prefix_items: 3,
                first_differing_index: 1,
            }
        );
    }
}

/// WebSocket 传输编排行为：会话连接复用、重试预算、invalid continuation
/// 回退与 HTTP fallback 隔离，经 `ModelRuntime::complete` 端到端驱动。
#[cfg(test)]
mod orchestration_tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use futures::{SinkExt, StreamExt};
    use pretty_assertions::assert_eq;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::WebSocketStream;
    use tokio_tungstenite::tungstenite::Message as WebSocketMessage;
    use tokio_tungstenite::tungstenite::handshake::server::{
        ErrorResponse, Request as WebSocketRequest, Response as WebSocketResponse,
    };
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
    use tokio_tungstenite::{accept_async, accept_hdr_async};

    use crate::completion::{CompletionRequest, CompletionTraceContext};
    use crate::provider::ProviderEndpoint;
    use crate::runtime::test_support::{
        assert_complete_workflow_wire_body, capture_http_request, complete_workflow_wire_request,
        minimal_request, responses_websocket_model, send_responses_sse,
    };
    use crate::runtime::transport_policy::RESPONSES_WEBSOCKET_MAX_RETRIES;
    use crate::runtime::{ModelInvocationContext, ModelRuntime, ModelSession};
    use pl_protocol::{Message, MessageContent, MessageRole};
    use pl_trace::AgentEvent;

    type WsStream = WebSocketStream<TcpStream>;
    type WsSender = futures::stream::SplitSink<WsStream, WebSocketMessage>;
    type WsReceiver = futures::stream::SplitStream<WsStream>;

    // tungstenite 的 header callback 必须返回其固定 ErrorResponse；该第三方类型较大，
    // 测试 helper 无法在不改变 callback 契约的情况下装箱。
    #[allow(clippy::result_large_err)]
    fn validate_openai_websocket_handshake(
        request: &WebSocketRequest,
        response: WebSocketResponse,
    ) -> std::result::Result<WebSocketResponse, ErrorResponse> {
        assert_eq!(request.uri().path(), "/v1/responses");
        assert_eq!(
            request.headers()["openai-beta"],
            "responses_websockets=2026-02-06"
        );
        assert_eq!(request.headers()["authorization"], "Bearer test-token");
        Ok(response)
    }

    async fn accept_websocket(listener: &TcpListener) -> (WsSender, WsReceiver) {
        let (stream, _) = listener.accept().await.unwrap();
        accept_async(stream).await.unwrap().split()
    }

    async fn read_json_frame(reader: &mut WsReceiver) -> serde_json::Value {
        match reader.next().await.unwrap().unwrap() {
            WebSocketMessage::Text(text) => serde_json::from_str(text.as_str()).unwrap(),
            other => panic!("expected a text frame, got {other:?}"),
        }
    }

    async fn send_json_events(writer: &mut WsSender, events: &[serde_json::Value]) {
        for event in events {
            writer
                .send(WebSocketMessage::Text(event.to_string().into()))
                .await
                .unwrap();
        }
    }

    fn streamed_completion(
        response_id: &str,
        message_id: &str,
        text: &str,
    ) -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "type": "response.output_text.delta",
                "item_id": message_id,
                "delta": text
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": response_id,
                    "model": "local-responses",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": text }]
                    }],
                    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                }
            }),
        ]
    }

    fn created_completion(
        response_id: &str,
        message_id: &str,
        text: &str,
    ) -> Vec<serde_json::Value> {
        vec![serde_json::json!({
            "type": "response.created",
            "response": {"id": response_id, "model": "local-responses"}
        })]
        .into_iter()
        .chain(streamed_completion(response_id, message_id, text))
        .collect()
    }

    /// 只建立流的 created + delta 事件，不带终止 completed；用于注入中途失败。
    fn started_stream_events(
        response_id: &str,
        message_id: &str,
        text: &str,
    ) -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "type": "response.created",
                "response": {"id": response_id, "model": "local-responses"}
            }),
            serde_json::json!({
                "type": "response.output_text.delta",
                "item_id": message_id,
                "delta": text
            }),
        ]
    }

    fn local_websocket_provider(address: &str, context_window: Option<u64>) -> ModelRuntime {
        let mut info = ProviderEndpoint::openai(Some(format!("http://{address}/v1")));
        info.bearer_token = Some("test-token".to_string());
        let mut model = responses_websocket_model("local-responses");
        model.context_window = context_window;
        ModelRuntime::new(info, model).unwrap()
    }

    fn followup_request(previous_assistant_text: &str) -> CompletionRequest {
        let mut request = minimal_request("local-responses");
        request.input.extend([
            Message {
                presentation: Default::default(),
                role: MessageRole::Assistant,
                content: MessageContent::text(previous_assistant_text.to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_result: None,
                metadata: HashMap::new(),
            }
            .into(),
            Message {
                presentation: Default::default(),
                role: MessageRole::User,
                content: MessageContent::text("again".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_result: None,
                metadata: HashMap::new(),
            }
            .into(),
        ]);
        request
    }

    #[tokio::test]
    async fn responses_websocket_reuses_the_agent_session_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let websocket = accept_hdr_async(stream, validate_openai_websocket_handshake)
                .await
                .unwrap();
            let (mut writer, mut reader) = websocket.split();
            let mut requests = Vec::new();
            for ordinal in 1..=2 {
                requests.push(read_json_frame(&mut reader).await);
                send_json_events(
                    &mut writer,
                    &created_completion(
                        &format!("resp-{ordinal}"),
                        &format!("message-{ordinal}"),
                        &format!("ok-{ordinal}"),
                    ),
                )
                .await;
                if ordinal == 1 {
                    writer
                        .send(WebSocketMessage::Ping(vec![1, 2, 3].into()))
                        .await
                        .unwrap();
                    let pong =
                        tokio::time::timeout(std::time::Duration::from_secs(1), reader.next())
                            .await
                            .expect("client must answer ping while the model stream is idle")
                            .expect("pong frame")
                            .expect("valid pong frame");
                    assert_eq!(pong, WebSocketMessage::Pong(vec![1, 2, 3].into()));
                }
            }
            requests
        });

        let provider = local_websocket_provider(&address.to_string(), Some(128_000));
        let session = ModelSession::default();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

        let first = provider
            .complete(
                complete_workflow_wire_request(),
                ModelInvocationContext::new(session.clone())
                    .with_events(event_tx.clone())
                    .with_prompt_cache_key(Some("thread-generation-key".to_string())),
            )
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut second_request = complete_workflow_wire_request();
        second_request.input.extend([
            Message {
                presentation: Default::default(),
                role: MessageRole::Assistant,
                content: MessageContent::text("ok-1".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_result: None,
                metadata: HashMap::new(),
            }
            .into(),
            Message {
                presentation: Default::default(),
                role: MessageRole::User,
                content: MessageContent::text("again".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_result: None,
                metadata: HashMap::new(),
            }
            .into(),
        ]);
        let second = provider
            .complete(
                second_request,
                ModelInvocationContext::new(session)
                    .with_events(event_tx)
                    .with_prompt_cache_key(Some("thread-generation-key".to_string())),
            )
            .await
            .unwrap();
        let requests = server.await.unwrap();

        assert_eq!(first.content.as_deref(), Some("ok-1"));
        assert_eq!(second.content.as_deref(), Some("ok-2"));
        assert_eq!(first.orchestration.continuation_attempts, 0);
        assert_eq!(second.orchestration.continuation_attempts, 1);
        assert_eq!(second.orchestration.continuation_used, 1);
        assert_eq!(second.orchestration.continuation_invalid, 0);
        assert_eq!(requests[0]["type"], "response.create");
        assert!(
            requests[0]["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty())
        );
        assert_eq!(requests[0]["store"], false);
        assert_eq!(requests[1]["store"], false);
        assert_eq!(requests[0]["prompt_cache_key"], "thread-generation-key");
        assert_eq!(requests[1]["prompt_cache_key"], "thread-generation-key");
        assert_eq!(requests[1]["previous_response_id"], "resp-1");
        assert_complete_workflow_wire_body(&requests[0]);
        assert_eq!(
            requests[1]["input"],
            serde_json::json!([{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "again" }]
            }])
        );
    }

    #[tokio::test]
    async fn responses_websocket_partial_failure_falls_back_only_for_the_next_turn() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let first = read_json_frame(&mut reader).await;
            send_json_events(
                &mut writer,
                &started_stream_events("partial-response", "partial-message", "partial"),
            )
            .await;
            writer
                .send(WebSocketMessage::Text(
                    serde_json::json!({
                        "type": "error",
                        "error": {
                            "code": "server_error",
                            "message": "upstream websocket proxy failed"
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            drop(writer);

            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(750), listener.accept())
                    .await
                    .is_err(),
                "a request that emitted stream events must not be replayed"
            );
            first
        });

        let provider = local_websocket_provider(&address.to_string(), Some(128_000));
        let session = ModelSession::default();
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(64);
        let trace_sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 0));
        let context = ModelInvocationContext::new(session.clone())
            .with_events(event_tx)
            .with_trace(
                CompletionTraceContext {
                    session_id: "session-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    inference_id: "turn-1-inf-0".to_string(),
                },
                trace_sink,
            );

        let error = provider
            .complete(minimal_request("local-responses"), context)
            .await
            .expect_err("partial stream failure must be returned without replay");
        let initial = server.await.unwrap();
        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();

        assert!(error.is_transient_model_transport());
        assert_eq!(initial["type"], "response.create");
        assert!(
            session.uses_responses_http_fallback(provider.connection_fingerprint()),
            "a partial WebSocket failure must move only the next turn to HTTP"
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::TracePartFailed { item }
                if item.item_id() == "turn-1-inf-0-text-final-1"
                    && item.failure().is_some_and(|error| error.contains("upstream websocket proxy failed"))
        )));
    }

    #[tokio::test]
    async fn responses_websocket_does_not_retry_invalid_request_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let _ = read_json_frame(&mut reader).await;
            writer
                .send(WebSocketMessage::Text(
                    serde_json::json!({
                        "type": "error",
                        "status": 400,
                        "error": {
                            "code": "invalid_request_error",
                            "message": "model does not support image inputs"
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
        });

        let info = ProviderEndpoint::openai(Some(format!("http://{address}/v1")));
        let provider =
            ModelRuntime::new(info, responses_websocket_model("local-responses")).unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

        let error = provider
            .complete(
                minimal_request("local-responses"),
                ModelInvocationContext::new(ModelSession::default()).with_events(event_tx),
            )
            .await
            .expect_err("invalid request must fail without transport retry");
        server.await.unwrap();

        assert!(!error.is_transient_model_transport());
        assert!(error.to_string().contains("invalid_request_error"));
        assert!(error.to_string().contains("HTTP 400"));
    }

    #[tokio::test]
    async fn responses_websocket_does_not_retry_unauthorized_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4_096];
            let read = socket.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..read]).contains("GET /v1/responses"));
            socket
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });

        let info = ProviderEndpoint::openai(Some(format!("http://{address}/v1")));
        let provider =
            ModelRuntime::new(info, responses_websocket_model("local-responses")).unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

        let error = provider
            .complete(
                minimal_request("local-responses"),
                ModelInvocationContext::new(ModelSession::default()).with_events(event_tx),
            )
            .await
            .expect_err("unauthorized handshake must fail without transport retry");
        server.await.unwrap();

        assert!(!error.is_transient_model_transport());
        assert_eq!(
            error.to_string(),
            "HTTP error: Responses WebSocket handshake failed with HTTP 401"
        );
    }

    #[tokio::test]
    async fn responses_websocket_retries_an_immediate_close_with_full_history() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let initial = read_json_frame(&mut reader).await;
            writer.send(WebSocketMessage::Close(None)).await.unwrap();
            drop(writer);

            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let retried = read_json_frame(&mut reader).await;
            send_json_events(
                &mut writer,
                &streamed_completion("resp-1", "message-1", "ok"),
            )
            .await;
            [initial, retried]
        });

        let provider = local_websocket_provider(&address.to_string(), Some(128_000));
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

        let response = provider
            .complete(
                minimal_request("local-responses"),
                ModelInvocationContext::new(ModelSession::default())
                    .with_events(event_tx)
                    .with_prompt_cache_key(Some("thread-generation-key".to_string())),
            )
            .await
            .unwrap();
        let [initial, retried] = server.await.unwrap();

        assert_eq!(response.content.as_deref(), Some("ok"));
        assert_eq!(response.orchestration.transport_attempts, 2);
        assert_eq!(response.orchestration.http_fallbacks, 0);
        assert_eq!(initial, retried);
        assert_eq!(retried["tools"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn responses_websocket_falls_back_to_http_after_one_full_replay() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut websocket_requests = Vec::new();
            for _ in 0..=RESPONSES_WEBSOCKET_MAX_RETRIES {
                let (mut writer, mut reader) = accept_websocket(&listener).await;
                websocket_requests.push(read_json_frame(&mut reader).await);
                writer
                    .send(WebSocketMessage::Close(Some(CloseFrame {
                        code: CloseCode::Error,
                        reason: "upstream websocket proxy failed".into(),
                    })))
                    .await
                    .unwrap();
            }

            let mut http_requests = Vec::new();
            for (response_id, message_id, text) in [
                ("http-response-1", "http-message-1", "http-ok-1"),
                ("http-response-2", "http-message-2", "http-ok-2"),
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                http_requests.push(capture_http_request(&mut socket).await);
                send_responses_sse(&mut socket, response_id, message_id, text).await;
            }

            (websocket_requests, http_requests)
        });

        let provider = local_websocket_provider(&address.to_string(), None);
        let session = ModelSession::default();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

        let first = provider
            .complete(
                minimal_request("local-responses"),
                ModelInvocationContext::new(session.clone()).with_events(event_tx.clone()),
            )
            .await
            .unwrap();
        assert!(session.uses_responses_http_fallback(provider.connection_fingerprint()));

        let second = provider
            .complete(
                followup_request("http-ok-1"),
                ModelInvocationContext::new(session).with_events(event_tx),
            )
            .await
            .unwrap();
        let (websocket_requests, http_requests) = server.await.unwrap();

        assert_eq!(first.content.as_deref(), Some("http-ok-1"));
        assert_eq!(second.content.as_deref(), Some("http-ok-2"));
        assert_eq!(first.orchestration.transport_attempts, 3);
        assert_eq!(first.orchestration.http_fallbacks, 1);
        assert_eq!(second.orchestration.transport_attempts, 1);
        assert_eq!(second.orchestration.http_fallbacks, 0);
        assert_eq!(websocket_requests.len(), 2);
        assert_eq!(websocket_requests[0], websocket_requests[1]);
        assert_eq!(http_requests.len(), 2);
        assert_eq!(http_requests[0].request_line, "POST /v1/responses HTTP/1.1");
        assert_eq!(http_requests[1].request_line, "POST /v1/responses HTTP/1.1");
        assert_eq!(http_requests[0].body["input"].as_array().unwrap().len(), 1);
        assert_eq!(http_requests[1].body["input"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn responses_http_fallback_isolated_by_model_transport_fingerprint() {
        let endpoint = ProviderEndpoint::openai(Some("http://127.0.0.1:1/v1".to_string()));
        let first_runtime =
            ModelRuntime::new(endpoint.clone(), responses_websocket_model("responses-a")).unwrap();
        let second_runtime =
            ModelRuntime::new(endpoint, responses_websocket_model("responses-b")).unwrap();
        let session = ModelSession::default();
        let first = first_runtime.connection_fingerprint();
        let second = second_runtime.connection_fingerprint();

        assert_ne!(first, second);
        assert!(session.activate_responses_http_fallback(first).await);
        assert!(session.uses_responses_http_fallback(first));
        assert!(!session.uses_responses_http_fallback(second));
    }

    #[tokio::test]
    async fn responses_websocket_retries_an_invalid_continuation_once_with_full_history() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let initial = read_json_frame(&mut reader).await;
            send_json_events(
                &mut writer,
                &streamed_completion("resp-1", "message-1", "ok-1"),
            )
            .await;
            let incremental = read_json_frame(&mut reader).await;
            writer
                .send(WebSocketMessage::Text(
                    serde_json::json!({
                        "type": "error",
                        "error": {
                            "code": "invalid_previous_response_id",
                            "message": "previous_response_id is no longer valid"
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            drop(writer);

            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let retried = read_json_frame(&mut reader).await;
            send_json_events(
                &mut writer,
                &streamed_completion("resp-2", "message-2", "ok-2"),
            )
            .await;
            [initial, incremental, retried]
        });

        let provider = local_websocket_provider(&address.to_string(), Some(128_000));
        let session = ModelSession::default();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

        let first = provider
            .complete(
                minimal_request("local-responses"),
                ModelInvocationContext::new(session.clone()).with_events(event_tx.clone()),
            )
            .await
            .unwrap();
        let second = provider
            .complete(
                followup_request("ok-1"),
                ModelInvocationContext::new(session).with_events(event_tx),
            )
            .await
            .unwrap();
        let [initial, incremental, retried] = server.await.unwrap();

        assert_eq!(first.content.as_deref(), Some("ok-1"));
        assert_eq!(second.content.as_deref(), Some("ok-2"));
        assert_eq!(first.orchestration.transport_attempts, 1);
        assert_eq!(second.orchestration.transport_attempts, 2);
        assert_eq!(second.orchestration.continuation_attempts, 1);
        assert_eq!(second.orchestration.continuation_invalid, 1);
        assert_eq!(second.orchestration.continuation_used, 0);
        assert_eq!(second.orchestration.http_fallbacks, 0);
        assert!(initial.get("previous_response_id").is_none());
        assert_eq!(incremental["previous_response_id"], "resp-1");
        assert_eq!(incremental["input"].as_array().unwrap().len(), 1);
        assert!(retried.get("previous_response_id").is_none());
        assert_eq!(retried["input"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn invalid_continuation_full_replay_consumes_the_websocket_retry_budget() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let initial = read_json_frame(&mut reader).await;
            send_json_events(
                &mut writer,
                &streamed_completion("resp-1", "message-1", "ok-1"),
            )
            .await;

            let incremental = read_json_frame(&mut reader).await;
            writer
                .send(WebSocketMessage::Text(
                    serde_json::json!({
                        "type": "error",
                        "error": {
                            "code": "invalid_previous_response_id",
                            "message": "previous_response_id is no longer valid"
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            drop(writer);

            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let full_replay = read_json_frame(&mut reader).await;
            writer
                .send(WebSocketMessage::Close(Some(CloseFrame {
                    code: CloseCode::Error,
                    reason: "full replay failed".into(),
                })))
                .await
                .unwrap();
            drop(writer);

            let (mut socket, _) = listener.accept().await.unwrap();
            let http_request = capture_http_request(&mut socket).await;
            send_responses_sse(
                &mut socket,
                "http-response-2",
                "http-message-2",
                "http-ok-2",
            )
            .await;

            ([initial, incremental, full_replay], http_request)
        });

        let provider = local_websocket_provider(&address.to_string(), None);
        let session = ModelSession::default();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

        let first = provider
            .complete(
                minimal_request("local-responses"),
                ModelInvocationContext::new(session.clone()).with_events(event_tx.clone()),
            )
            .await
            .unwrap();
        let second = provider
            .complete(
                followup_request("ok-1"),
                ModelInvocationContext::new(session.clone()).with_events(event_tx),
            )
            .await
            .unwrap();
        let ([initial, incremental, full_replay], http_request) = server.await.unwrap();

        assert_eq!(first.content.as_deref(), Some("ok-1"));
        assert_eq!(second.content.as_deref(), Some("http-ok-2"));
        assert_eq!(second.orchestration.transport_attempts, 3);
        assert_eq!(second.orchestration.continuation_attempts, 1);
        assert_eq!(second.orchestration.continuation_invalid, 1);
        assert_eq!(second.orchestration.http_fallbacks, 1);
        assert!(session.uses_responses_http_fallback(provider.connection_fingerprint()));
        assert!(initial.get("previous_response_id").is_none());
        assert_eq!(incremental["previous_response_id"], "resp-1");
        assert!(full_replay.get("previous_response_id").is_none());
        assert_eq!(full_replay["input"].as_array().unwrap().len(), 3);
        assert_eq!(http_request.request_line, "POST /v1/responses HTTP/1.1");
        assert_eq!(http_request.body["input"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn cancelled_websocket_task_replays_full_history_without_uncommitted_continuation() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let ready = std::sync::Arc::new(tokio::sync::Notify::new());
        let signal = ready.clone();
        let server = tokio::spawn(async move {
            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let first = read_json_frame(&mut reader).await;
            writer
                .send(WebSocketMessage::Text(
                    serde_json::json!({"type":"response.output_text.delta","item_id":"unfinished","delta":"not-consumed"})
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            signal.notify_one();
            let _ = reader.next().await;
            drop(writer);

            let (mut writer, mut reader) = accept_websocket(&listener).await;
            let second = read_json_frame(&mut reader).await;
            send_json_events(
                &mut writer,
                &streamed_completion("resp-2", "message-2", "ok"),
            )
            .await;
            [first, second]
        });

        let provider = local_websocket_provider(&address.to_string(), None);
        let session = ModelSession::default();
        let first_request = minimal_request("local-responses");
        let token = tokio_util::sync::CancellationToken::new();
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let running_provider = provider.clone();
        let context = ModelInvocationContext::new(session.clone())
            .with_events(event_tx)
            .with_cancellation(Some(token.clone()));
        let first =
            tokio::spawn(async move { running_provider.complete(first_request, context).await });
        ready.notified().await;
        token.cancel();
        assert!(first.await.unwrap().is_err());

        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let response = provider
            .complete(
                followup_request("not-consumed"),
                ModelInvocationContext::new(session).with_events(event_tx),
            )
            .await
            .unwrap();
        let [first, second] = server.await.unwrap();

        assert_eq!(response.content.as_deref(), Some("ok"));
        assert!(first.get("previous_response_id").is_none());
        assert!(second.get("previous_response_id").is_none());
        assert_eq!(second["input"].as_array().unwrap().len(), 3);
    }
}
