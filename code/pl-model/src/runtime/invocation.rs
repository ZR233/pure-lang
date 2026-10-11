use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use futures::StreamExt;
use pl_protocol::trace::{AgentEventSender, TraceEventSink};
use pl_protocol::{InferenceModelObservation, InferenceTiming, PureError, Result};

use super::{ModelSession, responses_websocket, wire_capture};

use crate::completion::stream::{
    CompletionEventStream, StreamCollectContext, decode_raw_event_stream,
};
use crate::completion::{
    CompletionFailure, CompletionRequest, CompletionResponse, CompletionTraceContext,
};
use crate::model::capabilities::ModelCapabilities;
use crate::model::info::ModelInfo;
use crate::provider::{ProviderConnectionMode, ProviderEndpoint, ProviderWireProtocol};
use crate::runtime::openai::{OpenAiProtocol, OpenAiRequestBody};
use crate::runtime::transport_policy::{
    MODEL_MAX_RETRIES, RESPONSES_WEBSOCKET_IDLE_TIMEOUT, RESPONSES_WEBSOCKET_MAX_RETRIES,
    model_request_retry_delay,
};

struct OpenedCompletionStream {
    events: CompletionEventStream,
    sent_model: String,
    idle_timeout: Option<Duration>,
}
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
    progress: Option<pl_core::model::ModelProgressSender>,
}

impl ModelInvocationContext {
    /// Binds core request identities for wire diagnostics without a parallel event sink.
    pub fn with_trace_metadata(mut self, trace: CompletionTraceContext) -> Self {
        self.trace = Some(trace);
        self
    }

    pub(super) fn with_progress(
        mut self,
        progress: Option<pl_core::model::ModelProgressSender>,
    ) -> Self {
        self.progress = progress;
        self
    }

    pub(super) fn prompt_cache_key(&self) -> Option<String> {
        self.prompt_cache_key.clone()
    }

    pub(super) fn with_session(mut self, session: ModelSession) -> Self {
        self.session = session;
        self
    }

    pub fn new(session: ModelSession) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        Self {
            session,
            event_tx,
            trace: None,
            trace_sink: None,
            cancellation: None,
            prompt_cache_key: None,
            progress: None,
        }
    }

    /// Attaches an event consumer when the host needs streaming projections.
    pub fn with_events(mut self, event_tx: AgentEventSender) -> Self {
        self.event_tx = event_tx;
        self
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
    fn begin_retry(&self, error: &mut CompletionFailure) -> Result<()> {
        let sender = self
            .progress
            .as_ref()
            .expect("invocation owns its observation");
        let message = error.to_string();
        sender
            .charge_output(
                sender
                    .charged_output()
                    .saturating_add(message.len() as u64)
                    .saturating_add(256),
            )
            .map_err(|error| PureError::MemoryError(error.to_string()))?;
        sender
            .begin_recovery(MODEL_MAX_RETRIES, Arc::from(message))
            .map_err(|error| PureError::Protocol(error.to_string()))?;
        // These parts now belong solely to the retained failed generation. A
        // cancellation/refresh failure before the next send must not relabel
        // them as that unsent generation's current presentation.
        error.presentation_items.clear();
        Ok(())
    }
}

impl Default for ModelInvocationContext {
    fn default() -> Self {
        Self::new(ModelSession::default())
    }
}

#[derive(Clone)]
pub(crate) struct InvocationRunner {
    provider_instance_id: String,
    endpoint: ProviderEndpoint,
    pub(super) http_client: reqwest::Client,
    model: ModelInfo,
    native_body: serde_json::Map<String, serde_json::Value>,
    purpose: InvocationPurpose,
    pub(crate) clock: Arc<dyn super::InferenceClock>,
    pub(crate) pricing_mode: pl_protocol::PricingMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvocationPurpose {
    Completion,
    RemoteCompaction,
}

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

    fn decode_millis(&self) -> Option<u64> {
        let ttft_millis = self.first_token_millis.load(Ordering::Acquire);
        (ttft_millis != Self::FIRST_TOKEN_UNSET)
            .then(|| duration_millis(self.started_at.elapsed()).saturating_sub(ttft_millis))
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

impl std::fmt::Debug for InvocationRunner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InvocationRunner")
            .field("provider", &self.provider_instance_id)
            .field("model", &self.model.slug)
            .field("transport", &self.model.binding.transport)
            .field("pricing_mode", &self.pricing_mode)
            .finish_non_exhaustive()
    }
}

