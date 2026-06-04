use std::collections::HashMap;
use std::time::Duration;

use async_openai::Client;
use async_openai::config::Config;
use async_openai::error::OpenAIError;
use async_openai::types::stream::StreamResponse;
use futures::StreamExt;
use pl_protocol::{
    AgentEvent, AgentEventSender, PureError, Result, TimelineDelta, TimelineItem,
    TimelineItemDeltaEvent, TimelineItemKind, TimelineItemStatus, TimelineTextRole,
    TimelineThinkingChunk, TimelineToolItem, TraceEvent, TraceEventKind,
};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use secrecy::SecretString;
use tracing::warn;

use crate::capabilities::ProviderCapabilities;
use crate::model_info::ModelInfo;
use crate::proposed_plan::{ProposedPlanParser, ProposedPlanSegment};
use crate::provider::ModelProvider;
use crate::provider_info::{ProviderInfo, WireApi};
use crate::request::{
    CompletionRequest, CompletionResponse, CompletionTimelineContext, FinishReason, TokenUsage,
    ToolCall,
};
use crate::sse::{self, StreamEvent, ToolCallDeltaPayload};
use crate::wire_api::{ProviderRequestBody, WireDispatch};

#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    info: ProviderInfo,
    http_client: reqwest::Client,
    wire_dispatch: WireDispatch,
    bundled_models: Vec<ModelInfo>,
}

impl OpenAiCompatibleProvider {
    pub fn new(info: ProviderInfo) -> Result<Self> {
        Self::with_models(info, Vec::new())
    }

    pub fn with_models(info: ProviderInfo, configured_models: Vec<ModelInfo>) -> Result<Self> {
        let wire_dispatch = match info.wire_api {
            WireApi::Responses => WireDispatch::Responses,
            WireApi::Chat if info.uses_zhipu_glm_chat_api() => WireDispatch::ZhipuChat,
            WireApi::Chat if info.uses_deepseek_chat_api() => WireDispatch::DeepSeekChat,
            WireApi::Chat => WireDispatch::Chat,
        };
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| PureError::HttpError(e.to_string()))?;

        let bundled_models =
            merge_models(crate::default_models::default_models(), configured_models);

        Ok(Self {
            info,
            http_client,
            wire_dispatch,
            bundled_models,
        })
    }

    fn resolve_base_url(&self) -> String {
        self.info
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".into())
            .trim_end_matches('/')
            .to_string()
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

impl ModelProvider for OpenAiCompatibleProvider {
    fn info(&self) -> &ProviderInfo {
        &self.info
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::all()
    }

    fn auth_token(&self) -> impl std::future::Future<Output = Result<Option<String>>> + Send {
        let bearer = self.info.bearer_token.clone();
        get_auth_token(bearer)
    }

    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send {
        let http_client = self.http_client.clone();
        let api_base = self.resolve_base_url();
        let wire_dispatch = self.wire_dispatch;
        let info = self.info.clone();
        let model_info = self.model_info(&request.model);
        async move {
            let bearer = info.bearer_token.clone();
            let token = get_auth_token(bearer).await?;

            let supports_custom_tools = info.supports_custom_tools_for_model(&model_info)
                && info.supports_freeform_tools_for_model(&model_info);
            let timeline = request.timeline.clone();
            let request = request.provider_compatible(supports_custom_tools);
            let body = wire_dispatch.build_request(&request);
            let config = PureOpenAiConfig::new(api_base, token, info.http_headers.as_ref())?;
            let client = Client::build(http_client, config);
            let stream: StreamResponse<sse::SseStreamEvent> = match body {
                ProviderRequestBody::Responses(body) => client
                    .responses()
                    .create_stream_byot(body)
                    .await
                    .map_err(openai_error_to_pure)?,
                ProviderRequestBody::Chat(body) => client
                    .chat()
                    .create_stream_byot(body)
                    .await
                    .map_err(openai_error_to_pure)?,
            };

            process_provider_stream(stream, &event_tx, &wire_dispatch, timeline).await
        }
    }

    fn model_info(&self, model: &str) -> ModelInfo {
        self.bundled_models
            .iter()
            .find(|m| m.slug == model)
            .cloned()
            .unwrap_or_else(|| ModelInfo::fallback(model))
    }

    fn default_model(&self) -> &str {
        self.info.default_model.as_str()
    }
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
        OpenAIError::ApiError(api_error) => PureError::LlmError(format!("API error {api_error}")),
        OpenAIError::Reqwest(error) => PureError::HttpError(error.to_string()),
        OpenAIError::JSONDeserialize(error, content) => {
            PureError::HttpError(format!("{error}: {content}"))
        }
        OpenAIError::StreamError(error) => PureError::HttpError(error.to_string()),
        OpenAIError::InvalidArgument(message) => PureError::ConfigError(message),
        OpenAIError::FileSaveError(message) | OpenAIError::FileReadError(message) => {
            PureError::Io(std::io::Error::other(message))
        }
    }
}

