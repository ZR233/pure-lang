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

use crate::ModelSession;
use crate::completion::stream::OpenAiRawEventStream;
use crate::provider::RESPONSES_WEBSOCKET_DIALECT;
use crate::runtime::openai::sse::SseStreamEvent;
use crate::runtime::session::{ResponsesWebSocketConnection, ResponsesWebSocketSession};
use crate::runtime::transport_policy::{
    RESPONSES_WEBSOCKET_CONNECT_TIMEOUT, RESPONSES_WEBSOCKET_IDLE_TIMEOUT,
    RESPONSES_WEBSOCKET_SEND_TIMEOUT,
};

mod dialer;
mod error;
mod state;

use error::{
    close_error, connection_error, continuation_id_invalid, continuation_retry_error,
    handshake_error, handshake_timeout_error, protocol_error, response_terminal_error,
    server_error,
};
use state::{
    ClosedResponsesStream, CompletedResponsesStream, FailedResponsesStream, ResponsesStreamState,
};

pub(super) async fn stream_responses(
    api_base: String,
    token: Option<String>,
    provider_headers: Option<&HashMap<String, String>>,
    model_headers: &HashMap<String, String>,
    connection_key: u64,
    model_session: ModelSession,
    mut body: Map<String, Value>,
) -> Result<OpenAiRawEventStream> {
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
    .map_err(connection_error)
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
                    return Err(self.invalidate_with_connection_error(
                        "connection closed before a terminal response event",
                    ));
                }
                Err(_) => {
                    return Err(self.invalidate_with_connection_error(
                        "idle timeout waiting for a response event",
                    ));
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

    fn invalidate_with_connection_error(&mut self, detail: impl AsRef<str>) -> PureError {
        let error = connection_error(detail);
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
mod unit_tests;