impl InvocationRunner {
    /// 使用已解析 endpoint 与单个模型构造运行时。
    /// 使用稳定 Provider 实例 ID、已解析 endpoint 与单个模型构造运行时。
    pub fn new_with_provider_id(
        provider_instance_id: impl Into<String>,
        endpoint: ProviderEndpoint,
        model: ModelInfo,
    ) -> Result<Self> {
        model
            .validate()
            .map_err(|error| PureError::ConfigError(error.to_string()))?;
        let provider_instance_id = provider_instance_id.into();
        if provider_instance_id.trim().is_empty() {
            return Err(PureError::ConfigError(
                "provider instance id cannot be empty".to_string(),
            ));
        }
        let http_client = reqwest::Client::builder()
            .retry(reqwest::retry::never())
            .connect_timeout(super::transport_policy::HTTP_CONNECT_TIMEOUT)
            .read_timeout(super::transport_policy::HTTP_READ_IDLE_TIMEOUT)
            .build()
            .map_err(|e| PureError::HttpError(e.to_string()))?;

        Ok(Self {
            provider_instance_id,
            endpoint,
            http_client,
            model,
            native_body: Default::default(),
            purpose: InvocationPurpose::Completion,
            clock: Arc::new(super::clock::SystemInferenceClock),
            pricing_mode: pl_protocol::PricingMode::Catalog,
        })
    }

    pub(super) fn resolve_base_url(&self) -> String {
        self.endpoint
            .base_url
            .clone()
            .trim_end_matches('/')
            .to_string()
    }
    pub fn endpoint(&self) -> &ProviderEndpoint {
        &self.endpoint
    }

    pub(crate) fn with_native_body(
        &self,
        body: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        let mut runner = self.clone();
        runner.native_body = body;
        runner
    }

    pub(super) fn for_compaction(
        &self,
        headers: HashMap<String, String>,
        body: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        let mut runner = self.with_native_body(body);
        runner.model.binding.transport.default_connection_mode = ProviderConnectionMode::Http;
        runner.model.binding.request.headers = headers;
        runner.purpose = InvocationPurpose::RemoteCompaction;
        runner
    }

    pub fn provider_instance_id(&self) -> &str {
        &self.provider_instance_id
    }

    pub async fn complete(
        &self,
        request: CompletionRequest,
        mut context: ModelInvocationContext,
    ) -> std::result::Result<CompletionResponse, CompletionFailure> {
        let progress = context
            .progress
            .get_or_insert_with(|| {
                pl_core::model::ModelProgressSender::detached(
                    crate::completion::stream::MAX_COMPLETION_OUTPUT_BYTES as u64,
                )
            })
            .clone();
        // A Thread-owned channel is initialized with its stable turn/attempt live marker. Do not
        // replace it with a default snapshot here: doing so would erase the identity before the
        // first provider event and make a reused invocation look like an unowned observation.
        let caller_cancellation = context.cancellation.take().unwrap_or_default();
        let closing = context.session.closing_token();
        let cancellation = tokio_util::sync::CancellationToken::new();
        context.cancellation = Some(cancellation.clone());
        let invocation = self.complete_inner(request, context);
        tokio::pin!(invocation);
        // Cancellation is cooperative: await the same invocation so its partial
        // accounting and transport guards finish before releasing admission.
        let mut result = tokio::select! {
            biased;
            _ = caller_cancellation.cancelled() => {
                cancellation.cancel();
                invocation.await
            }
            _ = closing.cancelled() => {
                cancellation.cancel();
                invocation.await
            }
            result = &mut invocation => result,
        };
        match &mut result {
            Ok(response) => {
                progress.finish_recovery(pl_core::model::ModelRecoveryPhase::Recovered);
                response.observation = progress.latest().observation().clone();
            }
            Err(error) => {
                let phase = if error.is_cancelled() {
                    pl_core::model::ModelRecoveryPhase::Cancelled
                } else if progress
                    .latest()
                    .observation()
                    .recovery
                    .is_some_and(|status| {
                        status.phase == pl_core::model::ModelRecoveryPhase::Exhausted
                    })
                {
                    pl_core::model::ModelRecoveryPhase::Exhausted
                } else {
                    pl_core::model::ModelRecoveryPhase::Failed
                };
                progress.finish_recovery(phase);
                let latest = progress.latest();
                error.partial_progress = (!latest.is_empty()).then_some(Box::new(latest));
            }
        }
        result
    }