async fn process_provider_stream(
    stream: StreamResponse<sse::SseStreamEvent>,
    event_tx: &AgentEventSender,
    wire_dispatch: &WireDispatch,
    timeline: Option<CompletionTimelineContext>,
) -> Result<CompletionResponse> {
    let mut accumulator = StreamCompletionAccumulator::new(timeline);

    let mut stream = std::pin::pin!(stream);

    while let Some(event) = stream.next().await {
        let sse_event = match event {
            Ok(e) => e,
            Err(e) => {
                warn!("SSE parse error: {e}");
                break;
            }
        };

        let stream_event = match wire_dispatch.parse_stream_event(&sse_event)? {
            Some(e) => e,
            None => continue,
        };

        accumulator.apply(stream_event, event_tx)?;
    }

    Ok(accumulator.finish(event_tx))
}

struct StreamCompletionAccumulator {
    content_parts: Vec<String>,
    raw_content_parts: Vec<String>,
    reasoning_parts: Vec<String>,
    tool_calls: Vec<ToolCall>,
    tool_call_accumulators: HashMap<String, ToolCallAccumulator>,
    final_usage: Option<TokenUsage>,
    timeline: Option<TimelineState>,
    text_item_id: Option<String>,
    thinking_item_id: Option<String>,
    plan_parser: Option<ProposedPlanParser>,
}

impl StreamCompletionAccumulator {
    fn new(timeline: Option<CompletionTimelineContext>) -> Self {
        let plan_mode = timeline.as_ref().is_some_and(|context| context.plan_mode);
        Self {
            content_parts: Vec::new(),
            raw_content_parts: Vec::new(),
            reasoning_parts: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_accumulators: HashMap::new(),
            final_usage: None,
            timeline: timeline.map(TimelineState::new),
            text_item_id: None,
            thinking_item_id: None,
            plan_parser: plan_mode.then(ProposedPlanParser::new),
        }
    }

    fn apply(&mut self, stream_event: StreamEvent, event_tx: &AgentEventSender) -> Result<()> {
        match stream_event {
            StreamEvent::OutputTextDelta { item_id, delta } => {
                self.raw_content_parts.push(delta.clone());
                if let Some(parser) = self.plan_parser.as_mut() {
                    let segments = parser.push_str(&delta).segments;
                    self.apply_proposed_plan_segments(item_id, segments, event_tx);
                } else {
                    self.content_parts.push(delta.clone());
                    self.record_text_delta(item_id, delta, event_tx, TimelineTextRole::Assistant);
                }
            }

            StreamEvent::ThinkingDelta {
                item_id,
                chunk_index,
                delta,
            } => {
                self.reasoning_parts.push(delta.clone());
                self.record_thinking_delta(item_id, chunk_index, delta, event_tx);
            }

            StreamEvent::ToolCallDelta {
                stream_id,
                item_id,
                call_id,
                name,
                payload_delta,
            } => {
                let key = tool_call_accumulator_key(stream_id.as_ref(), call_id.as_ref(), &item_id);
                let initial_id = if item_id.is_empty() {
                    key.clone()
                } else {
                    item_id.clone()
                };
                let initial_call_id = call_id
                    .as_ref()
                    .filter(|call_id| !call_id.is_empty())
                    .cloned();
                let (snapshot, delta_text) = {
                    let acc = self
                        .tool_call_accumulators
                        .entry(key.clone())
                        .or_insert_with(|| ToolCallAccumulator {
                            id: initial_id,
                            call_id: initial_call_id,
                            name: String::new(),
                            payload: ToolCallPayloadAccumulator::FunctionArguments(String::new()),
                        });
                    acc.merge_metadata(&key, &item_id, call_id.as_ref(), name);
                    let delta_text = payload_delta.text().to_string();
                    acc.push_delta(payload_delta);
                    (acc.snapshot(), delta_text)
                };
                self.record_tool_delta(&snapshot, delta_text, event_tx);
            }

            StreamEvent::OutputItemDone(value) => {
                if let Some(func_call) = value.get("type").and_then(|t| t.as_str())
                    && func_call == "function_call"
                {
                    let mut call_id = value_string(&value, "call_id");
                    let mut id = value_string(&value, "id");
                    let lookup_id = id.as_deref().unwrap_or_default();
                    let acc = self.take_tool_call_accumulator(None, call_id.as_ref(), lookup_id);
                    if id.is_none() {
                        id = acc
                            .as_ref()
                            .map(|acc| acc.id.clone())
                            .or_else(|| call_id.clone());
                    }
                    if call_id.is_none() {
                        call_id = acc.as_ref().and_then(|acc| acc.call_id.clone());
                    }
                    let name = value_string(&value, "name")
                        .or_else(|| acc.as_ref().and_then(ToolCallAccumulator::name))
                        .unwrap_or_default();
                    let arguments = value_string(&value, "arguments")
                        .or_else(|| {
                            acc.as_ref()
                                .and_then(ToolCallAccumulator::function_arguments)
                        })
                        .unwrap_or_default();
                    let id = id.unwrap_or_default();
                    let call = ToolCall::function(
                        id,
                        name,
                        serde_json::from_str(&arguments)
                            .unwrap_or(serde_json::Value::String(arguments)),
                        call_id,
                    );
                    self.complete_tool_item(&call, event_tx);

                    self.tool_calls.push(call);
                } else if let Some(custom_call) = value.get("type").and_then(|t| t.as_str())
                    && custom_call == "custom_tool_call"
                {
                    let mut call_id = value_string(&value, "call_id");
                    let mut id = value_string(&value, "id");
                    let lookup_id = id.as_deref().unwrap_or_default();
                    let acc = self.take_tool_call_accumulator(None, call_id.as_ref(), lookup_id);
                    if id.is_none() {
                        id = acc
                            .as_ref()
                            .map(|acc| acc.id.clone())
                            .or_else(|| call_id.clone());
                    }
                    if call_id.is_none() {
                        call_id = acc.as_ref().and_then(|acc| acc.call_id.clone());
                    }
                    let name = value_string(&value, "name")
                        .or_else(|| acc.as_ref().and_then(ToolCallAccumulator::name))
                        .unwrap_or_default();
                    let input = value_string(&value, "input")
                        .or_else(|| acc.as_ref().and_then(ToolCallAccumulator::custom_input))
                        .unwrap_or_default();
                    let id = id.unwrap_or_default();
                    let call = ToolCall::custom(id, name, input, call_id);
                    self.complete_tool_item(&call, event_tx);

                    self.tool_calls.push(call);
                }
            }

            StreamEvent::Completed { usage, response_id } => {
                self.final_usage = usage;
                let _ = response_id;
            }

            StreamEvent::Failed { message, .. } => {
                return Err(PureError::LlmError(message));
            }

            StreamEvent::Created => {}
        }

        Ok(())
    }

