use std::collections::HashMap;

use futures::{SinkExt, StreamExt};
use pl_protocol::{PureError, Result};
use serde_json::{Map, Value};
use tokio::sync::OwnedMutexGuard;
use tokio::time::{Duration, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};

use crate::ModelTransportSession;
use crate::protocol::openai::sse::SseStreamEvent;
use crate::provider_info::RESPONSES_WEBSOCKET_DIALECT;
use crate::stream::OpenAiRawEventStream;
use crate::transport_session::{ResponsesWebSocket, ResponsesWebSocketSession};

use super::redact_secret_like_values;

const WEBSOCKET_IDLE_TIMEOUT: Duration = Duration::from_secs(180);

pub(super) async fn stream_responses(
    api_base: String,
    token: Option<String>,
    provider_headers: Option<&HashMap<String, String>>,
    model_headers: &HashMap<String, String>,
    connection_key: u64,
    transport_session: ModelTransportSession,
    mut body: Map<String, Value>,
) -> Result<OpenAiRawEventStream> {
    normalize_websocket_request_body(&mut body);

    let mut guard = transport_session.lock_responses_websocket().await;
    if guard.connection_key != Some(connection_key) || guard.connection.is_none() {
        guard.invalidate();
        guard.connection =
            Some(connect(&api_base, token.as_deref(), provider_headers, model_headers).await?);
        guard.connection_key = Some(connection_key);
    }

    let (mut wire_body, mut used_continuation) = incremental_request(&guard, &body)
        .map(|body| (body, true))
        .unwrap_or_else(|| (body.clone(), false));
    let mut request_text = response_create_text(&mut wire_body)?;
    if send_request(&mut guard, &request_text).await.is_err() {
        guard.invalidate();
        guard.connection =
            Some(connect(&api_base, token.as_deref(), provider_headers, model_headers).await?);
        guard.connection_key = Some(connection_key);
        used_continuation = false;
        let mut retry_body = body.clone();
        request_text = response_create_text(&mut retry_body)?;
        if let Err(error) = send_request(&mut guard, &request_text).await {
            guard.invalidate();
            return Err(error);
        }
    }

    let state = WebSocketEventState {
        guard,
        terminal: false,
        used_continuation,
        retry_available: true,
        events_emitted: false,
        full_request: body,
        reconnect: WebSocketReconnect {
            api_base,
            token,
            provider_headers: provider_headers.cloned(),
            model_headers: model_headers.clone(),
            connection_key,
        },
    };
    Ok(Box::pin(futures::stream::unfold(
        state,
        |mut state| async move {
            if state.terminal {
                return None;
            }
            let event = state.next_event().await;
            Some((event, state))
        },
    )))
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
) -> Option<Map<String, Value>> {
    let previous = session.last_request.as_ref()?;
    let response_id = session.last_response_id.as_deref()?;
    if request_properties(previous) != request_properties(current) {
        return None;
    }
    let mut previous_items = previous.get("input")?.as_array()?.clone();
    previous_items.extend(session.last_response_items.iter().cloned());
    let current_items = current.get("input")?.as_array()?;
    if !current_items.starts_with(&previous_items) {
        return None;
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
    Some(incremental)
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
) -> Result<ResponsesWebSocket> {
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

    let (connection, _) = connect_async(request).await.map_err(|error| {
        PureError::HttpError(redact_secret_like_values(&format!(
            "Responses WebSocket handshake failed: {error}"
        )))
    })?;
    Ok(connection)
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
    let connection = session.connection.as_mut().ok_or_else(|| {
        PureError::HttpError("Responses WebSocket connection is unavailable".to_string())
    })?;
    timeout(
        WEBSOCKET_IDLE_TIMEOUT,
        connection.send(Message::Text(request_text.to_string().into())),
    )
    .await
    .map_err(|_| PureError::HttpError("Responses WebSocket send timed out".to_string()))?
    .map_err(|error| PureError::HttpError(error.to_string()))
}

struct WebSocketEventState {
    guard: OwnedMutexGuard<ResponsesWebSocketSession>,
    terminal: bool,
    used_continuation: bool,
    retry_available: bool,
    events_emitted: bool,
    full_request: Map<String, Value>,
    reconnect: WebSocketReconnect,
}

struct WebSocketReconnect {
    api_base: String,
    token: Option<String>,
    provider_headers: Option<HashMap<String, String>>,
    model_headers: HashMap<String, String>,
    connection_key: u64,
}

impl WebSocketEventState {
    async fn next_event(&mut self) -> Result<SseStreamEvent> {
        loop {
            let next = {
                let connection = self.guard.connection.as_mut().ok_or_else(|| {
                    PureError::HttpError(
                        "Responses WebSocket connection is unavailable".to_string(),
                    )
                })?;
                timeout(WEBSOCKET_IDLE_TIMEOUT, connection.next()).await
            };
            let message = match next {
                Ok(Some(Ok(message))) => message,
                Ok(Some(Err(error))) => {
                    if self.retry_after_disconnect().await? {
                        continue;
                    }
                    return Err(self.connection_error(error.to_string()));
                }
                Ok(None) => {
                    if self.retry_after_disconnect().await? {
                        continue;
                    }
                    return Err(self.connection_error(
                        "connection closed before a terminal response event".to_string(),
                    ));
                }
                Err(_) => {
                    if self.retry_after_disconnect().await? {
                        continue;
                    }
                    return Err(self.connection_error(
                        "idle timeout waiting for a response event".to_string(),
                    ));
                }
            };
            match message {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(text.as_str()).map_err(|error| {
                        self.connection_error(format!("invalid Responses WebSocket event: {error}"))
                    })?;
                    if value.get("type").and_then(Value::as_str) == Some("error") {
                        if !self.events_emitted
                            && self.used_continuation
                            && continuation_id_invalid(&value)
                        {
                            self.restart_with_full_history().await?;
                            continue;
                        }
                        return Err(self.connection_error(websocket_error_message(&value)));
                    }
                    let event: SseStreamEvent = serde_json::from_value(value)
                        .map_err(|error| self.connection_error(error.to_string()))?;
                    match event.kind.as_str() {
                        "response.completed" => {
                            self.terminal = true;
                            self.commit_completed_response(&event);
                        }
                        "response.failed" | "response.incomplete" => {
                            self.terminal = true;
                            self.guard.invalidate();
                        }
                        _ => {}
                    }
                    self.events_emitted = true;
                    return Ok(event);
                }
                Message::Ping(payload) => {
                    let connection = self.guard.connection.as_mut().ok_or_else(|| {
                        PureError::HttpError(
                            "Responses WebSocket connection is unavailable".to_string(),
                        )
                    })?;
                    connection
                        .send(Message::Pong(payload))
                        .await
                        .map_err(|error| self.connection_error(error.to_string()))?;
                }
                Message::Pong(_) | Message::Frame(_) => {}
                Message::Binary(_) => {
                    return Err(PureError::LlmError(
                        "unexpected binary Responses WebSocket event".to_string(),
                    ));
                }
                Message::Close(frame) => {
                    if self.retry_after_disconnect().await? {
                        continue;
                    }
                    return Err(
                        self.connection_error(format!("server closed the connection: {frame:?}"))
                    );
                }
            }
        }
    }

    fn connection_error(&mut self, detail: String) -> PureError {
        self.guard.invalidate();
        let prefix = if self.used_continuation {
            "previous_response_id requires the original Responses WebSocket connection"
        } else {
            "Responses WebSocket stream failed"
        };
        PureError::HttpError(redact_secret_like_values(&format!("{prefix}: {detail}")))
    }

    async fn retry_after_disconnect(&mut self) -> Result<bool> {
        if self.events_emitted || !self.retry_available {
            return Ok(false);
        }
        self.restart_with_full_history().await?;
        Ok(true)
    }

    async fn restart_with_full_history(&mut self) -> Result<()> {
        self.retry_available = false;
        self.guard.invalidate();
        self.guard.connection = Some(
            connect(
                &self.reconnect.api_base,
                self.reconnect.token.as_deref(),
                self.reconnect.provider_headers.as_ref(),
                &self.reconnect.model_headers,
            )
            .await?,
        );
        self.guard.connection_key = Some(self.reconnect.connection_key);
        let mut full_request = self.full_request.clone();
        let request_text = response_create_text(&mut full_request)?;
        if let Err(error) = send_request(&mut self.guard, &request_text).await {
            self.guard.invalidate();
            return Err(error);
        }
        self.used_continuation = false;
        Ok(())
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
        if !self.terminal {
            self.guard.invalidate();
        }
    }
}

fn websocket_error_message(value: &Value) -> String {
    let error = value.get("error").unwrap_or(value);
    let code = error.get("code").and_then(Value::as_str);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Responses WebSocket returned an error");
    redact_secret_like_values(&match code {
        Some(code) => format!("Responses WebSocket error {code}: {message}"),
        None => format!("Responses WebSocket error: {message}"),
    })
}

fn continuation_id_invalid(value: &Value) -> bool {
    let error = value.get("error").unwrap_or(value);
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    code.contains("previous_response")
        || message.contains("previous_response_id")
        || (message.contains("response")
            && (message.contains("not found") || message.contains("invalid")))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value};

    use super::{
        canonical_response_history_items, normalize_websocket_request_body, responses_websocket_url,
    };

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
}
