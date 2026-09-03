use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use async_openai::Client;
use async_openai::types::stream::StreamResponse;
use futures::StreamExt;
use pl_protocol::{InferenceTiming, PureError, Result};
use pl_trace::{AgentEventSender, TraceEventSink};

mod compaction;
pub(crate) mod openai;
mod provider_error;
mod responses_websocket;
mod session;
pub(crate) mod transport_policy;
mod wire_capture;

pub(crate) use provider_error::provider_stream_failure;
pub use session::ModelSession;

use crate::completion::stream::{
    CompletionEventStream, StreamCollectContext, collect_completion_event_stream,
    decode_raw_event_stream,
};
use crate::completion::tool_schema::CustomToolProjection;
use crate::completion::{
    CompletionRequest, CompletionResponse, CompletionTraceContext, ModelCompactionRequest,
    ModelCompactionResponse,
};
use crate::model::capabilities::ModelCapabilities;
use crate::model::info::ModelInfo;
use crate::provider::{ProviderConnectionMode, ProviderEndpoint, ProviderWireProtocol};
use crate::runtime::openai::sse;
use crate::runtime::openai::{OpenAiProtocol, OpenAiRequestBody, PureOpenAiConfig};
use crate::runtime::transport_policy::{
    OPENAI_HTTP_MAX_RETRIES, RESPONSES_WEBSOCKET_MAX_RETRIES, model_request_retry_delay,
};
/// 单次模型调用的运行期上下文。
///
/// 连接 continuation 属于 [`ModelSession`]；trace、事件输出和 prompt cache key
/// 只在当前调用内有效，不进入 canonical completion request。
#[derive(Debug, Clone)]
pub struct ModelInvocationContext {
    session: ModelSession,
    event_tx: AgentEventSender,
    trace: Option<CompletionTraceContext>,
    trace_sink: Option<Arc<dyn TraceEventSink>>,
    cancellation: Option<tokio_util::sync::CancellationToken>,
    prompt_cache_key: Option<String>,
}

impl ModelInvocationContext {
    pub fn new(session: ModelSession, event_tx: AgentEventSender) -> Self {
        Self {
            session,
            event_tx,
            trace: None,
            trace_sink: None,
            cancellation: None,
            prompt_cache_key: None,
        }
    }

    pub fn with_trace(
        mut self,
        trace: CompletionTraceContext,
        sink: Arc<dyn TraceEventSink>,
    ) -> Self {
        self.trace = Some(trace);
        self.trace_sink = Some(sink);
        self
    }

    pub fn with_cancellation(
        mut self,
        cancellation: Option<tokio_util::sync::CancellationToken>,
    ) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn with_prompt_cache_key(mut self, prompt_cache_key: Option<String>) -> Self {
        self.prompt_cache_key = prompt_cache_key;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ModelRuntime {
    provider_instance_id: String,
    endpoint: ProviderEndpoint,
    http_client: reqwest::Client,
    model: ModelInfo,
}

pub(crate) use provider_error::openai_error_to_pure;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAiTransport {
    ResponsesWebSocket,
    Http,
}

impl OpenAiTransport {
    fn label(self) -> &'static str {
        match self {
            Self::ResponsesWebSocket => "WebSocket",
            Self::Http => "HTTP",
        }
    }

    fn trace_label(self) -> &'static str {
        match self {
            Self::ResponsesWebSocket => "ws",
            Self::Http => "http",
        }
    }

    fn max_retries(self) -> u32 {
        match self {
            Self::ResponsesWebSocket => RESPONSES_WEBSOCKET_MAX_RETRIES,
            Self::Http => OPENAI_HTTP_MAX_RETRIES,
        }
    }
}

#[derive(Debug, Clone)]
struct InferenceTimer {
    started_at: tokio::time::Instant,
    first_token_millis: Arc<AtomicU64>,
}

impl InferenceTimer {
    const FIRST_TOKEN_UNSET: u64 = u64::MAX;

    fn start() -> Self {
        Self {
            started_at: tokio::time::Instant::now(),
            first_token_millis: Arc::new(AtomicU64::new(Self::FIRST_TOKEN_UNSET)),
        }
    }