    fn take_tool_call_accumulator(
        &mut self,
        stream_id: Option<&String>,
        call_id: Option<&String>,
        item_id: &str,
    ) -> Option<ToolCallAccumulator> {
        let key = tool_call_accumulator_key(stream_id, call_id, item_id);
        if self.tool_call_accumulators.contains_key(&key) {
            return self.tool_call_accumulators.remove(&key);
        }

        let fallback_key = self
            .tool_call_accumulators
            .iter()
            .find_map(|(key, accumulator)| {
                let call_id_matches = call_id
                    .filter(|call_id| !call_id.is_empty())
                    .zip(accumulator.call_id.as_ref())
                    .is_some_and(|(left, right)| left == right);
                let item_id_matches = !item_id.is_empty() && accumulator.id == item_id;
                (call_id_matches || item_id_matches).then(|| key.clone())
            });
        fallback_key.and_then(|key| self.tool_call_accumulators.remove(&key))
    }

    fn finish(mut self, event_tx: &AgentEventSender) -> CompletionResponse {
        if let Some(parser) = self.plan_parser.as_mut() {
            let segments = parser.finish().segments;
            self.apply_proposed_plan_segments(None, segments, event_tx);
        }
        // 合并累积的工具调用（如果有 delta 但没有 output_item.done）
        let remaining_accumulators = std::mem::take(&mut self.tool_call_accumulators);
        for (_, acc) in remaining_accumulators {
            if !self.tool_calls.iter().any(|tc| tc.id == acc.id) {
                let call = acc.into_tool_call();
                self.complete_tool_item_without_broadcast(&call);
                self.tool_calls.push(call);
            }
        }
        if let Some(timeline) = self.timeline.as_mut() {
            for event in timeline.complete_streaming_items() {
                let _ = event_tx.send(event);
            }
        }

        let content = if self.content_parts.is_empty() {
            None
        } else {
            Some(self.content_parts.join(""))
        };
        let raw_content = if self.raw_content_parts.is_empty() {
            None
        } else {
            Some(self.raw_content_parts.join(""))
        };

        let reasoning_content = if self.reasoning_parts.is_empty() {
            None
        } else {
            Some(self.reasoning_parts.join(""))
        };

        let finish_reason = if !self.tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        };

        let timeline_events = self
            .timeline
            .as_ref()
            .map(TimelineState::events)
            .unwrap_or_default();
        let next_sequence = self
            .timeline
            .as_ref()
            .map(TimelineState::next_sequence)
            .unwrap_or(0);

