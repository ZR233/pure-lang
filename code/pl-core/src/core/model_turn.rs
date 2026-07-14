use std::collections::HashSet;
use std::sync::Arc;

use pl_model::{
    CompletionRequest, CompletionResponse, ModelProvider, ReasoningConfig, SharedModelProvider,
    ToolSchema, is_continuation_unsupported_error,
};
use pl_protocol::{Message, PureError, Result};
use pl_trace::AgentEventSender;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::CoreSession;
use crate::message::completion_response_message_text;

/// 单次模型 completion 请求配置。
///
/// 该结构覆盖不需要完整 `PureCore` turn loop 的宿主场景，例如只调用模型做
/// context compaction。它复用 `CoreSession` 的 continuation 状态，避免宿主重复
/// 实现 previous_response_id、prompt cache 和全量回退逻辑。
#[derive(Debug, Clone)]
pub struct CoreModelTurnRequest {
    model: String,
    instructions: Option<String>,
    tools: Vec<ToolSchema>,
    parallel_tool_calls: bool,
    max_tokens: Option<u64>,
    reasoning: Option<ReasoningConfig>,
    use_continuation: bool,
    continuation_cache_key: Option<String>,
}

/// 模型 provider 的 continuation 策略族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreModelProviderFamily {
    OpenAi,
    Other,
}

/// 模型请求使用的 wire API 族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreModelWireApi {
    Responses,
    Chat,
    Other,
}

/// 用于推导模型 continuation 策略的宿主输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreModelContinuationProfile {
    pub provider_family: CoreModelProviderFamily,
    pub wire_api: CoreModelWireApi,
    pub model_supports_continuation: bool,
    pub base_url: String,
    pub model: String,
}

/// pl-core 模型 continuation 执行配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreModelContinuationConfig {
    enabled: bool,
    cache_key: Option<String>,
}

impl CoreModelContinuationConfig {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            cache_key: None,
        }
    }

    pub fn from_profile(profile: CoreModelContinuationProfile) -> Self {
        if profile.provider_family != CoreModelProviderFamily::OpenAi
            || profile.wire_api != CoreModelWireApi::Responses
            || !profile.model_supports_continuation
        {
            return Self::disabled();
        }
        Self {
            enabled: true,
            cache_key: Some(format!(
                "openai|{}|{}",
                profile.base_url.trim_end_matches('/'),
                profile.model
            )),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn cache_key(&self) -> Option<&str> {
        self.cache_key.as_deref()
    }
}

impl CoreModelTurnRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            instructions: None,
            tools: Vec::new(),
            parallel_tool_calls: false,
            max_tokens: None,
            reasoning: None,
            use_continuation: false,
            continuation_cache_key: None,
        }
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolSchema>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_parallel_tool_calls(mut self, parallel_tool_calls: bool) -> Self {
        self.parallel_tool_calls = parallel_tool_calls;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: Option<u64>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_reasoning(mut self, reasoning: Option<ReasoningConfig>) -> Self {
        self.reasoning = reasoning;
        self
    }

    pub fn with_continuation(mut self, use_continuation: bool) -> Self {
        self.use_continuation = use_continuation;
        self
    }

    pub fn with_continuation_cache_key(mut self, key: impl Into<String>) -> Self {
        self.continuation_cache_key = Some(key.into());
        self
    }

    pub fn with_continuation_config(mut self, config: CoreModelContinuationConfig) -> Self {
        self.use_continuation = config.enabled;
        self.continuation_cache_key = config.cache_key;
        self
    }
}

/// 单次模型 completion 执行选项。
#[derive(Debug, Clone, Default)]
pub struct CoreModelTurnOptions {
    cancellation_token: Option<CancellationToken>,
    event_tx: Option<AgentEventSender>,
}

impl CoreModelTurnOptions {
    pub fn with_cancellation(mut self, cancellation_token: CancellationToken) -> Self {
        self.cancellation_token = Some(cancellation_token);
        self
    }

    pub fn with_event_sender(mut self, event_tx: AgentEventSender) -> Self {
        self.event_tx = Some(event_tx);
        self
    }
}

/// 带跨会话 continuation fallback 缓存的模型回合客户端。
///
/// `stream_session_completion_response` 负责单次请求内的 fallback；
/// 该客户端额外记住已知不支持 `previous_response_id` 的 provider/model key，
/// 让后续会话直接走完整历史，避免宿主重复实现 provider 级缓存。
#[derive(Debug, Clone, Default)]
pub struct CoreModelTurnClient {
    unsupported_continuations: Arc<Mutex<HashSet<String>>>,
}

impl CoreModelTurnClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn stream_session_completion_response(
        &self,
        provider: SharedModelProvider,
        session: &mut CoreSession,
        request: CoreModelTurnRequest,
        options: CoreModelTurnOptions,
    ) -> Result<CompletionResponse> {
        if let Some(key) = request.continuation_cache_key.as_deref()
            && self.continuation_is_unsupported(key).await
        {
            session.mark_continuation_unsupported();
        }
        let cache_key = request.continuation_cache_key.clone();
        let used_continuation = request.use_continuation && !session.continuation_disabled();
        let response =
            stream_session_completion_response(provider, session, request, options).await?;
        if used_continuation
            && session.continuation_disabled()
            && let Some(key) = cache_key
        {
            self.unsupported_continuations.lock().await.insert(key);
        }
        Ok(response)
    }

    pub async fn stream_session_completion_message_text(
        &self,
        provider: SharedModelProvider,
        session: &mut CoreSession,
        request: CoreModelTurnRequest,
        options: CoreModelTurnOptions,
    ) -> Result<String> {
        let response = self
            .stream_session_completion_response(provider, session, request, options)
            .await?;
        Ok(completion_response_message_text(&response))
    }

    async fn continuation_is_unsupported(&self, key: &str) -> bool {
        self.unsupported_continuations.lock().await.contains(key)
    }
}

