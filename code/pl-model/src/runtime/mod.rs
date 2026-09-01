use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use async_openai::Client;
use async_openai::config::Config;
use async_openai::types::stream::StreamResponse;
use futures::StreamExt;
use pl_protocol::{InferenceTiming, PureError, Result};
use pl_trace::{AgentEventSender, TraceEventSink};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use secrecy::SecretString;

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
    CompletionEventStream, collect_completion_event_stream, decode_raw_event_stream,
};
use crate::completion::{
    CompletionRequest, CompletionResponse, CompletionTraceContext, ModelCompactionRequest,
    ModelCompactionResponse,
};
use crate::model::capabilities::ModelCapabilities;
use crate::model::info::ModelInfo;
use crate::provider::{ProviderConnectionMode, ProviderEndpoint, ProviderWireProtocol};
use crate::runtime::openai::sse;
use crate::runtime::openai::{OpenAiProtocol, OpenAiRequestBody};
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
            .map_err(PureError::ConfigError)?;
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
            let (result, retry_allowed) = match self
                .stream_events(
                    attempt_request,
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
                        &context.event_tx,
                        trace,
                        context.trace_sink.clone(),
                        context.cancellation.clone(),
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
            };
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
            let supports_custom_tools = endpoint.uses_native_custom_tools()
                && effective_capabilities.supports_custom_tools()
                && effective_capabilities.supports_freeform_tools();
            let request = request.provider_compatible(supports_custom_tools);
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
        model_headers: &HashMap<String, String>,
    ) -> Result<Self> {
        let mut custom_headers = HeaderMap::new();
        if let Some(headers) = http_headers {
            for (key, value) in headers {
                insert_header(&mut custom_headers, key, value)?;
            }
        }
        for (key, value) in model_headers {
            insert_header(&mut custom_headers, key, value)?;
        }

        Ok(Self {
            api_base,
            api_key: bearer_token.clone().unwrap_or_default().into(),
            bearer_token,
            custom_headers,
        })
    }
}

fn insert_header(headers: &mut HeaderMap, key: &str, value: &str) -> Result<()> {
    let name = HeaderName::from_bytes(key.as_bytes())
        .map_err(|error| PureError::HttpError(error.to_string()))?;
    let value =
        HeaderValue::from_str(value).map_err(|error| PureError::HttpError(error.to_string()))?;
    headers.insert(name, value);
    Ok(())
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
        let base = &self.api_base;
        format!("{base}{path}")
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

#[cfg(test)]
mod unit_tests;