        CompletionResponse {
            content,
            raw_content,
            reasoning_content,
            tool_calls: self.tool_calls,
            timeline_events,
            next_sequence,
            usage: self.final_usage.unwrap_or_default(),
            finish_reason,
            model: String::new(),
        }
    }

    fn record_text_delta(
        &mut self,
        item_id: Option<String>,
        delta: String,
        event_tx: &AgentEventSender,
        role: TimelineTextRole,
    ) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        let item_id = item_id
            .filter(|value| !value.is_empty())
            .or_else(|| self.text_item_id.clone())
            .unwrap_or_else(|| timeline.item_id("text"));
        self.text_item_id = Some(item_id.clone());
        for event in timeline.append_text_delta(&item_id, role, delta) {
            let _ = event_tx.send(event);
        }
    }

    fn apply_proposed_plan_segments(
        &mut self,
        item_id: Option<String>,
        segments: Vec<ProposedPlanSegment>,
        event_tx: &AgentEventSender,
    ) {
        for segment in segments {
            match segment {
                ProposedPlanSegment::Normal(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    self.content_parts.push(text.clone());
                    self.record_text_delta(
                        item_id.clone(),
                        text,
                        event_tx,
                        TimelineTextRole::Assistant,
                    );
                }
                ProposedPlanSegment::ProposedPlanStart => {
                    self.record_plan_start(event_tx);
                }
                ProposedPlanSegment::ProposedPlanDelta(delta) => {
                    if !delta.is_empty() {
                        self.record_plan_delta(delta, event_tx);
                    }
                }
                ProposedPlanSegment::ProposedPlanEnd => {}
            }
        }
    }

    fn record_plan_start(&mut self, event_tx: &AgentEventSender) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        for event in timeline.append_plan_start() {
            let _ = event_tx.send(event);
        }
    }

    fn record_plan_delta(&mut self, delta: String, event_tx: &AgentEventSender) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        for event in timeline.append_plan_delta(delta) {
            let _ = event_tx.send(event);
        }
    }

    fn record_thinking_delta(
        &mut self,
        item_id: Option<String>,
        chunk_index: u32,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        let item_id = item_id
            .filter(|value| !value.is_empty())
            .or_else(|| self.thinking_item_id.clone())
            .unwrap_or_else(|| timeline.item_id("thinking"));
        self.thinking_item_id = Some(item_id.clone());
        for event in timeline.append_thinking_delta(&item_id, chunk_index, delta) {
            let _ = event_tx.send(event);
        }
    }

    fn record_tool_delta(
        &mut self,
        snapshot: &ToolCallAccumulatorSnapshot,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        for event in timeline.append_tool_arguments_delta(snapshot, delta) {
            let _ = event_tx.send(event);
        }
    }

    fn complete_tool_item(&mut self, call: &ToolCall, event_tx: &AgentEventSender) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        let event = timeline.complete_tool_call(call);
        let _ = event_tx.send(event);
    }

    fn complete_tool_item_without_broadcast(&mut self, call: &ToolCall) {
        if let Some(timeline) = self.timeline.as_mut() {
            timeline.complete_tool_call_trace_only(call);
        }
    }
}

struct ToolCallAccumulator {
    id: String,
    call_id: Option<String>,
    name: String,
    payload: ToolCallPayloadAccumulator,
}

impl ToolCallAccumulator {
    fn merge_metadata(
        &mut self,
        key: &str,
        item_id: &str,
        call_id: Option<&String>,
        name: Option<String>,
    ) {
        if !item_id.is_empty() && (self.id.is_empty() || self.id == key) {
            self.id = item_id.to_string();
        }
        if self.call_id.is_none()
            && let Some(call_id) = call_id.filter(|call_id| !call_id.is_empty())
        {
            self.call_id = Some(call_id.clone());
        }
        if let Some(name) = name
            && !name.is_empty()
        {
            self.name = name;
        }
    }

    fn push_delta(&mut self, payload_delta: ToolCallDeltaPayload) {
        match (&mut self.payload, payload_delta) {
            (
                ToolCallPayloadAccumulator::FunctionArguments(arguments),
                ToolCallDeltaPayload::FunctionArguments(delta),
            ) => arguments.push_str(&delta),
            (
                ToolCallPayloadAccumulator::CustomInput(input),
                ToolCallDeltaPayload::CustomInput(delta),
            ) => input.push_str(&delta),
            (_, ToolCallDeltaPayload::FunctionArguments(delta)) => {
                self.payload = ToolCallPayloadAccumulator::FunctionArguments(delta);
            }
            (_, ToolCallDeltaPayload::CustomInput(delta)) => {
                self.payload = ToolCallPayloadAccumulator::CustomInput(delta);
            }
        }
    }

    fn name(&self) -> Option<String> {
        (!self.name.is_empty()).then(|| self.name.clone())
    }