pub async fn stream_session_completion_response(
    provider: SharedModelProvider,
    session: &mut CoreSession,
    request: CoreModelTurnRequest,
    options: CoreModelTurnOptions,
) -> Result<CompletionResponse> {
    let use_continuation = request.use_continuation && !session.continuation_disabled();
    let request_body = completion_request(session, &request, use_continuation);
    match stream_completion(&provider, request_body, &options).await {
        Ok(response) => {
            session.acknowledge_model_response(session.len(), response.response_id.clone());
            Ok(response)
        }
        Err(error) if use_continuation && is_continuation_unsupported_error(&error) => {
            session.mark_continuation_unsupported();
            let fallback = completion_request(session, &request, false);
            let response = stream_completion(&provider, fallback, &options).await?;
            session.mark_continuation_unsupported();
            Ok(response)
        }
        Err(error) => Err(error),
    }
}

pub async fn stream_session_completion_message_text(
    provider: SharedModelProvider,
    session: &mut CoreSession,
    request: CoreModelTurnRequest,
    options: CoreModelTurnOptions,
) -> Result<String> {
    let response = stream_session_completion_response(provider, session, request, options).await?;
    Ok(completion_response_message_text(&response))
}

pub async fn stream_history_completion_message_text(
    provider: SharedModelProvider,
    history: Vec<Message>,
    request: CoreModelTurnRequest,
    options: CoreModelTurnOptions,
) -> Result<String> {
    let mut session = CoreSession::from_messages(history);
    stream_session_completion_message_text(provider, &mut session, request, options).await
}

fn completion_request(
    session: &CoreSession,
    request: &CoreModelTurnRequest,
    use_continuation: bool,
) -> CompletionRequest {
    let history_source = if use_continuation {
        session
            .continuation_start_index()
            .and_then(|start| session.items().get(start..))
            .unwrap_or_else(|| session.items())
    } else {
        session.items()
    };
    CompletionRequest::builder(request.model.clone())
        .maybe_instructions(request.instructions.clone())
        .input(history_source.to_vec())
        .tools(request.tools.clone())
        .parallel_tool_calls(request.parallel_tool_calls)
        .maybe_max_tokens(request.max_tokens)
        .store(Some(use_continuation))
        .previous_response_id(
            use_continuation
                .then(|| session.previous_response_id().map(ToString::to_string))
                .flatten(),
        )
        .prompt_cache_key(session.prompt_cache_key().map(ToString::to_string))
        .reasoning(request.reasoning.clone())
        .build()
}

async fn stream_completion(
    provider: &SharedModelProvider,
    request: CompletionRequest,
    options: &CoreModelTurnOptions,
) -> Result<CompletionResponse> {
    let event_tx = match &options.event_tx {
        Some(event_tx) => event_tx.clone(),
        None => {
            let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
            event_tx
        }
    };
    match &options.cancellation_token {
        Some(token) => {
            tokio::select! {
                response = provider.stream_complete(request, event_tx) => response,
                _ = token.cancelled() => Err(PureError::LlmError("model request cancelled".to_string())),
            }
        }
        None => provider.stream_complete(request, event_tx).await,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CoreModelContinuationConfig, CoreModelContinuationProfile, CoreModelProviderFamily,
        CoreModelTurnRequest, CoreModelWireApi,
    };

    #[test]
    fn continuation_profile_enables_openai_responses_with_stable_cache_key() {
        let config = CoreModelContinuationConfig::from_profile(CoreModelContinuationProfile {
            provider_family: CoreModelProviderFamily::OpenAi,
            wire_api: CoreModelWireApi::Responses,
            model_supports_continuation: true,
            base_url: "https://api.openai.com/v1/".to_string(),
            model: "gpt-5.5".to_string(),
        });

        assert!(config.enabled());
        assert_eq!(
            config.cache_key(),
            Some("openai|https://api.openai.com/v1|gpt-5.5")
        );

        let request = CoreModelTurnRequest::new("gpt-5.5").with_continuation_config(config);

        assert!(request.use_continuation);
        assert_eq!(
            request.continuation_cache_key.as_deref(),
            Some("openai|https://api.openai.com/v1|gpt-5.5")
        );
    }

    #[test]
    fn continuation_profile_disables_non_openai_responses_capability() {
        for profile in [
            CoreModelContinuationProfile {
                provider_family: CoreModelProviderFamily::Other,
                wire_api: CoreModelWireApi::Responses,
                model_supports_continuation: true,
                base_url: "https://example.com/v1".to_string(),
                model: "gpt-5.5".to_string(),
            },
            CoreModelContinuationProfile {
                provider_family: CoreModelProviderFamily::OpenAi,
                wire_api: CoreModelWireApi::Chat,
                model_supports_continuation: true,
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-5.5".to_string(),
            },
            CoreModelContinuationProfile {
                provider_family: CoreModelProviderFamily::OpenAi,
                wire_api: CoreModelWireApi::Responses,
                model_supports_continuation: false,
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-5.5".to_string(),
            },
        ] {
            let config = CoreModelContinuationConfig::from_profile(profile);
            assert!(!config.enabled());
            assert_eq!(config.cache_key(), None);
        }
    }
}
