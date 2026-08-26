use pl_model::{
    CompletionRequest, CompletionResponse, ModelInvocationContext, ModelRuntime, ReasoningConfig,
    ReasoningSummary, ToolSpec,
};
use pl_protocol::Result;
use tokio_util::sync::CancellationToken;

use crate::message::{
    CompletionResponseSnapshot, completion_response_message_text, completion_response_snapshot,
};
use crate::{AgentSession, ResolvedModelRoute};

/// 不需要完整 turn loop 的单次模型请求。
///
/// 模型由 [`ModelTurnClient`] 绑定，请求只描述本次 invocation 的 canonical 输入。
#[derive(Debug, Clone, Default)]
pub struct ModelTurnRequest {
    instructions: Option<String>,
    tools: Vec<ToolSpec>,
    parallel_tool_calls: bool,
    max_tokens: Option<u64>,
    reasoning: Option<ReasoningConfig>,
}

impl ModelTurnRequest {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从已校验的模型路由继承模型能力、token 限制和 reasoning 配置。
    pub fn from_route(route: &ResolvedModelRoute) -> Self {
        let reasoning = route.effort.as_ref().map(|effort| ReasoningConfig {
            effort: Some(effort.as_str().to_string()),
            summary: Some(if effort.is_none() {
                ReasoningSummary::Disabled
            } else {
                ReasoningSummary::Enabled
            }),
        });
        Self::new()
            .with_parallel_tool_calls(route.model.capabilities.tools.parallel_tool_calls)
            .with_max_tokens(route.model.max_output_tokens)
            .with_reasoning(reasoning)
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
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

/// 单次模型调用的宿主执行选项。
#[derive(Debug, Clone, Default)]
pub struct ModelTurnOptions {
    cancellation_token: Option<CancellationToken>,
}

impl ModelTurnOptions {
    pub fn with_cancellation(mut self, cancellation_token: CancellationToken) -> Self {
        self.cancellation_token = Some(cancellation_token);
        self
    }
}

/// 绑定一个已解析模型路由的轻量宿主客户端。
#[derive(Debug, Clone)]
pub struct ModelTurnClient {
    runtime: ModelRuntime,
}

impl ModelTurnClient {
    /// 从 canonical 路由构造客户端。
    pub fn from_route(route: &ResolvedModelRoute) -> Result<Self> {
        Ok(Self {
            runtime: ModelRuntime::new(route.endpoint.clone(), route.model.clone())?,
        })
    }

    /// 执行一次模型调用，并返回不暴露 provider/wire 类型的宿主快照。
    pub async fn complete(
        &self,
        session: &AgentSession,
        request: ModelTurnRequest,
        options: ModelTurnOptions,
    ) -> Result<CompletionResponseSnapshot> {
        let response = self.complete_raw(session, request, options).await?;
        Ok(completion_response_snapshot(&response))
    }

    /// 执行一次模型调用并只返回 assistant 可见文本。
    pub async fn complete_text(
        &self,
        session: &AgentSession,
        request: ModelTurnRequest,
        options: ModelTurnOptions,
    ) -> Result<String> {
        let response = self.complete_raw(session, request, options).await?;
        Ok(completion_response_message_text(&response))
    }

    async fn complete_raw(
        &self,
        session: &AgentSession,
        request: ModelTurnRequest,
        options: ModelTurnOptions,
    ) -> Result<CompletionResponse> {
        let request = CompletionRequest::builder()
            .maybe_instructions(request.instructions)
            .input(session.items().to_vec())
            .tools(request.tools)
            .parallel_tool_calls(request.parallel_tool_calls)
            .maybe_max_tokens(request.max_tokens)
            .reasoning(request.reasoning)
            .build();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let invocation = ModelInvocationContext::new(session.model_session(), event_tx)
            .with_prompt_cache_key(session.prompt_cache_key().map(ToString::to_string))
            .with_cancellation(options.cancellation_token);
        self.runtime.complete(request, invocation).await
    }
}