    fn observe(&self, event: &crate::completion::stream::event::ModelStreamEvent) {
        if !event.starts_visible_output() {
            return;
        }
        let elapsed = duration_millis(self.started_at.elapsed());
        let _ = self.first_token_millis.compare_exchange(
            Self::FIRST_TOKEN_UNSET,
            elapsed,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    fn finish(&self) -> Option<InferenceTiming> {
        let total_millis = duration_millis(self.started_at.elapsed());
        let ttft_millis = self.first_token_millis.load(Ordering::Acquire);
        (ttft_millis != Self::FIRST_TOKEN_UNSET).then_some(InferenceTiming {
            ttft_millis,
            decode_millis: total_millis.saturating_sub(ttft_millis),
            total_millis,
        })
    }
}

impl ModelRuntime {
    /// 使用已解析 endpoint 与单个模型构造运行时。
    pub fn new(endpoint: ProviderEndpoint, model: ModelInfo) -> Result<Self> {
        let provider_instance_id = endpoint.name.clone();
        Self::new_with_provider_id(provider_instance_id, endpoint, model)
    }

    /// 使用稳定 Provider 实例 ID、已解析 endpoint 与单个模型构造运行时。
    pub fn new_with_provider_id(
        provider_instance_id: impl Into<String>,
        endpoint: ProviderEndpoint,
        model: ModelInfo,
    ) -> Result<Self> {
        model
            .transport
            .validate(&model.slug)
            .map_err(|error| PureError::ConfigError(error.to_string()))?;
        let provider_instance_id = provider_instance_id.into();
        if provider_instance_id.trim().is_empty() {
            return Err(PureError::ConfigError(
                "provider instance id cannot be empty".to_string(),
            ));
        }
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| PureError::HttpError(e.to_string()))?;

        Ok(Self {
            provider_instance_id,
            endpoint,
            http_client,
            model,
        })
    }

    fn resolve_base_url(&self) -> String {
        self.endpoint
            .base_url
            .clone()
            .trim_end_matches('/')
            .to_string()
    }
    pub fn endpoint(&self) -> &ProviderEndpoint {
        &self.endpoint
    }

    pub fn provider_instance_id(&self) -> &str {
        &self.provider_instance_id
    }

    pub async fn complete(
        &self,
        request: CompletionRequest,
        context: ModelInvocationContext,
    ) -> Result<CompletionResponse> {
        let inference_timer = InferenceTimer::start();
        let original_trace = context.trace.clone();
        let retry_jitter_key = original_trace
            .as_ref()
            .map(|trace| trace.inference_id.as_str())
            .unwrap_or(self.model.slug.as_str())
            .to_string();
        let mut attempt_number = 0_u32;
        let mut transport_retry_number = 0_u32;
        let transport_metrics_before = context.session.orchestration_snapshot();
        let mut http_fallbacks = 0_u64;

        loop {
            let transport = self.active_transport(&context.session);
            let trace_checkpoint = context.trace_sink.as_ref().map(|sink| sink.next_sequence());
            let max_retries = transport.max_retries();
            let attempt_request = request.clone();
            let mut trace = original_trace.clone();
            if attempt_number > 0
                && let Some(trace) = trace.as_mut()
            {
                let original_inference_id = original_trace
                    .as_ref()
                    .map(|trace| trace.inference_id.as_str())
                    .unwrap_or(trace.inference_id.as_str());
                let transport = transport.trace_label();
                trace.inference_id =
                    format!("{original_inference_id}-{transport}-retry-{attempt_number}");
            }
            let (result, retry_allowed) = self
                .run_stream_attempt(
                    attempt_request,
                    &context,
                    trace,
                    &inference_timer,
                    trace_checkpoint,
                )
                .await;
            let error = match result {
                Ok(mut response) => {
                    if response.model.is_empty() {
                        response.model = self.model.slug.clone();
                    }
                    let transport_metrics_after = context.session.orchestration_snapshot();
                    response.orchestration.transport_attempts = u64::from(attempt_number) + 1;
                    response.orchestration.continuation_attempts = transport_metrics_after
                        .continuation_attempts
                        .saturating_sub(transport_metrics_before.continuation_attempts);
                    response.orchestration.continuation_used = transport_metrics_after
                        .continuation_used
                        .saturating_sub(transport_metrics_before.continuation_used);
                    response.orchestration.continuation_invalid = transport_metrics_after
                        .continuation_invalid
                        .saturating_sub(transport_metrics_before.continuation_invalid);
                    response.orchestration.http_fallbacks = http_fallbacks;
                    response.timing = inference_timer.finish();
                    return Ok(response);
                }
                Err(error) => error,
            };
            if !retry_allowed {
                if transport == OpenAiTransport::ResponsesWebSocket
                    && error.is_transient_model_transport()
                {
                    let connection_key = self.connection_fingerprint();
                    let activated = context
                        .session
                        .activate_responses_http_fallback(connection_key)
                        .await;
                    let (provider_code, http_status) =
                        error.transient_model_metadata().unwrap_or((None, None));
                    tracing::warn!(
                        provider = %self.endpoint.name,
                        from_transport = transport.label(),
                        fallback_transport = OpenAiTransport::Http.label(),
                        fallback_reason = "partialStreamFailure",
                        fallback_scope = "nextTurn",
                        fallback_activated = activated,
                        provider_code,
                        http_status,
                        error_bytes = error.to_string().len(),
                        "Responses WebSocket 已产生事件后失败，当前请求不重放，后续请求切换到 HTTP"
                    );
                }
                return Err(error);
            }
            if !request.prepared_content.is_empty() {
                let (provider_code, http_status) =
                    error.transient_model_metadata().unwrap_or((None, None));
                tracing::warn!(
                    provider = %self.endpoint.name,
                    transport = transport.label(),
                    provider_code,
                    http_status,
                    attachment_count = request.prepared_content.len(),
                    error_bytes = error.to_string().len(),
                    "含附件的推理请求已开始，禁止自动重放"
                );
                return Err(error);
            }
            if !error.is_transient_model_transport() {
                return Err(error);
            }

            if transport_retry_number >= max_retries {
                if transport == OpenAiTransport::ResponsesWebSocket {
                    let connection_key = self.connection_fingerprint();
                    let activated = context
                        .session
                        .activate_responses_http_fallback(connection_key)
                        .await;
                    if activated {
                        http_fallbacks = http_fallbacks.saturating_add(1);
                    }
                    tracing::warn!(
                        provider = %self.endpoint.name,
                        from_transport = transport.label(),
                        fallback_transport = OpenAiTransport::Http.label(),
                        fallback_reason = "retryBudgetExhausted",
                        fallback_activated = activated,
                        retries = transport_retry_number,
                        error_bytes = error.to_string().len(),
                        "Responses WebSocket 重试预算耗尽，当前模型会话切换到 HTTP"
                    );
                    attempt_number += 1;
                    transport_retry_number = 0;
                    continue;
                }
                return Err(error);
            }

            transport_retry_number += 1;
            attempt_number += 1;
            let delay = model_request_retry_delay(
                transport_retry_number,
                error.retry_after_ms(),
                &retry_jitter_key,
            );
            let (provider_code, http_status) =
                error.transient_model_metadata().unwrap_or((None, None));
            tracing::warn!(
                provider = %self.endpoint.name,
                transport = transport.label(),
                retry_number = transport_retry_number,
                max_retries,
                delay_ms = delay.as_millis(),
                provider_code,
                http_status,
                error_bytes = error.to_string().len(),
                "模型请求遇到瞬态 provider 错误，将在同一连接模式下重放完整请求"
            );
            tokio::time::sleep(delay).await;
        }
    }

    /// 执行单次流式尝试并收集结果；返回结果与「尚未产出 canonical 事件、允许完整重放」。
    async fn run_stream_attempt(
        &self,
        request: CompletionRequest,
        context: &ModelInvocationContext,
        trace: Option<CompletionTraceContext>,
        inference_timer: &InferenceTimer,
        trace_checkpoint: Option<u64>,
    ) -> (Result<CompletionResponse>, bool) {
        match self
            .stream_events(
                request,
                context.session.clone(),
                context.prompt_cache_key.clone(),
                trace.clone(),
            )
            .await
        {
            Ok(event_stream) => {
                let stream_started = Arc::new(AtomicBool::new(false));
                let tracked_stream: CompletionEventStream = event_stream
                    .inspect({
                        let stream_started = Arc::clone(&stream_started);
                        let inference_timer = inference_timer.clone();
                        move |event| {
                            if event.is_ok() {
                                stream_started.store(true, Ordering::Release);
                            }
                            if let Ok(event) = event {
                                inference_timer.observe(event);
                            }
                        }
                    })
                    .boxed();
                let result = collect_completion_event_stream(
                    tracked_stream,
                    StreamCollectContext {
                        event_tx: &context.event_tx,
                        trace,
                        trace_sink: context.trace_sink.clone(),
                        cancellation: context.cancellation.clone(),
                    },
                )
                .await;
                // design/13：仅在模型流尚未产生任何 canonical 事件时允许完整重放。
                // 该门控对全部 transport 一致；事件一旦出现即禁止重放，避免重复输出。
                let retry_allowed = context.trace_sink.as_ref().map_or_else(
                    || !stream_started.load(Ordering::Acquire),
                    |sink| Some(sink.next_sequence()) == trace_checkpoint,
                );
                (result, retry_allowed)
            }
            Err(error) => (Err(error), true),
        }
    }

    fn active_transport(&self, session: &ModelSession) -> OpenAiTransport {
        let transport = &self.model.transport;
        let connection_key = self.connection_fingerprint();
        if transport.protocol == ProviderWireProtocol::Responses
            && transport.default_connection_mode == ProviderConnectionMode::WebSocket
            && !session.uses_responses_http_fallback(connection_key)
        {
            return OpenAiTransport::ResponsesWebSocket;
        }
        OpenAiTransport::Http
    }

    fn stream_events(
        &self,
        request: CompletionRequest,
        session: ModelSession,
        prompt_cache_key: Option<String>,
        trace: Option<CompletionTraceContext>,
    ) -> impl std::future::Future<Output = Result<CompletionEventStream>> + Send {
        let http_client = self.http_client.clone();
        let api_base = self.resolve_base_url();
        let endpoint = self.endpoint.clone();
        let model_info = self.model.clone();
        let protocol = openai_protocol(model_info.transport.protocol);
        let connection_key = self.connection_fingerprint();
        let transport = self.active_transport(&session);
        async move {
            let token = endpoint.bearer_token.clone();

            let effective_capabilities = model_info
                .capabilities
                .clone()
                .with_native_custom_tools(endpoint.uses_native_custom_tools());
            let custom_tools_native = endpoint.uses_native_custom_tools()
                && effective_capabilities.supports_custom_tools()
                && effective_capabilities.supports_freeform_tools();
            let projection = if custom_tools_native {
                CustomToolProjection::Native
            } else {
                CustomToolProjection::ToFunction
            };
            let request = request.provider_compatible(projection);
            request.validate_against(&model_info.slug, &effective_capabilities)?;
            let body =
                protocol.build_request(&request, &model_info, prompt_cache_key.as_deref())?;
            if transport == OpenAiTransport::ResponsesWebSocket {
                let OpenAiRequestBody::Responses(body) = body else {
                    return Err(PureError::ConfigError(
                        "web_socket connection mode requires the Responses API".to_string(),
                    ));
                };
                let raw_stream = responses_websocket::stream_responses(
                    responses_websocket::StreamResponsesInput {
                        api_base,
                        token,
                        provider_headers: endpoint.http_headers.as_ref(),
                        model_headers: &model_info.request_profile.headers,
                        connection_key,
                        model_session: session,
                        body,
                        trace,
                    },
                )
                .await?;
                return Ok(decode_raw_event_stream(raw_stream, protocol));
            }
            let capture = wire_capture::capture_http(&body, trace.as_ref()).await?;
            let config = PureOpenAiConfig::new(
                api_base,
                token,
                endpoint.http_headers.as_ref(),
                &model_info.request_profile.headers,
            )?;
            let client = Client::build(http_client, config);
            let stream_result: std::result::Result<
                StreamResponse<sse::SseStreamEvent>,
                async_openai::error::OpenAIError,
            > = match body {
                OpenAiRequestBody::Responses(body) => {
                    client.responses().create_stream_byot(body).await
                }
                OpenAiRequestBody::Chat(body) => client.chat().create_stream_byot(body).await,
            };
            let stream = match stream_result {
                Ok(stream) => {
                    if let Some(capture) = &capture {
                        capture.record_stage("streamOpened").await?;
                    }
                    stream
                }
                Err(error) => {
                    if let Some(capture) = &capture {
                        capture.record_stage("streamOpenFailed").await?;
                    }
                    return Err(openai_error_to_pure(error));
                }
            };

            let raw_stream = stream
                .map(|event| event.map_err(openai_error_to_pure))
                .boxed();
            let raw_stream = wire_capture::observe_http_stream(raw_stream, capture);
            Ok(decode_raw_event_stream(raw_stream, protocol))
        }
    }

    pub fn compact_context(
        &self,
        request: ModelCompactionRequest,
    ) -> impl std::future::Future<Output = Result<ModelCompactionResponse>> + Send {
        compaction::compact_context(self, request)
    }

    pub fn model(&self) -> &ModelInfo {
        &self.model
    }

    pub fn effective_model_capabilities(&self) -> ModelCapabilities {
        self.model
            .capabilities
            .clone()
            .with_native_custom_tools(self.endpoint.uses_native_custom_tools())
    }

    pub fn connection_fingerprint(&self) -> u64 {
        use std::hash::{DefaultHasher, Hash, Hasher};

        let model_info = &self.model;
        let mut hasher = DefaultHasher::new();
        self.endpoint.connection_fingerprint().hash(&mut hasher);
        model_info.slug.hash(&mut hasher);
        model_info.transport.protocol.hash(&mut hasher);
        model_info
            .transport
            .default_connection_mode
            .hash(&mut hasher);
        model_info
            .transport
            .supported_connection_modes
            .hash(&mut hasher);
        let mut headers = model_info
            .request_profile
            .headers
            .iter()
            .collect::<Vec<_>>();
        headers.sort_by(|left, right| left.0.cmp(right.0));
        for (name, value) in headers {
            name.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        let fingerprint = hasher.finish();
        if fingerprint == 0 { 1 } else { fingerprint }
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// 按 wire protocol 选择 endpoint；供应商能力由模型目录与数据化 wire policy 收敛。
fn openai_protocol(protocol: ProviderWireProtocol) -> OpenAiProtocol {
    match protocol {
        ProviderWireProtocol::Responses => OpenAiProtocol::responses(),
        ProviderWireProtocol::ChatCompletions => OpenAiProtocol::chat(),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::HashMap;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::provider::{ApplyPatchToolType, ToolWirePolicy};

    pub(crate) struct CapturedHttpRequest {
        pub(crate) request_line: String,
        pub(crate) headers: HashMap<String, String>,
        pub(crate) body: serde_json::Value,
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    /// 启动一次性 SSE 假服务器，返回 (base_url, 捕获句柄)。
    pub(crate) async fn serve_sse_once(
        sse_body: String,
    ) -> (String, tokio::task::JoinHandle<CapturedHttpRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let captured = capture_http_request(&mut socket).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
            captured
        });

        (format!("http://{addr}"), handle)
    }

    pub(crate) async fn capture_http_request(
        socket: &mut tokio::net::TcpStream,
    ) -> CapturedHttpRequest {
        let mut buffer = Vec::new();
        let mut temp = [0_u8; 1024];
        let (header_end, content_length) = loop {
            let read = socket.read(&mut temp).await.unwrap();
            assert_ne!(read, 0);
            buffer.extend_from_slice(&temp[..read]);
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
            let read = socket.read(&mut temp).await.unwrap();
            assert_ne!(read, 0);
            buffer.extend_from_slice(&temp[..read]);
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
        let body = serde_json::from_slice(&buffer[header_end + 4..header_end + 4 + content_length])
            .unwrap();

        CapturedHttpRequest {
            request_line,
            headers,
            body,
        }
    }

    pub(crate) async fn send_responses_sse(
        socket: &mut tokio::net::TcpStream,
        response_id: &str,
        message_id: &str,
        text: &str,
    ) {
        let body = format!(
            "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"{message_id}\",\"delta\":\"{text}\"}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"{response_id}\",\"model\":\"local-responses\",\"output\":[{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"{text}\"}}]}}],\"usage\":{{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}}}}\n\ndata: [DONE]\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    }

    fn websocket_bundled_model(slug: &str, websocket: bool) -> ModelInfo {
        let mut model = ModelInfo::fallback(slug);
        model.transport = if websocket {
            crate::model::ModelTransportProfile::responses_websocket()
        } else {
            crate::model::ModelTransportProfile::responses_http()
        };
        model
    }

    pub(crate) fn responses_websocket_model(slug: &str) -> ModelInfo {
        websocket_bundled_model(slug, true)
    }

    pub(crate) fn responses_http_model(slug: &str) -> ModelInfo {
        websocket_bundled_model(slug, false)
    }

    pub(crate) fn openai_provider(
        base_url: String,
        connection_mode: ProviderConnectionMode,
    ) -> ModelRuntime {
        let mut model = crate::model::default_models()
            .into_iter()
            .find(|model| model.slug == "gpt-5.5")
            .expect("bundled reasoning model");
        model.slug = "local-responses".to_string();
        model.context_window = Some(128_000);
        model.transport.default_connection_mode = connection_mode;
        ModelRuntime::new(
            ProviderEndpoint {
                name: "Local Responses".to_string(),
                base_url,
                bearer_token: Some("test-token".to_string()),
                http_headers: Some(HashMap::from([
                    ("x-provider-test".to_string(), "present".to_string()),
                    (
                        "x-codex-beta-features".to_string(),
                        "existing_feature".to_string(),
                    ),
                ])),
                tool_wire_policy: ToolWirePolicy::NativeCustomTools,
                apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
                service_capabilities: Default::default(),
            },
            model,
        )
        .unwrap()
    }

    fn user_message(role: pl_protocol::MessageRole, content: &str) -> pl_protocol::Message {
        pl_protocol::Message {
            presentation: Default::default(),
            role,
            content: pl_protocol::MessageContent::text(content.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        }
    }

    pub(crate) fn minimal_request(_model: &str) -> CompletionRequest {
        CompletionRequest::builder()
            .input(vec![
                user_message(pl_protocol::MessageRole::User, "hello").into(),
            ])
            .build()
    }

    pub(crate) fn complete_workflow_wire_request() -> CompletionRequest {
        CompletionRequest::builder()
            .instructions(
                "WORKFLOW_WIRE_BASE_SYSTEM\nWORKFLOW_WIRE_GLOBAL_DEVELOPER\nWORKFLOW_WIRE_THREAD_MODE_PROMPT\n\
                 WORKFLOW_WIRE_WORKSPACE_AGENTS\nWORKFLOW_WIRE_CONSTRAINTS",
            )
            .input(vec![user_message(
                pl_protocol::MessageRole::User,
                "WORKFLOW_WIRE_REAL_USER_PROMPT WORKFLOW_WIRE_HOT_HISTORY WORKFLOW_WIRE_CONTEXT",
            )
            .into()])
            .tools(vec![pl_protocol::ToolSpec::function(
                "workflow_current",
                "Read the canonical Thread Mode workflow state.",
                serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
            )])
            .build()
    }

    pub(crate) fn invocation(event_tx: pl_trace::AgentEventSender) -> ModelInvocationContext {
        ModelInvocationContext::new(ModelSession::default(), event_tx)
    }

    pub(crate) fn assert_complete_workflow_wire_body(body: &serde_json::Value) {
        let body = body.to_string();
        for marker in [
            "WORKFLOW_WIRE_BASE_SYSTEM",
            "WORKFLOW_WIRE_GLOBAL_DEVELOPER",
            "WORKFLOW_WIRE_THREAD_MODE_PROMPT",
            "WORKFLOW_WIRE_WORKSPACE_AGENTS",
            "WORKFLOW_WIRE_CONSTRAINTS",
            "WORKFLOW_WIRE_REAL_USER_PROMPT",
            "WORKFLOW_WIRE_HOT_HISTORY",
            "WORKFLOW_WIRE_CONTEXT",
            "workflow_current",
            "additionalProperties",
        ] {
            assert!(body.contains(marker), "final wire body is missing {marker}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    use super::test_support::*;
    use super::*;
    use crate::completion::stream::event::{ModelStreamEvent, ToolInputDeltaPayload};
    use crate::completion::stream::test_support::{trace_part_text, trace_text_channel};
    use crate::model::{ModelTransportProfile, default_models};
    use crate::provider::ToolWirePolicy;
    use pl_protocol::{Message, MessageContent, MessageRole};
    use pl_trace::{AgentEvent, TraceDelta, TraceEventKind};

    fn responses_success_sse(text: &str) -> String {
        format!(
            "data: {{\"type\":\"response.output_item.added\",\"item\":{{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}}}\n\ndata: {{\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"{text}\"}}\n\ndata: {{\"type\":\"response.output_item.done\",\"item\":{{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{{\"type\":\"output_text\",\"text\":\"{text}\"}}]}}}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp_1\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}}}}\n\ndata: [DONE]\n\n"
        )
    }

    fn chat_success_sse(text: &str) -> String {
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"<final>{text}</final>\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}}}\n\ndata: [DONE]\n\n"
        )
    }

    async fn capture_model_http_request(
        mut info: ProviderEndpoint,
        model: ModelInfo,
        sse_body: String,
    ) -> CapturedHttpRequest {
        let model_slug = model.slug.clone();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        info.base_url = base_url;
        let provider = ModelRuntime::new(info, model).unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

        provider
            .complete(minimal_request(&model_slug), invocation(event_tx))
            .await
            .unwrap();
        handle.await.unwrap()
    }

    fn local_chat_provider(base_url: String, tool_wire_policy: ToolWirePolicy) -> ModelRuntime {
        let mut model = ModelInfo::fallback("local-chat");
        model.context_window = Some(128_000);
        ModelRuntime::new(
            ProviderEndpoint {
                name: "Local Chat".to_string(),
                base_url,
                bearer_token: None,
                http_headers: None,
                tool_wire_policy,
                apply_patch_tool_type: None,
                service_capabilities: Default::default(),
            },
            model,
        )
        .unwrap()
    }

    #[test]
    fn runtime_binds_the_configured_model() {
        use pretty_assertions::assert_eq;

        let mut model = ModelInfo::fallback("deepseek-v4-flash");
        model.display_name = "Custom DeepSeek".to_string();
        let provider = ModelRuntime::new(ProviderEndpoint::deepseek(None), model).unwrap();

        assert_eq!(provider.model().display_name, "Custom DeepSeek");
    }

    #[test]
    fn chat_completions_model_rejects_websocket_before_creating_a_client() {
        let info = ProviderEndpoint::compatible("Future Chat Provider", "http://127.0.0.1:1/v1");
        let mut model = ModelInfo::fallback("future-model");
        model.transport = ModelTransportProfile {
            protocol: ProviderWireProtocol::ChatCompletions,
            supported_connection_modes: vec![ProviderConnectionMode::WebSocket],
            default_connection_mode: ProviderConnectionMode::WebSocket,
        };

        let error = ModelRuntime::new(info, model).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("chat_completions transport does not support web_socket")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn inference_timer_includes_wait_before_first_visible_delta() {
        let timer = InferenceTimer::start();

        tokio::time::advance(std::time::Duration::from_millis(250)).await;
        timer.observe(&ModelStreamEvent::ResponseStarted { response_id: None });
        timer.observe(&ModelStreamEvent::text_delta(
            "output".to_string(),
            pl_trace::TraceTextChannel::Final,
            String::new(),
        ));
        tokio::time::advance(std::time::Duration::from_millis(750)).await;
        timer.observe(&ModelStreamEvent::ReasoningRawDelta {
            id: "reasoning".to_string(),
            content_index: 0,
            delta: "thinking".to_string(),
        });
        tokio::time::advance(std::time::Duration::from_millis(500)).await;

        assert_eq!(
            timer.finish(),
            Some(pl_protocol::InferenceTiming {
                ttft_millis: 1_000,
                decode_millis: 500,
                total_millis: 1_500,
            })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn inference_timer_keeps_first_delta_and_exposes_zero_decode() {
        let timer = InferenceTimer::start();
        timer.observe(&ModelStreamEvent::ToolInputDelta {
            stream_id: None,
            item_id: "tool".to_string(),
            call_id: None,
            name: None,
            payload_delta: ToolInputDeltaPayload::CustomInput("x".to_string()),
        });
        timer.observe(&ModelStreamEvent::text_delta(
            "output".to_string(),
            pl_trace::TraceTextChannel::Final,
            "later".to_string(),
        ));

        let timing = timer.finish().expect("first token timing");
        assert_eq!(timing.ttft_millis, 0);
        assert_eq!(timing.decode_millis, 0);
        assert!(!timing.has_throughput_sample());
    }

    #[tokio::test]
    async fn responses_http_terminal_server_error_preserves_retry_metadata() {
        use pretty_assertions::assert_eq;

        let body = concat!(
            "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_failed\",",
            "\"error\":{\"code\":\"server_error\",\"message\":\"temporary upstream failure\"}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, server) = serve_sse_once(body).await;
        let provider = openai_provider(base_url, ProviderConnectionMode::Http);
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

        let error = provider
            .complete(minimal_request("local-responses"), invocation(event_tx))
            .await
            .expect_err("response.failed must surface a typed provider failure");
        server.await.unwrap();

        let failure = error
            .provider_failure_ref()
            .expect("response.failed must preserve provider metadata");
        assert_eq!(failure.kind, pl_protocol::ProviderFailureKind::Capacity);
        assert_eq!(failure.code.as_deref(), Some("server_error"));
        assert_eq!(failure.http_status, None);
        assert!(failure.retry.is_retryable());
    }

    #[tokio::test]
    async fn model_transport_matrix_selects_http_endpoint_per_model() {
        use pretty_assertions::assert_eq;

        let find_model = |slug: &str| {
            default_models()
                .into_iter()
                .find(|model| model.slug == slug)
                .unwrap()
        };

        let glm = capture_model_http_request(
            ProviderEndpoint::zhipu(None),
            find_model("glm-5.2"),
            chat_success_sse("glm ok"),
        )
        .await;
        let mimo = capture_model_http_request(
            ProviderEndpoint::compatible("MiMo", "https://api.xiaomimimo.com/v1"),
            find_model("mimo-v2.5"),
            chat_success_sse("mimo ok"),
        )
        .await;
        let flash = capture_model_http_request(
            ProviderEndpoint::deepseek(None),
            find_model("deepseek-v4-flash"),
            responses_success_sse("flash ok"),
        )
        .await;
        let pro = capture_model_http_request(
            ProviderEndpoint::deepseek(None),
            find_model("deepseek-v4-pro"),
            responses_success_sse("pro ok"),
        )
        .await;
        let mut gpt_model = find_model("gpt-5.6-sol");
        gpt_model.transport.default_connection_mode = ProviderConnectionMode::Http;
        let gpt = capture_model_http_request(
            ProviderEndpoint::openai(None),
            gpt_model,
            responses_success_sse("gpt ok"),
        )
        .await;

        assert_eq!(glm.request_line, "POST /chat/completions HTTP/1.1");
        assert_eq!(mimo.request_line, "POST /chat/completions HTTP/1.1");
        assert_eq!(flash.request_line, "POST /responses HTTP/1.1");
        assert_eq!(pro.request_line, "POST /responses HTTP/1.1");
        assert_eq!(gpt.request_line, "POST /responses HTTP/1.1");
    }

    #[tokio::test]
    async fn stream_complete_uses_chat_endpoint_without_auth_when_token_missing() {
        use pretty_assertions::assert_eq;

        let sse_body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"<final>ok</final>\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let provider = local_chat_provider(base_url, ToolWirePolicy::FunctionFallback);
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

        let response = provider
            .complete(
                complete_workflow_wire_request(),
                invocation(event_tx)
                    .with_prompt_cache_key(Some("must-not-cross-chat-wire".to_string())),
            )
            .await
            .unwrap();
        let captured = handle.await.unwrap();

        assert_eq!(response.content.as_deref(), Some("ok"));
        assert_eq!(response.usage.total_tokens, 3);
        assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
        assert!(!captured.headers.contains_key("authorization"));
        assert_eq!(captured.body["stream"], serde_json::json!(true));
        assert!(captured.body.get("prompt_cache_key").is_none());
        assert_complete_workflow_wire_body(&captured.body);
    }

    #[tokio::test]
    async fn openai_compatible_chat_provider_uses_chat_endpoint() {
        use pretty_assertions::assert_eq;

        let sse_body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"<final>mimo ok</final>\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let mut model = ModelInfo::fallback("mimo-chat");
        model.context_window = Some(128_000);
        let provider =
            ModelRuntime::new(ProviderEndpoint::compatible("MiMo", base_url), model).unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

        let response = provider
            .complete(minimal_request("mimo-chat"), invocation(event_tx))
            .await
            .unwrap();
        let captured = handle.await.unwrap();

        assert_eq!(response.content.as_deref(), Some("mimo ok"));
        assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
    }

    #[tokio::test]
    async fn stream_complete_chat_tags_project_commentary_and_final_only() {
        use pretty_assertions::assert_eq;

        let sse_body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"<commentary>检查配置。</commentary>\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"<final>Ready</final>\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let provider = local_chat_provider(base_url, ToolWirePolicy::FunctionFallback);
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
        let trace_sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 0));
        let context = invocation(event_tx).with_trace(
            CompletionTraceContext {
                session_id: "session-1".to_string(),
                turn_id: "turn-1".to_string(),
                inference_id: "inf-1".to_string(),
            },
            trace_sink.clone(),
        );

        let response = provider
            .complete(minimal_request("local-chat"), context)
            .await
            .unwrap();
        let trace_events = trace_sink.events();
        let captured = handle.await.unwrap();

        assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
        assert_eq!(response.content.as_deref(), Some("Ready"));
        assert!(trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if trace_text_channel(item) == Some(pl_trace::TraceTextChannel::Commentary)
                    && trace_part_text(item) == "检查配置。"
        )));
        assert!(trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if trace_text_channel(item) == Some(pl_trace::TraceTextChannel::Final)
                    && trace_part_text(item) == "Ready"
        )));
    }

    #[tokio::test]
    async fn gated_responses_sse_publishes_delta_before_next_frame_and_completion() {
        use pretty_assertions::assert_eq;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (first_frame_tx, first_frame_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let first = concat!(
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"a\"}\n\n"
        );
        let rest = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"b\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ab\"}]}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        );
        let content_length = first.len() + rest.len();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let captured = capture_http_request(&mut socket).await;
            let header = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {content_length}\r\nconnection: close\r\n\r\n"
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(first.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            first_frame_tx.send(()).unwrap();
            release_rx.await.unwrap();
            socket.write_all(rest.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
            captured
        });

        let provider =
            openai_provider(format!("http://{address}/v1"), ProviderConnectionMode::Http);
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 0));
        let context = invocation(event_tx).with_trace(
            CompletionTraceContext {
                session_id: "session-1".to_string(),
                turn_id: "turn-1".to_string(),
                inference_id: "inf-1".to_string(),
            },
            sink.clone(),
        );
        let completion = tokio::spawn(async move {
            provider
                .complete(minimal_request("local-responses"), context)
                .await
        });

        first_frame_rx.await.unwrap();
        let first_delta = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if let AgentEvent::TracePartDelta { event } = event_rx.recv().await.unwrap()
                    && matches!(&event.delta, TraceDelta::Text { delta, .. } if delta == "a")
                {
                    break event;
                }
            }
        })
        .await
        .expect("first delta must publish while the provider is gated");
        assert_eq!(first_delta.revision, 1);
        assert!(!completion.is_finished());
        assert!(sink.events().iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartDelta { event }
                if matches!(&event.delta, TraceDelta::Text { delta, .. } if delta == "a")
        )));

        release_tx.send(()).unwrap();
        let response = completion.await.unwrap().unwrap();
        let captured = server.await.unwrap();
        assert_eq!(response.content.as_deref(), Some("ab"));
        assert_eq!(captured.request_line, "POST /v1/responses HTTP/1.1");
    }

    #[tokio::test]
    async fn stream_complete_sends_responses_bearer_and_custom_headers() {
        use crate::provider::ApplyPatchToolType;
        use pretty_assertions::assert_eq;

        let sse_body = concat!(
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let mut model = responses_http_model("local-responses");
        model.context_window = Some(128_000);
        let provider = ModelRuntime::new(
            ProviderEndpoint {
                name: "Local Responses".to_string(),
                base_url,
                bearer_token: Some("test-token".to_string()),
                http_headers: Some(HashMap::from([(
                    "x-provider-test".to_string(),
                    "present".to_string(),
                )])),
                tool_wire_policy: ToolWirePolicy::NativeCustomTools,
                apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
                service_capabilities: Default::default(),
            },
            model,
        )
        .unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

        let response = provider
            .complete(complete_workflow_wire_request(), invocation(event_tx))
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
        assert_complete_workflow_wire_body(&captured.body);
    }

    #[tokio::test]
    async fn http_retries_transient_request_failures_before_the_stream_starts() {
        use pretty_assertions::assert_eq;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for attempt in 0..=OPENAI_HTTP_MAX_RETRIES {
                let (mut socket, _) = listener.accept().await.unwrap();
                requests.push(capture_http_request(&mut socket).await);
                if attempt < OPENAI_HTTP_MAX_RETRIES {
                    socket.shutdown().await.unwrap();
                } else {
                    send_responses_sse(&mut socket, "http-response", "http-message", "http-ok")
                        .await;
                }
            }
            requests
        });

        let provider =
            openai_provider(format!("http://{address}/v1"), ProviderConnectionMode::Http);
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let response = provider
            .complete(minimal_request("local-responses"), invocation(event_tx))
            .await
            .unwrap();
        let requests = server.await.unwrap();

        assert_eq!(response.content.as_deref(), Some("http-ok"));
        assert_eq!(requests.len(), 1 + OPENAI_HTTP_MAX_RETRIES as usize);
        assert!(
            requests
                .iter()
                .all(|request| request.request_line == "POST /v1/responses HTTP/1.1")
        );
    }

    #[tokio::test]
    async fn http_does_not_replay_media_requests_after_the_first_send() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let first = capture_http_request(&mut socket).await;
            socket.shutdown().await.unwrap();
            let replayed =
                tokio::time::timeout(std::time::Duration::from_millis(750), listener.accept())
                    .await
                    .is_ok();
            (first, replayed)
        });

        let model = default_models()
            .into_iter()
            .find(|model| model.slug == "glm-5.3-flash")
            .unwrap();
        let mut endpoint = ProviderEndpoint::zhipu(Some(format!("http://{address}/v1")));
        endpoint.bearer_token = Some("test-token".to_string());
        let provider = ModelRuntime::new(endpoint, model).unwrap();
        let attachment_id = "attachment-1".to_string();
        let request = CompletionRequest::builder()
            .input(vec![
                Message {
                    presentation: Default::default(),
                    role: MessageRole::User,
                    content: MessageContent::new(vec![
                        pl_protocol::ContentPart::Text {
                            text: "inspect".to_string(),
                        },
                        pl_protocol::ContentPart::Attachment {
                            attachment_id: attachment_id.clone(),
                            modality: pl_protocol::AttachmentModality::Image,
                            media_type: "image/png".to_string(),
                            filename: Some("marker.png".to_string()),
                        },
                    ]),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_result: None,
                    metadata: HashMap::new(),
                }
                .into(),
            ])
            .prepared_content(vec![crate::completion::PreparedContentPart {
                attachment_id,
                modality: pl_protocol::AttachmentModality::Image,
                media_type: "image/png".to_string(),
                filename: Some("marker.png".to_string()),
                sources: vec![crate::completion::PreparedContentSource::DataUrl {
                    base64: "aW1hZ2U=".to_string(),
                }],
            }])
            .build();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

        provider
            .complete(request, invocation(event_tx))
            .await
            .expect_err("a failed media request must surface without automatic replay");
        let (first, replayed) = server.await.unwrap();

        assert_eq!(first.request_line, "POST /v1/chat/completions HTTP/1.1");
        assert!(!replayed, "the media inference request was replayed");
    }
}