    fn function_arguments(&self) -> Option<String> {
        match &self.payload {
            ToolCallPayloadAccumulator::FunctionArguments(arguments) => Some(arguments.clone()),
            ToolCallPayloadAccumulator::CustomInput(_) => None,
        }
    }

    fn custom_input(&self) -> Option<String> {
        match &self.payload {
            ToolCallPayloadAccumulator::FunctionArguments(_) => None,
            ToolCallPayloadAccumulator::CustomInput(input) => Some(input.clone()),
        }
    }

    fn snapshot(&self) -> ToolCallAccumulatorSnapshot {
        ToolCallAccumulatorSnapshot {
            id: self.id.clone(),
            call_id: self.call_id.clone(),
            name: self.name.clone(),
            arguments: self.payload.text().to_string(),
        }
    }

    fn into_tool_call(self) -> ToolCall {
        match self.payload {
            ToolCallPayloadAccumulator::FunctionArguments(arguments) => ToolCall::function(
                self.id,
                self.name,
                serde_json::from_str(&arguments).unwrap_or(serde_json::Value::String(arguments)),
                self.call_id,
            ),
            ToolCallPayloadAccumulator::CustomInput(input) => {
                ToolCall::custom(self.id, self.name, input, self.call_id)
            }
        }
    }
}

enum ToolCallPayloadAccumulator {
    FunctionArguments(String),
    CustomInput(String),
}

impl ToolCallPayloadAccumulator {
    fn text(&self) -> &str {
        match self {
            Self::FunctionArguments(arguments) | Self::CustomInput(arguments) => arguments,
        }
    }
}

struct ToolCallAccumulatorSnapshot {
    id: String,
    call_id: Option<String>,
    name: String,
    arguments: String,
}

struct TimelineState {
    session_id: String,
    turn_id: String,
    inference_id: String,
    sequence: u64,
    started: HashMap<String, TimelineItem>,
    events: Vec<TraceEvent>,
}

impl TimelineState {
    fn new(context: CompletionTimelineContext) -> Self {
        Self {
            session_id: context.session_id,
            turn_id: context.turn_id,
            inference_id: context.inference_id,
            sequence: context.starting_sequence,
            started: HashMap::new(),
            events: Vec::new(),
        }
    }

    fn events(&self) -> Vec<TraceEvent> {
        self.events.clone()
    }

    fn next_sequence(&self) -> u64 {
        self.sequence
    }

    fn item_id(&self, prefix: &str) -> String {
        format!("{}-{prefix}", self.inference_id)
    }

    fn plan_item_id(&self) -> String {
        format!("{}-plan", self.turn_id)
    }

    fn namespaced_item_id(&self, item_id: &str) -> String {
        if item_id.starts_with(&self.turn_id) {
            return item_id.to_string();
        }
        format!("{}-{item_id}", self.turn_id)
    }

