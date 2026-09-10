use pl_protocol::Result;
use tokio_util::sync::CancellationToken;

use crate::completion::{CompletionRequest, CompletionResponse, ReasoningConfig};
use crate::completion::{
    CompletionResponseSnapshot, completion_response_message_text, completion_response_snapshot,
};
use crate::config::ResolvedModelRoute;
use crate::runtime::{ModelInvocationContext, ModelRuntime};
use pl_protocol::ToolSpec;

/// 不需要完整 turn loop 的单次模型请求。
///
/// 模型由 [`ModelTurnClient`] 绑定，请求只描述本次 invocation 的 canonical 输入。
#[derive(Debug, Clone)]
pub struct ModelTurnRequest {
    instructions: Option<String>,
    tools: Vec<ToolSpec>,
    tool_choice: String,
    parallel_tool_calls: bool,
    max_tokens: Option<u64>,
    reasoning: Option<ReasoningConfig>,
}

impl Default for ModelTurnRequest {
    fn default() -> Self {
        Self {
            instructions: None,
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            max_tokens: None,
            reasoning: None,
        }
    }
}

impl ModelTurnRequest {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从已校验的模型路由继承模型能力、token 限制和 reasoning 配置。
    pub fn from_route(route: &ResolvedModelRoute) -> Self {
        let reasoning = route.reasoning_config();
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

    /// Sets the provider-neutral tool selection mode for this invocation.
    ///
    /// Use `"none"` when the caller must guarantee that no tool can be selected,
    /// even if a provider defaults an empty tool list to automatic selection.
    pub fn with_tool_choice(mut self, tool_choice: impl Into<String>) -> Self {
        self.tool_choice = tool_choice.into();
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
    session: Option<crate::runtime::ModelSession>,
    prompt_cache_key: Option<String>,
}

impl ModelTurnOptions {
    /// Supplies the caller-owned model session; no agent or product state is accessed.
    pub fn with_session(mut self, session: crate::runtime::ModelSession) -> Self {
        self.session = Some(session);
        self
    }

    /// Supplies a provider cache hint for this invocation, without changing its input.
    pub fn with_prompt_cache_key(mut self, key: String) -> Self {
        self.prompt_cache_key = Some(key);
        self
    }

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
            runtime: ModelRuntime::from_route(route)?,
        })
    }

    /// Explicit access to the bound native client; ordinary completion remains provider-neutral.
    pub fn provider(&self) -> &crate::provider::ProviderClient {
        self.runtime.provider()
    }

    /// 执行一次模型调用，并返回不暴露 provider/wire 类型的宿主快照。
    pub async fn complete(
        &self,
        input: &[pl_protocol::ModelContextItem],
        request: ModelTurnRequest,
        options: ModelTurnOptions,
    ) -> std::result::Result<CompletionResponseSnapshot, crate::completion::CompletionFailure> {
        let response = self.complete_raw(input, request, options).await?;
        Ok(completion_response_snapshot(&response))
    }

    /// 执行一次模型调用并只返回 assistant 可见文本。
    pub async fn complete_text(
        &self,
        input: &[pl_protocol::ModelContextItem],
        request: ModelTurnRequest,
        options: ModelTurnOptions,
    ) -> std::result::Result<String, crate::completion::CompletionFailure> {
        let response = self.complete_raw(input, request, options).await?;
        Ok(completion_response_message_text(&response))
    }

    async fn complete_raw(
        &self,
        input: &[pl_protocol::ModelContextItem],
        request: ModelTurnRequest,
        options: ModelTurnOptions,
    ) -> std::result::Result<CompletionResponse, crate::completion::CompletionFailure> {
        let request = CompletionRequest::builder()
            .maybe_instructions(request.instructions)
            .input(input.to_vec())
            .tools(request.tools)
            .tool_choice(request.tool_choice)
            .parallel_tool_calls(request.parallel_tool_calls)
            .maybe_max_tokens(request.max_tokens)
            .reasoning(request.reasoning)
            .build();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let owns_session = options.session.is_none();
        let session = options.session.unwrap_or_default();
        let invocation = ModelInvocationContext::new(session.clone())
            .with_events(event_tx)
            .with_prompt_cache_key(options.prompt_cache_key)
            .with_cancellation(options.cancellation_token);
        let result = self.runtime.complete(request, invocation).await;
        if !owns_session {
            return result;
        }
        match (result, session.close().await) {
            (result, Ok(())) => result,
            (Ok(response), Err(source)) => Err(crate::completion::CompletionFailure {
                source,
                accounting: Box::new(response.accounting),
            }),
            (Err(failure), Err(cleanup)) => Err(crate::completion::CompletionFailure {
                source: pl_protocol::PureError::Io(std::io::Error::other(HostCallCleanupFailure {
                    primary: failure.source,
                    cleanup,
                })),
                accounting: failure.accounting,
            }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("host model call failed ({primary}); owned session cleanup also failed ({cleanup})")]
struct HostCallCleanupFailure {
    #[source]
    primary: pl_protocol::PureError,
    cleanup: pl_protocol::PureError,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn explicitly_supplied_session_remains_available_until_its_owner_closes_it() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                crate::runtime::test_support::capture_http_request(&mut socket).await;
                let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        let client = ModelTurnClient {
            runtime: ModelRuntime::new(
                crate::provider::ProviderEndpoint::deepseek(Some(format!("http://{address}"))),
                crate::model::ModelInfo::compatible("host-test"),
            )
            .unwrap(),
        };
        let session = crate::runtime::ModelSession::default();
        for _ in 0..2 {
            let text = client
                .complete_text(
                    &[],
                    ModelTurnRequest::new(),
                    ModelTurnOptions::default().with_session(session.clone()),
                )
                .await
                .unwrap();
            assert_eq!(text, "ok");
        }
        session.close().await.unwrap();
        assert!(
            client
                .complete(
                    &[],
                    ModelTurnRequest::new(),
                    ModelTurnOptions::default().with_session(session)
                )
                .await
                .is_err()
        );
        server.await.unwrap();
    }
}