    async fn complete_inner(
        &self,
        request: CompletionRequest,
        context: ModelInvocationContext,
    ) -> std::result::Result<CompletionResponse, CompletionFailure> {
        super::context::validate(self.model.binding.transport.protocol, &request.input)?;
        let _session_lease = if let Some(cancellation) = &context.cancellation {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(CompletionFailure::cancelled(
                    PureError::LlmError("model invocation cancelled before admission".into()),
                    Box::default(),
                )),
                lease = context.session.admit() => lease?,
            }
        } else {
            context.session.admit().await?
        };
        let mut request = super::context::project_request(
            &self.endpoint,
            &self.model,
            &self.provider_instance_id,
            request,
        )?;
        let inference_timer = InferenceTimer::start();
        let original_trace = context.trace.clone();
        let retry_jitter_key = original_trace
            .as_ref()
            .map(|trace| trace.inference_id.as_str())
            .unwrap_or(self.model.slug.as_str())
            .to_string();
        let mut attempt_number = 0_u32;
        let mut transport_attempts = 0_u64;
        let mut attachments_prepared = false;
        let transport_metrics_before = context.session.orchestration_snapshot();
        let mut http_fallbacks = 0_u64;
        let mut refreshed_attachments = false;
        let mut last_model_observation = None;

        loop {
            if context
                .cancellation
                .as_ref()
                .is_some_and(|token| token.is_cancelled())
            {
                return Err(CompletionFailure::cancelled(
                    PureError::LlmError("model invocation cancelled".into()),
                    Box::default(),
                )
                .with_optional_model_observation(last_model_observation));
            }
            let transport = self.active_transport(&context.session);
            let max_retries = MODEL_MAX_RETRIES;
            let preparation = if attachments_prepared {
                Ok(())
            } else {
                self.prepare_attachments(&mut request, &context).await
            };
            let attempted_stream = preparation.is_ok();
            let mut attempt_request = request.clone();
            if attempt_number > 0 {
                for media in &mut attempt_request.prepared_content {
                    media.sources.retain(|source| {
                        !matches!(
                            source,
                            crate::completion::AttachmentRepresentation::RemoteUrl { .. }
                        )
                    });
                }
            }
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
            let request_started_at = self.clock.unix_seconds()?;
            let (result, retry_allowed) = match preparation {
                Ok(()) => {
                    attachments_prepared = true;
                    transport_attempts += 1;
                    self.run_stream_attempt(attempt_request, &context, trace, &inference_timer)
                        .await
                }
                Err(error) => (Err(error), true),
            };
            let mut error = match result {
                Ok(mut response) => {
                    let transport_metrics_after = context.session.orchestration_snapshot();
                    response.orchestration.transport_attempts = transport_attempts;
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
                    response.accounting = self.model.pricing.account(
                        response.accounting.usage,
                        self.pricing_mode,
                        request_started_at,
                    );
                    return Ok(response);
                }
                Err(mut error) => {
                    error.accounting = Box::new(self.model.pricing.account(
                        error.accounting.usage,
                        self.pricing_mode,
                        request_started_at,
                    ));
                    error
                }
            };
            last_model_observation = error
                .model_observation()
                .cloned()
                .or(last_model_observation);
            if error.is_cancelled() {
                return Err(error);
            }
            if !retry_allowed {
                if transport == OpenAiTransport::ResponsesWebSocket
                    && error.is_transient_model_transport()
                    && self.supports_http_fallback()
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
            let recovery = error
                .source
                .provider_failure_ref()
                .map(|failure| failure.context.recovery)
                .unwrap_or_default();
            if recovery == pl_protocol::ProviderRecovery::HttpFallback
                && transport == OpenAiTransport::ResponsesWebSocket
                && self.supports_http_fallback()
                && attempt_number < max_retries
            {
                if context
                    .session
                    .activate_responses_http_fallback(self.connection_fingerprint())
                    .await
                {
                    http_fallbacks += 1;
                }
                attempt_number += 1;
                if let Err(cause) = context.begin_retry(&mut error) {
                    return Err(error.with_source(cause));
                }
                continue;
            }
            if recovery == pl_protocol::ProviderRecovery::RefreshAttachments
                && !refreshed_attachments
                && attempt_number < max_retries
                && request.prepared_content.iter().any(|part| {
                    part.sources.iter().any(|source| {
                        matches!(
                            source,
                            crate::completion::AttachmentRepresentation::ProviderFile { .. }
                        )
                    })
                })
            {
                attempt_number += 1;
                if let Err(cause) = context.begin_retry(&mut error) {
                    return Err(error.with_source(cause));
                }
                context
                    .session
                    .uploaded_files
                    .lock()
                    .await
                    .retain(|(fingerprint, _), _| *fingerprint != self.connection_fingerprint());
                attachments_prepared = false;
                refreshed_attachments = true;
                continue;
            }
            if !error.is_transient_model_transport() {
                return Err(error);
            }

            if attempt_number >= max_retries {
                if let Some(progress) = &context.progress {
                    progress.finish_recovery(pl_core::model::ModelRecoveryPhase::Exhausted);
                }
                return Err(error);
            }
            // WS 重连一次仍失败后切换 HTTP；切换不重置逻辑请求的总预算。
            if transport == OpenAiTransport::ResponsesWebSocket
                && attempted_stream
                && attempt_number >= RESPONSES_WEBSOCKET_MAX_RETRIES
                && self.supports_http_fallback()
            {
                let activated = context
                    .session
                    .activate_responses_http_fallback(self.connection_fingerprint())
                    .await;
                if activated {
                    http_fallbacks = http_fallbacks.saturating_add(1);
                }
            }
            attempt_number += 1;
            if let Err(cause) = context.begin_retry(&mut error) {
                return Err(error.with_source(cause));
            }
            let delay = model_request_retry_delay(
                attempt_number,
                error.retry_after_ms(),
                &retry_jitter_key,
            );
            let (provider_code, http_status) =
                error.transient_model_metadata().unwrap_or((None, None));
            tracing::warn!(
                provider = %self.endpoint.name,
                transport = transport.label(),
                retry_number = attempt_number,
                max_retries,
                delay_ms = delay.as_millis(),
                provider_code,
                http_status,
                error_bytes = error.to_string().len(),
                "模型连接中断，将在统一预算内重试当前请求"
            );
            if let Some(token) = &context.cancellation {
                let model_observation = error.model_observation().cloned();
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {},
                    _ = token.cancelled() => {
                        let mut cancelled = CompletionFailure::cancelled(
                            PureError::LlmError("model invocation cancelled".into()),
                            error.accounting,
                        ).with_optional_model_observation(model_observation);
                        cancelled.presentation_items = error.presentation_items;
                        return Err(cancelled);
                    },
                }
            } else {
                tokio::time::sleep(delay).await;
            }
        }
    }

    async fn prepare_attachments(
        &self,
        request: &mut CompletionRequest,
        context: &ModelInvocationContext,
    ) -> std::result::Result<(), CompletionFailure> {
        let mut candidate = request.clone();
        let prepare = async {
            super::attachments::AttachmentBackend {
                endpoint: &self.endpoint,
                model: &self.model,
                client: &self.http_client,
                session: &context.session,
                fingerprint: self.connection_fingerprint(),
                now: self.clock.unix_seconds()?,
                cancellation: context.cancellation.clone().unwrap_or_default(),
            }
            .prepare(&mut candidate)
            .await
        };
        if let Some(token) = &context.cancellation {
            tokio::select! {
                biased;
                _ = token.cancelled() => return Err(CompletionFailure::cancelled(
                    PureError::LlmError("attachment preparation cancelled".into()), Box::default())),
                result = prepare => result?,
            }
        } else {
            prepare.await?;
        }
        *request = candidate;
        Ok(())
    }

    /// 收集一次尝试；本地工具仅在成功返回后执行，托管工具和已报告使用量的响应不能重放。
    async fn run_stream_attempt(
        &self,
        request: CompletionRequest,
        context: &ModelInvocationContext,
        trace: Option<CompletionTraceContext>,
        inference_timer: &InferenceTimer,
    ) -> (
        std::result::Result<CompletionResponse, CompletionFailure>,
        bool,
    ) {
        let has_hosted_tools = request.tools.iter().any(|tool| match tool {
            pl_protocol::ToolSpec::Function { .. } | pl_protocol::ToolSpec::Custom { .. } => false,
            pl_protocol::ToolSpec::ProgrammaticToolCalling
            | pl_protocol::ToolSpec::WebSearch { .. } => true,
        });
        let is_compaction = self.purpose == InvocationPurpose::RemoteCompaction;
        let (body, mut model_observation) =
            match self.prepare_request_body(request, context.prompt_cache_key.as_deref()) {
                Ok(prepared) => prepared,
                Err(error) => return (Err(error.into()), true),
            };
        let opening = self.stream_events(body, context.session.clone(), trace.clone());
        tokio::pin!(opening);
        let opened = match context.cancellation.as_ref() {
            Some(token) => tokio::select! {
                result = &mut opening => result,
                _ = token.cancelled() => return (Err(CompletionFailure::cancelled(
                    PureError::LlmError("model invocation cancelled".into()),
                    Box::default(),
                ).with_model_observation(model_observation.clone())), false),
            },
            None => opening.await,
        };
        match opened {
            Ok(opened) => {
                model_observation.sent_model = opened.sent_model;
                let replay_unsafe = Arc::new(AtomicBool::new(false));
                let tracked_stream: CompletionEventStream = opened
                    .events
                    .map({
                        let replay_unsafe = Arc::clone(&replay_unsafe);
                        let inference_timer = inference_timer.clone();
                        let mut progress = super::thread_model::progress::ProgressProjection::new(context.progress.clone());
                        move |event| {
                            if let Ok(event) = &event
                                && (has_hosted_tools
                                    || (is_compaction && matches!(event,
                                        crate::completion::stream::event::ModelStreamEvent::ResponsesContextItem { .. }))
                                    || matches!(event,
                                    crate::completion::stream::event::ModelStreamEvent::Usage(_)
                                    | crate::completion::stream::event::ModelStreamEvent::Completed { .. }))
                            {
                                replay_unsafe.store(true, Ordering::Release);
                            }
                            if let Ok(event) = &event {
                                inference_timer.observe(event);
                                if let Some(decode_millis) = inference_timer.decode_millis() {
                                    progress.observe_decode_millis(decode_millis);
                            }
                                progress.observe(event)?;
                            }
                            event
                        }
                    })
                    .boxed();
                let result =
                    crate::completion::stream::collect_completion_event_stream_with_idle_timeout(
                        tracked_stream,
                        StreamCollectContext {
                            event_tx: &context.event_tx,
                            trace,
                            trace_sink: context.trace_sink.clone(),
                            cancellation: context.cancellation.clone(),
                            model_observation: Some(model_observation),
                        },
                        opened.idle_timeout,
                    )
                    .await;
                let retry_allowed = !replay_unsafe.load(Ordering::Acquire);
                (result, retry_allowed)
            }
            Err(error) => (
                Err(CompletionFailure::from(error).with_model_observation(model_observation)),
                true,
            ),
        }
    }

    fn prepare_request_body(
        &self,
        request: CompletionRequest,
        prompt_cache_key: Option<&str>,
    ) -> Result<(OpenAiRequestBody, InferenceModelObservation)> {
        let protocol = openai_protocol(self.model.binding.transport.protocol);
        let request = super::context::project_request(
            &self.endpoint,
            &self.model,
            &self.provider_instance_id,
            request,
        )?;
        let mut body = protocol.build_request(&request, &self.model, prompt_cache_key)?;
        body.apply_native_options(&self.native_body);
        if self.purpose == InvocationPurpose::RemoteCompaction {
            body.prepare_compaction();
        }
        let sent_model = body.sent_model()?;
        Ok((
            body,
            InferenceModelObservation {
                configured_model: self.model.slug.clone(),
                sent_model,
                reported_model: None,
            },
        ))
    }

    fn active_transport(&self, session: &ModelSession) -> OpenAiTransport {
        let transport = &self.model.binding.transport;
        let connection_key = self.connection_fingerprint();
        if transport.protocol == ProviderWireProtocol::Responses
            && transport.default_connection_mode == ProviderConnectionMode::WebSocket
            && !session.uses_responses_http_fallback(connection_key)
        {
            return OpenAiTransport::ResponsesWebSocket;
        }
        OpenAiTransport::Http
    }

    fn supports_http_fallback(&self) -> bool {
        self.model
            .binding
            .transport
            .supported_connection_modes
            .contains(&ProviderConnectionMode::Http)
    }

    fn stream_events(
        &self,
        body: OpenAiRequestBody,
        session: ModelSession,
        trace: Option<CompletionTraceContext>,
    ) -> impl std::future::Future<Output = Result<OpenedCompletionStream>> + Send {
        let http_client = self.http_client.clone();
        let api_base = self.resolve_base_url();
        let endpoint = self.endpoint.clone();
        let model_info = self.model.clone();
        let protocol = openai_protocol(model_info.binding.transport.protocol);
        let connection_key = self.connection_fingerprint();
        let transport = self.active_transport(&session);
        async move {
            let expected_sent_model = body.sent_model()?;
            let token = endpoint.bearer_token.clone();
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
                        model_headers: &model_info.binding.request.headers,
                        connection_key,
                        model_session: session,
                        body,
                        expected_sent_model,
                        trace: trace.clone(),
                    },
                )
                .await?;
                return Ok(OpenedCompletionStream {
                    events: decode_raw_event_stream(raw_stream.stream, protocol, trace),
                    sent_model: raw_stream.sent_model,
                    idle_timeout: Some(RESPONSES_WEBSOCKET_IDLE_TIMEOUT),
                });
            }
            let capture = wire_capture::capture_http(&body, trace.as_ref()).await?;
            let headers = super::transport::headers(
                token.as_deref(),
                endpoint.http_headers.as_ref(),
                &model_info.binding.request.headers,
            )?;
            let request = match body {
                OpenAiRequestBody::Responses(body) => http_client
                    .post(format!("{api_base}/responses"))
                    .json(&body),
                OpenAiRequestBody::Chat(body) => http_client
                    .post(format!("{api_base}/chat/completions"))
                    .json(&body),
            };
            let stream = match super::transport::sse(
                request
                    .headers(headers)
                    .header("accept", "text/event-stream"),
            )
            .await
            {
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
                    return Err(error);
                }
            };

            let raw_stream = stream;
            let raw_stream = wire_capture::observe_http_stream(raw_stream, capture);
            let raw_stream =
                if model_info.binding.transport.protocol == ProviderWireProtocol::Responses {
                    // HTTP has no continuation lease; close the body after the terminal response.
                    // WebSocket sources must still be fully drained so their lease can commit.
                    futures::stream::unfold((raw_stream, false), |(mut stream, ended)| async move {
                        if ended {
                            return None;
                        }
                        let event = stream.next().await?;
                        let ended = event.as_ref().is_ok_and(|event| {
                            matches!(
                                event.kind.as_str(),
                                "response.completed" | "response.failed" | "response.incomplete"
                            )
                        });
                        Some((event, (stream, ended)))
                    })
                    .boxed()
                } else {
                    raw_stream
                };
            Ok(OpenedCompletionStream {
                events: decode_raw_event_stream(raw_stream, protocol, trace),
                sent_model: expected_sent_model,
                idle_timeout: None,
            })
        }
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
        model_info.binding.transport.protocol.hash(&mut hasher);
        model_info
            .binding
            .transport
            .default_connection_mode
            .hash(&mut hasher);
        model_info
            .binding
            .transport
            .supported_connection_modes
            .hash(&mut hasher);
        let mut headers = model_info
            .binding
            .request
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
