use std::collections::HashMap;
use std::time::Duration;

use async_openai::Client;
use async_openai::config::Config;
use async_openai::error::OpenAIError;
use async_openai::types::stream::StreamResponse;
use pl_protocol::{AgentEventSender, PureError, Result};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use secrecy::SecretString;

use crate::capabilities::{ModelCapabilities, ProviderCapabilities};
use crate::model_info::ModelInfo;
use crate::protocol::openai::sse;
use crate::protocol::openai::{OpenAiProtocol, OpenAiRequestBody};
use crate::provider_info::ProviderInfo;
use crate::request::{CompletionRequest, CompletionResponse};
use crate::stream::process_provider_stream;

#[derive(Debug)]
pub(crate) struct OpenAiTransportProvider {
    info: ProviderInfo,
    http_client: reqwest::Client,
    protocol: OpenAiProtocol,
    capabilities: ProviderCapabilities,
    bundled_models: Vec<ModelInfo>,
}

impl OpenAiTransportProvider {
    pub(crate) fn new(
        info: ProviderInfo,
        bundled_models: Vec<ModelInfo>,
        configured_models: Vec<ModelInfo>,
        protocol: OpenAiProtocol,
        capabilities: ProviderCapabilities,
    ) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| PureError::HttpError(e.to_string()))?;

        let bundled_models = merge_models(bundled_models, configured_models);

        Ok(Self {
            info,
            http_client,
            protocol,
            capabilities,
            bundled_models,
        })
    }

    fn resolve_base_url(&self) -> String {
        self.info.base_url.clone().trim_end_matches('/').to_string()
    }
    pub(crate) fn info(&self) -> &ProviderInfo {
        &self.info
    }

    pub(crate) fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }

    pub(crate) fn auth_token(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<String>>> + Send {
        let bearer = self.info.bearer_token.clone();
        get_auth_token(bearer)
    }

    pub(crate) fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send {
        let http_client = self.http_client.clone();
        let api_base = self.resolve_base_url();
        let protocol = self.protocol;
        let info = self.info.clone();
        let model_info = self.model_info(&request.model);
        async move {
            let bearer = info.bearer_token.clone();
            let token = get_auth_token(bearer).await?;

            let supports_custom_tools = info.uses_native_custom_tools()
                && model_info.capabilities.supports_custom_tools()
                && model_info.capabilities.supports_freeform_tools();
            let timeline = request.timeline.clone();
            let request = request.provider_compatible(supports_custom_tools);
            let body = protocol.build_request(&request)?;
            let config = PureOpenAiConfig::new(api_base, token, info.http_headers.as_ref())?;
            let client = Client::build(http_client, config);
            let stream: StreamResponse<sse::SseStreamEvent> = match body {
                OpenAiRequestBody::Responses(body) => client
                    .responses()
                    .create_stream_byot(body)
                    .await
                    .map_err(openai_error_to_pure)?,
                OpenAiRequestBody::Chat(body) => client
                    .chat()
                    .create_stream_byot(body)
                    .await
                    .map_err(openai_error_to_pure)?,
            };

            process_provider_stream(stream, &event_tx, &protocol, timeline).await
        }
    }

    pub(crate) fn model_info(&self, model: &str) -> ModelInfo {
        self.bundled_models
            .iter()
            .find(|m| m.slug == model)
            .cloned()
            .unwrap_or_else(|| ModelInfo::fallback(model))
    }

    pub(crate) fn list_models(&self) -> Vec<ModelInfo> {
        self.bundled_models.clone()
    }

    pub(crate) fn effective_model_capabilities(&self, model: &str) -> ModelCapabilities {
        let mut capabilities = self.model_info(model).capabilities;
        if !self
            .capabilities
            .contains(ProviderCapabilities::PARALLEL_TOOL_CALLS)
        {
            capabilities.remove(ModelCapabilities::PARALLEL_TOOL_CALLS);
        }
        if !self
            .capabilities
            .contains(ProviderCapabilities::FUNCTION_CALLING)
        {
            capabilities.remove(
                ModelCapabilities::FUNCTION_CALLING
                    | ModelCapabilities::CUSTOM_TOOLS
                    | ModelCapabilities::FREEFORM_TOOLS,
            );
        }
        if !self.capabilities.contains(ProviderCapabilities::VISION) {
            capabilities.remove(ModelCapabilities::VISION);
        }
        if !self.info.uses_native_custom_tools() {
            capabilities
                .remove(ModelCapabilities::CUSTOM_TOOLS | ModelCapabilities::FREEFORM_TOOLS);
        }
        capabilities
    }

    pub(crate) fn default_model(&self) -> &str {
        self.info.default_model.as_str()
    }
}