    fn append_text_delta(
        &mut self,
        item_id: &str,
        role: TimelineTextRole,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.namespaced_item_id(item_id);
        let mut events = Vec::new();
        if !self.started.contains_key(&item_id) {
            let item = TimelineItem {
                turn_id: self.turn_id.clone(),
                item_id: item_id.clone(),
                sequence: self.sequence,
                kind: TimelineItemKind::Text,
                status: TimelineItemStatus::Streaming,
                created_at: now,
                updated_at: now,
                role: Some(role),
                content: String::new(),
                thinking_chunks: Vec::new(),
                tool: None,
                agent: None,
                inference: None,
                usage: None,
            };
            self.record(
                TraceEventKind::TimelineItemStarted { item: item.clone() },
                now,
            );
            events.push(AgentEvent::TimelineItemStarted { item: item.clone() });
            self.started.insert(item_id.clone(), item);
        }
        if let Some(item) = self.started.get_mut(&item_id) {
            item.status = TimelineItemStatus::Streaming;
            item.updated_at = now;
            item.content.push_str(&delta);
        }
        let event = TimelineItemDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            sequence: self.sequence,
            kind: TimelineItemKind::Text,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TimelineDelta::Text { delta },
        };
        self.record(
            TraceEventKind::TimelineItemDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TimelineItemDelta { event });
        events
    }

    fn append_plan_start(&mut self) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.plan_item_id();
        if self.started.contains_key(&item_id) {
            return Vec::new();
        }
        let item = TimelineItem {
            turn_id: self.turn_id.clone(),
            item_id: item_id.clone(),
            sequence: self.sequence,
            kind: TimelineItemKind::Plan,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            role: None,
            content: String::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        };
        self.record(
            TraceEventKind::TimelineItemStarted { item: item.clone() },
            now,
        );
        self.started.insert(item_id, item.clone());
        vec![AgentEvent::TimelineItemStarted { item }]
    }

    fn append_plan_delta(&mut self, delta: String) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let mut events = self.append_plan_start();
        let item_id = self.plan_item_id();
        if let Some(item) = self.started.get_mut(&item_id) {
            item.status = TimelineItemStatus::Streaming;
            item.updated_at = now;
            item.content.push_str(&delta);
        }
        let event = TimelineItemDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            sequence: self.sequence,
            kind: TimelineItemKind::Plan,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TimelineDelta::Plan { delta },
        };
        self.record(
            TraceEventKind::TimelineItemDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TimelineItemDelta { event });
        events
    }

    fn append_thinking_delta(
        &mut self,
        item_id: &str,
        chunk_index: u32,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.namespaced_item_id(item_id);
        let mut events = Vec::new();
        if !self.started.contains_key(&item_id) {
            let item = TimelineItem {
                turn_id: self.turn_id.clone(),
                item_id: item_id.clone(),
                sequence: self.sequence,
                kind: TimelineItemKind::Thinking,
                status: TimelineItemStatus::Streaming,
                created_at: now,
                updated_at: now,
                role: None,
                content: String::new(),
                thinking_chunks: Vec::new(),
                tool: None,
                agent: None,
                inference: None,
                usage: None,
            };
            self.record(
                TraceEventKind::TimelineItemStarted { item: item.clone() },
                now,
            );
            events.push(AgentEvent::TimelineItemStarted { item: item.clone() });
            self.started.insert(item_id.clone(), item);
        }
        if let Some(item) = self.started.get_mut(&item_id) {
            item.status = TimelineItemStatus::Streaming;
            item.updated_at = now;
            match item
                .thinking_chunks
                .iter_mut()
                .find(|chunk| chunk.chunk_index == chunk_index)
            {
                Some(chunk) => chunk.content.push_str(&delta),
                None => item.thinking_chunks.push(TimelineThinkingChunk {
                    chunk_index,
                    content: delta.clone(),
                }),
            }
            item.thinking_chunks.sort_by_key(|chunk| chunk.chunk_index);
        }
        let event = TimelineItemDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            sequence: self.sequence,
            kind: TimelineItemKind::Thinking,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TimelineDelta::Thinking { chunk_index, delta },
        };
        self.record(
            TraceEventKind::TimelineItemDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TimelineItemDelta { event });
        events
    }

    fn append_tool_arguments_delta(
        &mut self,
        snapshot: &ToolCallAccumulatorSnapshot,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.namespaced_item_id(&timeline_tool_item_id(
            snapshot.call_id.as_ref(),
            &snapshot.id,
        ));
        let mut events = Vec::new();
        if !self.started.contains_key(&item_id) {
            let item = TimelineItem {
                turn_id: self.turn_id.clone(),
                item_id: item_id.clone(),
                sequence: self.sequence,
                kind: TimelineItemKind::Tool,
                status: TimelineItemStatus::Streaming,
                created_at: now,
                updated_at: now,
                role: None,
                content: String::new(),
                thinking_chunks: Vec::new(),
                tool: Some(TimelineToolItem {
                    tool_call_id: item_id.clone(),
                    call_id: snapshot.call_id.clone(),
                    provider_item_id: (!snapshot.id.is_empty()).then(|| snapshot.id.clone()),
                    name: snapshot.name.clone(),
                    arguments: String::new(),
                    result: None,
                    exit_code: None,
                    timed_out: false,
                    working_directory: None,
                    denial_reason: None,
                }),
                agent: None,
                inference: None,
                usage: None,
            };
            self.record(
                TraceEventKind::TimelineItemStarted { item: item.clone() },
                now,
            );
            events.push(AgentEvent::TimelineItemStarted { item: item.clone() });
            self.started.insert(item_id.clone(), item);
        }
        if let Some(item) = self.started.get_mut(&item_id) {
            item.status = TimelineItemStatus::Streaming;
            item.updated_at = now;
            if let Some(tool) = &mut item.tool {
                tool.name = snapshot.name.clone();
                tool.arguments = snapshot.arguments.clone();
                tool.call_id = snapshot.call_id.clone();
                tool.provider_item_id = (!snapshot.id.is_empty()).then(|| snapshot.id.clone());
            }
        }
        let event = TimelineItemDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            sequence: self.sequence,
            kind: TimelineItemKind::Tool,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TimelineDelta::ToolArguments { delta },
        };
        self.record(
            TraceEventKind::TimelineItemDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TimelineItemDelta { event });
        events
    }

    fn complete_tool_call(&mut self, call: &ToolCall) -> AgentEvent {
        let item = self.complete_tool_call_item(call, TimelineItemStatus::Started);
        let sequence = self.sequence;
        self.record(
            TraceEventKind::TimelineItemCompleted { item: item.clone() },
            item.updated_at,
        );
        AgentEvent::TimelineItemCompleted { sequence, item }
    }

    fn complete_streaming_items(&mut self) -> Vec<AgentEvent> {
        let item_ids = self
            .started
            .iter()
            .filter(|(_, item)| {
                matches!(
                    item.kind,
                    TimelineItemKind::Text | TimelineItemKind::Thinking | TimelineItemKind::Plan
                )
            })
            .map(|(item_id, _)| item_id.clone())
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for item_id in item_ids {
            let Some(item) = self.started.get_mut(&item_id) else {
                continue;
            };
            if item.status == TimelineItemStatus::Completed {
                continue;
            }
            item.status = TimelineItemStatus::Completed;
            item.updated_at = unix_seconds();
            let item = item.clone();
            let sequence = self.sequence;
            self.record(
                TraceEventKind::TimelineItemCompleted { item: item.clone() },
                item.updated_at,
            );
            events.push(AgentEvent::TimelineItemCompleted { sequence, item });
        }
        events
    }

    fn complete_tool_call_trace_only(&mut self, call: &ToolCall) {
        let item = self.complete_tool_call_item(call, TimelineItemStatus::Started);
        self.record(
            TraceEventKind::TimelineItemCompleted { item },
            unix_seconds(),
        );
    }

    fn complete_tool_call_item(
        &mut self,
        call: &ToolCall,
        status: TimelineItemStatus,
    ) -> TimelineItem {
        let now = unix_seconds();
        let item_id =
            self.namespaced_item_id(&timeline_tool_item_id(call.call_id.as_ref(), &call.id));
        let arguments = call.payload_text();
        let tool_item = TimelineToolItem {
            tool_call_id: item_id.clone(),
            call_id: call.call_id.clone(),
            provider_item_id: Some(call.id.clone()),
            name: call.name.clone(),
            arguments,
            result: None,
            exit_code: None,
            timed_out: false,
            working_directory: None,
            denial_reason: None,
        };
        let item = self
            .started
            .entry(item_id.clone())
            .or_insert_with(|| TimelineItem {
                turn_id: self.turn_id.clone(),
                item_id: item_id.clone(),
                sequence: self.sequence,
                kind: TimelineItemKind::Tool,
                status,
                created_at: now,
                updated_at: now,
                role: None,
                content: String::new(),
                thinking_chunks: Vec::new(),
                tool: Some(tool_item.clone()),
                agent: None,
                inference: None,
                usage: None,
            });
        item.status = status;
        item.updated_at = now;
        item.tool = Some(tool_item);
        item.clone()
    }

    fn record(&mut self, kind: TraceEventKind, timestamp: i64) {
        self.events.push(TraceEvent {
            session_id: self.session_id.clone(),
            sequence: self.sequence,
            timestamp,
            kind,
        });
        self.sequence += 1;
    }
}

