use pl_model::{
    CompletionRequest, ModelProvider, ProviderInfo, SharedModelProvider, create_provider,
};
use pl_protocol::{AgentEvent, AgentEventSender, Result};

use crate::session::CoreSession;
use crate::turn::{TurnRequest, TurnResult};

/// Pure-Lang 核心逻辑层。
///
/// 负责组合会话状态、模型 provider 和单轮编译请求。它不执行命令、不写文件，
/// 也不依赖独立执行层。
#[derive(Debug, Clone)]
pub struct PureCore {
    provider: SharedModelProvider,
}

impl PureCore {
    pub fn new(provider: SharedModelProvider) -> Self {
        Self { provider }
    }

    pub fn from_provider_info(info: ProviderInfo) -> Result<Self> {
        Ok(Self::new(create_provider(info)?))
    }

    pub fn default_provider() -> Result<Self> {
        Self::from_provider_info(ProviderInfo::default_provider())
    }

    pub fn run_turn<'a>(
        &'a self,
        session: &'a mut CoreSession,
        request: TurnRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<TurnResult>> + Send + 'a {
        let provider = self.provider.clone();

        async move {
            let _ = event_tx.send(AgentEvent::TurnStarted);
            session.push_user_prompt(request.prompt);
            let model = provider.default_model().to_string();

            let completion_request = CompletionRequest {
                model: model.clone(),
                instructions: Some(request.mode.instructions().to_string()),
                messages: session.messages().to_vec(),
                tools: Vec::new(),
                tool_choice: "auto".to_string(),
                parallel_tool_calls: false,
                temperature: None,
                max_tokens: None,
                reasoning: None,
                stream: true,
            };

            let response = provider
                .stream_complete(completion_request, event_tx.clone())
                .await?;
            let content = response.content.unwrap_or_default();
            let reasoning_content = response.reasoning_content;
            session.push_assistant_response(content.clone(), reasoning_content.clone());
            let _ = event_tx.send(AgentEvent::Done);

            Ok(TurnResult {
                content,
                reasoning_content,
                model: if response.model.is_empty() {
                    model
                } else {
                    response.model
                },
                usage: response.usage,
                mode: request.mode,
                session_message_count: session.messages().len(),
            })
        }
    }
}