fn merge_models(
    mut bundled_models: Vec<ModelInfo>,
    configured_models: Vec<ModelInfo>,
) -> Vec<ModelInfo> {
    for model in configured_models {
        match bundled_models
            .iter_mut()
            .find(|existing| existing.slug == model.slug)
        {
            Some(existing) => *existing = model,
            None => bundled_models.push(model),
        }
    }
    bundled_models
}

async fn get_auth_token(bearer: Option<String>) -> Result<Option<String>> {
    if let Some(token) = bearer {
        return Ok(Some(token));
    }
    Ok(None)
}

#[derive(Debug, Clone)]
struct PureOpenAiConfig {
    api_base: String,
    api_key: SecretString,
    bearer_token: Option<String>,
    custom_headers: HeaderMap,
}

impl PureOpenAiConfig {
    fn new(
        api_base: String,
        bearer_token: Option<String>,
        http_headers: Option<&HashMap<String, String>>,
    ) -> Result<Self> {
        let mut custom_headers = HeaderMap::new();
        if let Some(headers) = http_headers {
            for (key, value) in headers {
                let name = HeaderName::from_bytes(key.as_bytes())
                    .map_err(|e| PureError::HttpError(e.to_string()))?;
                let value = HeaderValue::from_str(value)
                    .map_err(|e| PureError::HttpError(e.to_string()))?;
                custom_headers.insert(name, value);
            }
        }

        Ok(Self {
            api_base,
            api_key: bearer_token.clone().unwrap_or_default().into(),
            bearer_token,
            custom_headers,
        })
    }
}

impl Config for PureOpenAiConfig {
    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(token) = &self.bearer_token
            && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
        {
            headers.insert(AUTHORIZATION, value);
        }

        for (key, value) in &self.custom_headers {
            headers.insert(key, value.clone());
        }

        headers
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.api_base, path)
    }

    fn query(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    fn api_base(&self) -> &str {
        &self.api_base
    }

    fn api_key(&self) -> &SecretString {
        &self.api_key
    }
}