fn timeline_tool_item_id(call_id: Option<&String>, id: &str) -> String {
    call_id
        .filter(|call_id| !call_id.is_empty())
        .cloned()
        .unwrap_or_else(|| id.to_string())
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn tool_call_accumulator_key(
    stream_id: Option<&String>,
    call_id: Option<&String>,
    item_id: &str,
) -> String {
    stream_id
        .filter(|stream_id| !stream_id.is_empty())
        .cloned()
        .or_else(|| {
            call_id
                .filter(|call_id| !call_id.is_empty())
                .map(ToOwned::to_owned)
        })
        .or_else(|| (!item_id.is_empty()).then(|| item_id.to_string()))
        .unwrap_or_else(|| "tool_call".to_string())
}

fn value_string(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{Message, MessageContent, MessageRole};
    use pretty_assertions::assert_eq;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::request::{CompletionRequest, ToolCallPayload};

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
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let provider = OpenAiCompatibleProvider::new(ProviderInfo {
            name: "Local Chat".to_string(),
            base_url: Some(base_url),
            wire_api: WireApi::Chat,
            supports_custom_tools: Some(false),
            supports_freeform_tools: Some(false),
            default_model: "local-chat".to_string(),
            ..Default::default()
        })
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
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let provider = OpenAiCompatibleProvider::new(ProviderInfo {
            name: "Local Responses".to_string(),
            base_url: Some(base_url),
            bearer_token: Some("test-token".to_string()),
            http_headers: Some(HashMap::from([(
                "x-provider-test".to_string(),
                "present".to_string(),
            )])),
            wire_api: WireApi::Responses,
            supports_custom_tools: Some(true),
            supports_freeform_tools: Some(true),
            default_model: "local-responses".to_string(),
            ..Default::default()
        })
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
                StreamEvent::ThinkingDelta {
                    item_id: None,
                    chunk_index: 0,
                    delta: "先比较整数位。".to_string(),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                StreamEvent::OutputTextDelta {
                    item_id: None,
                    delta: "9.11 更大。".to_string(),
                },
                &event_tx,
            )
            .unwrap();

        let response = accumulator.finish(&event_tx);

        assert_eq!(response.content.as_deref(), Some("9.11 更大。"));
        assert_eq!(response.raw_content.as_deref(), Some("9.11 更大。"));
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
            "Intro\n<prop",
            "osed_plan>\n- step\n",
            "</proposed_plan>\nOutro",
        ] {
            accumulator
                .apply(
                    StreamEvent::OutputTextDelta {
                        item_id: None,
                        delta: delta.to_string(),
                    },
                    &event_tx,
                )
                .unwrap();
        }

        let response = accumulator.finish(&event_tx);

        assert_eq!(response.content.as_deref(), Some("Intro\n\nOutro"));
        assert_eq!(
            response.raw_content.as_deref(),
            Some("Intro\n<proposed_plan>\n- step\n</proposed_plan>\nOutro")
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
                | TraceEventKind::TimelineItemFailed { .. } => None,
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
                StreamEvent::ToolCallDelta {
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
                StreamEvent::ToolCallDelta {
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

        let response = accumulator.finish(&event_tx);

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
                StreamEvent::ToolCallDelta {
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
                StreamEvent::OutputItemDone(serde_json::json!({
                    "type": "function_call",
                    "id": "call_0",
                    "call_id": "call_0",
                    "name": "bash",
                    "arguments": "{\"command\":\"pwd\"}"
                })),
                &event_tx,
            )
            .unwrap();

        let response = accumulator.finish(&event_tx);

        assert_eq!(response.tool_calls[0].id, "call_0");
        let item_ids = response
            .timeline_events
            .iter()
            .map(|event| match &event.kind {
                TraceEventKind::TimelineItemStarted { item }
                | TraceEventKind::TimelineItemCompleted { item } => item.item_id.as_str(),
                TraceEventKind::TimelineItemDelta { event } => event.item_id.as_str(),
                TraceEventKind::TimelineItemFailed { item, .. } => item.item_id.as_str(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            item_ids,
            vec!["turn-1-call_0", "turn-1-call_0", "turn-1-call_0"]
        );
    }

    #[test]
    fn stream_accumulator_uses_responses_added_item_name_when_done_omits_name() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(
                StreamEvent::ToolCallDelta {
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
                StreamEvent::ToolCallDelta {
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
                StreamEvent::OutputItemDone(serde_json::json!({
                    "type": "custom_tool_call",
                    "id": "ctc_1",
                    "call_id": "call_1",
                    "input": "*** Begin Patch\n*** End Patch"
                })),
                &event_tx,
            )
            .unwrap();

        let response = accumulator.finish(&event_tx);

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
    fn deepseek_provider_uses_provider_default_model() {
        let provider = OpenAiCompatibleProvider::new(ProviderInfo::deepseek(None)).unwrap();

        assert_eq!(provider.default_model(), "deepseek-v4-flash");
    }

    #[test]
    fn openai_provider_uses_provider_default_model() {
        let provider = OpenAiCompatibleProvider::new(ProviderInfo::openai(None)).unwrap();

        assert_eq!(provider.default_model(), "gpt-5.5");
    }

    #[test]
    fn deepseek_provider_uses_deepseek_chat_dispatch() {
        let provider = OpenAiCompatibleProvider::new(ProviderInfo::deepseek(None)).unwrap();

        assert_eq!(provider.wire_dispatch, WireDispatch::DeepSeekChat);
    }

    #[test]
    fn zhipu_provider_uses_zhipu_chat_dispatch() {
        let api_provider = OpenAiCompatibleProvider::new(ProviderInfo::zhipu_api(None)).unwrap();
        let coding_provider =
            OpenAiCompatibleProvider::new(ProviderInfo::zhipu_coding_plan(None)).unwrap();

        assert_eq!(api_provider.wire_dispatch, WireDispatch::ZhipuChat);
        assert_eq!(coding_provider.wire_dispatch, WireDispatch::ZhipuChat);
    }

    #[test]
    fn custom_chat_provider_uses_plain_chat_dispatch() {
        let info = ProviderInfo {
            name: "Custom Chat".to_string(),
            base_url: Some("https://example.com/v1".to_string()),
            wire_api: WireApi::Chat,
            ..Default::default()
        };
        let provider = OpenAiCompatibleProvider::new(info).unwrap();

        assert_eq!(provider.wire_dispatch, WireDispatch::Chat);
    }

    #[test]
    fn configured_models_override_bundled_models() {
        let mut model = ModelInfo::fallback("deepseek-v4-flash");
        model.display_name = "Custom DeepSeek".to_string();
        let provider =
            OpenAiCompatibleProvider::with_models(ProviderInfo::deepseek(None), vec![model])
                .unwrap();

        assert_eq!(
            provider.model_info("deepseek-v4-flash").display_name,
            "Custom DeepSeek"
        );
    }
}
