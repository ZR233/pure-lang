use pl_model::{
    CompletionRequest, CompletionResponse, ModelProvider, ReasoningConfig, SharedModelProvider,
    ToolSchema,
};
use pl_protocol::{Message, PureError, Result};
use pl_trace::AgentEventSender;
use tokio_util::sync::CancellationToken;

use crate::AgentSession;
use crate::message::completion_response_message_text;

/// 单次模型 completion 请求配置。
///
/// 该结构覆盖不需要完整 `TurnEngine` turn loop 的宿主场景，例如只调用模型做
/// context compaction。它始终提交 canonical history；协议级 continuation 由
/// `pl-model` 的 session transport 负责。
#[derive(Debug, Clone)]
pub struct CoreModelTurnRequest {
    model: String,
    instructions: Option<String>,
    tools: Vec<ToolSchema>,
    parallel_tool_calls: bool,
    max_tokens: Option<u64>,
    reasoning: Option<ReasoningConfig>,
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

/// 复用 `AgentSession` transport 状态的轻量模型回合客户端。
#[derive(Debug, Clone, Default)]
pub struct CoreModelTurnClient;

impl CoreModelTurnClient {
    pub fn new() -> Self {
        Self
    }

    pub async fn stream_session_completion_response(
        &self,
        provider: SharedModelProvider,
        session: &mut AgentSession,
        request: CoreModelTurnRequest,
        options: CoreModelTurnOptions,
    ) -> Result<CompletionResponse> {
        stream_session_completion_response(provider, session, request, options).await
    }

    pub async fn stream_session_completion_message_text(
        &self,
        provider: SharedModelProvider,
        session: &mut AgentSession,
        request: CoreModelTurnRequest,
        options: CoreModelTurnOptions,
    ) -> Result<String> {
        let response = self
            .stream_session_completion_response(provider, session, request, options)
            .await?;
        Ok(completion_response_message_text(&response))
    }
}

pub async fn stream_session_completion_response(
    provider: SharedModelProvider,
    session: &mut AgentSession,
    request: CoreModelTurnRequest,
    options: CoreModelTurnOptions,
) -> Result<CompletionResponse> {
    let request_body = completion_request(session, &request);
    stream_completion(&provider, request_body, &options).await
}

pub async fn stream_session_completion_message_text(
    provider: SharedModelProvider,
    session: &mut AgentSession,
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
    let mut session = AgentSession::from_messages(history);
    stream_session_completion_message_text(provider, &mut session, request, options).await
}

fn completion_request(session: &AgentSession, request: &CoreModelTurnRequest) -> CompletionRequest {
    CompletionRequest::builder(request.model.clone())
        .maybe_instructions(request.instructions.clone())
        .input(session.items().to_vec())
        .tools(request.tools.clone())
        .parallel_tool_calls(request.parallel_tool_calls)
        .maybe_max_tokens(request.max_tokens)
        .store(Some(false))
        .prompt_cache_key(session.prompt_cache_key().map(ToString::to_string))
        .reasoning(request.reasoning.clone())
        .transport_session(session.transport_session())
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