fn openai_error_to_pure(error: OpenAIError) -> PureError {
    match error {
        OpenAIError::ApiError(api_error) => {
            PureError::LlmError(redact_secret_like_values(&format!("API error {api_error}")))
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

fn redact_secret_like_values(input: &str) -> String {
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
    use std::collections::HashMap;

    use pl_protocol::{AgentEvent, Message, MessageContent, MessageRole, TimelineItemKind};
    use pretty_assertions::assert_eq;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::protocol::openai::sse::{StreamEvent, ToolCallDeltaPayload};
    use crate::request::{CompletionRequest, CompletionTimelineContext, ToolCallPayload};
    use crate::stream::StreamCompletionAccumulator;
    use pl_protocol::TraceEventKind;

    fn apply_completed(
        accumulator: &mut StreamCompletionAccumulator,
        event_tx: &pl_protocol::AgentEventSender,
    ) {
        accumulator
            .apply(StreamEvent::Completed { response_id: None }, event_tx)
            .unwrap();
    }

    #[derive(Debug)]
    struct CapturedHttpRequest {
        request_line: String,
        headers: HashMap<String, String>,
        body: serde_json::Value,
    }

    async fn serve_sse_once(
        sse_body: String,
    ) -> (String, tokio::task::JoinHandle<CapturedHttpRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut temp = [0_u8; 1024];
            let (header_end, content_length) = loop {
                let n = socket.read(&mut temp).await.unwrap();
                assert_ne!(n, 0);
                buffer.extend_from_slice(&temp[..n]);
                if let Some(header_end) = find_header_end(&buffer) {
                    let headers = String::from_utf8_lossy(&buffer[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    break (header_end, content_length);
                }
            };

            while buffer.len() < header_end + 4 + content_length {
                let n = socket.read(&mut temp).await.unwrap();
                assert_ne!(n, 0);
                buffer.extend_from_slice(&temp[..n]);
            }

            let request_head = String::from_utf8_lossy(&buffer[..header_end]);
            let mut lines = request_head.lines();
            let request_line = lines.next().unwrap_or_default().to_string();
            let headers = lines
                .filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    Some((name.to_ascii_lowercase(), value.trim().to_string()))
                })
                .collect::<HashMap<_, _>>();
            let body_slice = &buffer[header_end + 4..header_end + 4 + content_length];
            let body = serde_json::from_slice(body_slice).unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();

            CapturedHttpRequest {
                request_line,
                headers,
                body,
            }
        });

        (format!("http://{addr}"), handle)
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn minimal_request(model: &str) -> CompletionRequest {
        CompletionRequest {
            model: model.to_string(),
            instructions: None,
            messages: vec![Message {
                role: MessageRole::User,
                content: MessageContent::Text("hello".to_string()),
                reasoning_content: None,
                metadata: HashMap::new(),
            }],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            reasoning: None,
            stream: false,
            timeline: None,
        }
    }

    #[tokio::test]
    async fn stream_complete_uses_chat_endpoint_without_auth_when_token_missing() {
        let sse_body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"<final>ok</final>\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let mut model = ModelInfo::fallback("local-chat");
        model.context_window = Some(128_000);
        let provider = OpenAiTransportProvider::new(
            ProviderInfo {
                provider_kind: crate::provider_info::ProviderKind::DeepSeek,
                name: "Local Chat".to_string(),
                base_url,
                default_model: "local-chat".to_string(),
                bearer_token: None,
                http_headers: None,
                tool_wire_policy: crate::provider_info::ToolWirePolicy::FunctionFallback,
                apply_patch_tool_type: None,
            },
            vec![model],
            Vec::new(),
            OpenAiProtocol::chat(crate::protocol::openai::ChatReasoningStyle::DeepSeek),
            ProviderCapabilities::STREAMING | ProviderCapabilities::FUNCTION_CALLING,
        )
        .unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

        let response = provider
            .stream_complete(minimal_request("local-chat"), event_tx)
            .await
            .unwrap();
        let captured = handle.await.unwrap();

        assert_eq!(response.content.as_deref(), Some("ok"));
        assert_eq!(response.usage.total_tokens, 3);
        assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
        assert!(!captured.headers.contains_key("authorization"));
        assert_eq!(captured.body["stream"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn stream_complete_sends_responses_bearer_and_custom_headers() {
        let sse_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"<final>ok</final>\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let mut model = ModelInfo::fallback("local-responses");
        model.context_window = Some(128_000);
        let provider = OpenAiTransportProvider::new(
            ProviderInfo {
                provider_kind: crate::provider_info::ProviderKind::OpenAi,
                name: "Local Responses".to_string(),
                base_url,
                bearer_token: Some("test-token".to_string()),
                http_headers: Some(HashMap::from([(
                    "x-provider-test".to_string(),
                    "present".to_string(),
                )])),
                default_model: "local-responses".to_string(),
                tool_wire_policy: crate::provider_info::ToolWirePolicy::NativeCustomTools,
                apply_patch_tool_type: Some(crate::provider_info::ApplyPatchToolType::Freeform),
            },
            vec![model],
            Vec::new(),
            OpenAiProtocol::responses(),
            ProviderCapabilities::all(),
        )
        .unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

        let response = provider
            .stream_complete(minimal_request("local-responses"), event_tx)
            .await
            .unwrap();
        let captured = handle.await.unwrap();

        assert_eq!(response.content.as_deref(), Some("ok"));
        assert_eq!(response.usage.total_tokens, 3);
        assert_eq!(captured.request_line, "POST /responses HTTP/1.1");
        assert_eq!(
            captured.headers.get("authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            captured.headers.get("x-provider-test").map(String::as_str),
            Some("present")
        );
        assert_eq!(captured.body["stream"], serde_json::json!(true));
    }

    #[test]
    fn stream_accumulator_returns_content_and_reasoning_content() {
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTimelineContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
            starting_sequence: 0,
            plan_mode: false,
        }));

        accumulator
            .apply(
                StreamEvent::ReasoningDelta {
                    item_id: None,
                    chunk_index: 0,
                    delta: "先比较整数位。".to_string(),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                StreamEvent::TextDelta {
                    item_id: None,
                    channel: None,
                    delta: "<final>9.11 更大。</final>".to_string(),
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = accumulator.finish(&event_tx).unwrap();

        assert_eq!(response.content.as_deref(), Some("9.11 更大。"));
        assert_eq!(
            response.raw_content.as_deref(),
            Some("<final>9.11 更大。</final>")
        );
        assert_eq!(
            response.reasoning_content.as_deref(),
            Some("先比较整数位。")
        );
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemStarted { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemDelta { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemStarted { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemDelta { .. }
        ));
    }

    #[test]
    fn stream_accumulator_streams_commentary_without_content() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTimelineContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
            starting_sequence: 0,
            plan_mode: false,
        }));

        for delta in [
            "<comm",
            "entary>检查配置。</commentary>",
            "<final>完成。</final>",
        ] {
            accumulator
                .apply(
                    StreamEvent::TextDelta {
                        item_id: None,
                        channel: None,
                        delta: delta.to_string(),
                    },
                    &event_tx,
                )
                .unwrap();
        }

        apply_completed(&mut accumulator, &event_tx);
        let response = accumulator.finish(&event_tx).unwrap();

        assert_eq!(response.content.as_deref(), Some("完成。"));
        assert!(response.timeline_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TimelineItemCompleted { item }
                if item.text_channel == Some(pl_protocol::TimelineTextChannel::Commentary)
                    && item.content == "检查配置。"
        )));
        assert!(response.timeline_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TimelineItemCompleted { item }
                if item.text_channel == Some(pl_protocol::TimelineTextChannel::Final)
                    && item.content == "完成。"
        )));
    }

    #[test]
    fn stream_accumulator_treats_untagged_timeline_text_as_final() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTimelineContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
            starting_sequence: 0,
            plan_mode: false,
        }));

        accumulator
            .apply(
                StreamEvent::TextDelta {
                    item_id: None,
                    channel: None,
                    delta: "plain text".to_string(),
                },
                &event_tx,
            )
            .unwrap();
        apply_completed(&mut accumulator, &event_tx);

        let response = accumulator.finish(&event_tx).unwrap();

        assert_eq!(response.content.as_deref(), Some("plain text"));
        assert!(response.timeline_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TimelineItemCompleted { item }
                if item.text_channel == Some(pl_protocol::TimelineTextChannel::Final)
                    && item.content == "plain text"
        )));
    }

    #[test]
    fn stream_accumulator_extracts_proposed_plan_item() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTimelineContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
            starting_sequence: 0,
            plan_mode: true,
        }));

        for delta in [
            "<commentary>Intro</commentary>\n<prop",
            "osed_plan>\n- step\n",
            "</proposed_plan>\n<final>Outro</final>",
        ] {
            accumulator
                .apply(
                    StreamEvent::TextDelta {
                        item_id: None,
                        channel: None,
                        delta: delta.to_string(),
                    },
                    &event_tx,
                )
                .unwrap();
        }

        apply_completed(&mut accumulator, &event_tx);
        let response = accumulator.finish(&event_tx).unwrap();

        assert_eq!(response.content.as_deref(), Some("Outro"));
        assert_eq!(
            response.raw_content.as_deref(),
            Some(
                "<commentary>Intro</commentary>\n<proposed_plan>\n- step\n</proposed_plan>\n<final>Outro</final>"
            )
        );
        let completed_plan = response
            .timeline_events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TimelineItemCompleted { item }
                    if item.kind == TimelineItemKind::Plan =>
                {
                    Some(item)
                }
                TraceEventKind::TimelineItemStarted { .. }
                | TraceEventKind::TimelineItemDelta { .. }
                | TraceEventKind::TimelineItemCompleted { .. }
                | TraceEventKind::TimelineItemFailed { .. }
                | TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            });
        assert_eq!(
            completed_plan.map(|item| item.content.as_str()),
            Some("\n- step\n")
        );
    }

    #[test]
    fn stream_accumulator_merges_chat_tool_call_chunks_by_index() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(
                StreamEvent::ToolInputDelta {
                    stream_id: Some("chat_tool_call:0".to_string()),
                    item_id: "call_1".to_string(),
                    call_id: None,
                    name: Some("read_file".to_string()),
                    payload_delta: ToolCallDeltaPayload::FunctionArguments(String::new()),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                StreamEvent::ToolInputDelta {
                    stream_id: Some("chat_tool_call:0".to_string()),
                    item_id: String::new(),
                    call_id: None,
                    name: None,
                    payload_delta: ToolCallDeltaPayload::FunctionArguments(
                        "{\"path\":\"Cargo.toml\"}".to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = accumulator.finish(&event_tx).unwrap();

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "call_1");
        assert_eq!(response.tool_calls[0].name, "read_file");
        match &response.tool_calls[0].payload {
            ToolCallPayload::Function { arguments } => {
                assert_eq!(arguments, &serde_json::json!({"path": "Cargo.toml"}));
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn stream_accumulator_requires_completed_event() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(
                StreamEvent::TextDelta {
                    item_id: None,
                    channel: None,
                    delta: "partial".to_string(),
                },
                &event_tx,
            )
            .unwrap();

        let error = accumulator.finish(&event_tx).unwrap_err();

        match error {
            PureError::LlmError(message) => {
                assert_eq!(message, "provider stream ended before completion");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn stream_accumulator_rejects_tool_delta_without_name() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(
                StreamEvent::ToolInputDelta {
                    stream_id: Some("chat_tool_call:0".to_string()),
                    item_id: "call_1".to_string(),
                    call_id: None,
                    name: None,
                    payload_delta: ToolCallDeltaPayload::FunctionArguments(
                        "{\"path\":\"Cargo.toml\"}".to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        let error = accumulator
            .apply(StreamEvent::Completed { response_id: None }, &event_tx)
            .unwrap_err();

        match error {
            PureError::LlmError(message) => {
                assert_eq!(message, "provider emitted tool call without name");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn stream_timeline_item_ids_are_scoped_to_turn() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTimelineContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
            starting_sequence: 0,
            plan_mode: false,
        }));

        accumulator
            .apply(
                StreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "call_0".to_string(),
                    call_id: Some("call_0".to_string()),
                    name: Some("bash".to_string()),
                    payload_delta: ToolCallDeltaPayload::FunctionArguments(
                        r#"{"command":"pwd"}"#.to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                StreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id: "call_0".to_string(),
                    call_id: Some("call_0".to_string()),
                    name: Some("bash".to_string()),
                    payload: Some(ToolCallDeltaPayload::FunctionArguments(
                        "{\"command\":\"pwd\"}".to_string(),
                    )),
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = accumulator.finish(&event_tx).unwrap();

        assert_eq!(response.tool_calls[0].id, "call_0");
        let item_ids = response
            .timeline_events
            .iter()
            .map(|event| match &event.kind {
                TraceEventKind::TimelineItemStarted { item }
                | TraceEventKind::TimelineItemCompleted { item } => item.item_id.as_str(),
                TraceEventKind::TimelineItemDelta { event } => event.item_id.as_str(),
                TraceEventKind::TimelineItemFailed { item, .. } => item.item_id.as_str(),
                TraceEventKind::PlanLifecycleChanged { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => "",
            })
            .filter(|item_id| !item_id.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(item_ids, vec!["turn-1-call_0", "turn-1-call_0"]);
    }

    #[test]
    fn stream_accumulator_uses_responses_added_item_name_when_done_omits_name() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(
                StreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "ctc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: Some("apply_patch".to_string()),
                    payload_delta: ToolCallDeltaPayload::CustomInput(String::new()),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                StreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "ctc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: None,
                    payload_delta: ToolCallDeltaPayload::CustomInput(
                        "*** Begin Patch\n*** End Patch".to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                StreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id: "ctc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: None,
                    payload: Some(ToolCallDeltaPayload::CustomInput(
                        "*** Begin Patch\n*** End Patch".to_string(),
                    )),
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = accumulator.finish(&event_tx).unwrap();

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "ctc_1");
        assert_eq!(response.tool_calls[0].name, "apply_patch");
        match &response.tool_calls[0].payload {
            ToolCallPayload::Custom { input } => {
                assert_eq!(input, "*** Begin Patch\n*** End Patch");
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn configured_models_override_bundled_models() {
        let mut model = ModelInfo::fallback("deepseek-v4-flash");
        model.display_name = "Custom DeepSeek".to_string();
        let provider = OpenAiTransportProvider::new(
            ProviderInfo::deepseek(None),
            vec![ModelInfo::fallback("deepseek-v4-flash")],
            vec![model],
            OpenAiProtocol::chat(crate::protocol::openai::ChatReasoningStyle::DeepSeek),
            ProviderCapabilities::STREAMING | ProviderCapabilities::FUNCTION_CALLING,
        )
        .unwrap();

        assert_eq!(
            provider.model_info("deepseek-v4-flash").display_name,
            "Custom DeepSeek"
        );
    }

    #[test]
    fn redacts_openai_api_keys_from_error_text() {
        let input = "Incorrect API key provided: sk-abc123*******************************************************xyz.";

        let redacted = redact_secret_like_values(input);

        assert_eq!(redacted, "Incorrect API key provided: [REDACTED_API_KEY].");
        assert!(!redacted.contains("sk-abc123"));
    }
}
