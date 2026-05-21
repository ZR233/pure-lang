use pl_model::{
    CompletionRequest, ModelProvider, ProviderInfo, ReasoningConfig, ReasoningSummary,
    SharedModelProvider, create_provider, create_provider_with_models,
};
use pl_protocol::{AgentEvent, AgentEventSender, Result};

use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::session::CoreSession;
use crate::turn::{TurnRequest, TurnResult};

/// Pure-Lang 核心逻辑层。
///
/// 负责组合会话状态、模型 provider 和单轮编译请求。它不执行命令、不写文件，
/// 也不依赖独立执行层。
#[derive(Debug, Clone)]
pub struct PureCore {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
}

impl PureCore {
    pub fn new(provider: SharedModelProvider) -> Self {
        Self {
            provider,
            reasoning_effort: None,
        }
    }

    pub fn with_reasoning_effort(
        provider: SharedModelProvider,
        reasoning_effort: ReasoningEffort,
    ) -> Self {
        Self {
            provider,
            reasoning_effort: Some(reasoning_effort),
        }
    }

    pub fn from_provider_info(info: ProviderInfo) -> Result<Self> {
        Ok(Self::new(create_provider(info)?))
    }

    pub fn default_provider() -> Result<Self> {
        Self::from_provider_info(ProviderInfo::default_provider())
    }

    pub fn from_config(config: &PureConfig, role: ModelRole) -> Result<Self> {
        let resolved = config.resolve_role(role)?;
        let provider = create_provider_with_models(resolved.provider_info, resolved.models)?;
        Ok(Self::with_reasoning_effort(
            provider,
            resolved.role_config.effort,
        ))
    }

    pub fn run_turn<'a>(
        &'a self,
        session: &'a mut CoreSession,
        request: TurnRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<TurnResult>> + Send + 'a {
        let provider = self.provider.clone();
        let reasoning_effort = self.reasoning_effort.clone();

        async move {
            let _ = event_tx.send(AgentEvent::TurnStarted);
            session.push_user_prompt(request.prompt);
            let model = provider.default_model().to_string();

            let completion_request = CompletionRequest {
                model: model.clone(),
                instructions: Some(format_instructions(
                    request.mode.instructions(),
                    request.workspace_instructions.as_deref(),
                )),
                messages: session.messages().to_vec(),
                tools: Vec::new(),
                tool_choice: "auto".to_string(),
                parallel_tool_calls: false,
                temperature: None,
                max_tokens: None,
                reasoning: reasoning_effort.map(|effort| ReasoningConfig {
                    effort: Some(effort.as_str().to_string()),
                    summary: Some(if effort.is_none() {
                        ReasoningSummary::Disabled
                    } else {
                        ReasoningSummary::Enabled
                    }),
                }),
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

fn format_instructions(base: &str, workspace: Option<&str>) -> String {
    match workspace {
        Some(content) if !content.trim().is_empty() => {
            format!("{base}\n\n# 项目记忆\n{content}")
        }
        _ => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConfigStore, ModelRole};

    #[test]
    fn config_core_uses_planner_role_model_and_effort() {
        let config = ConfigStore::new(crate::ConfigPaths::from_home("unused"))
            .load_or_default()
            .unwrap();
        let core = PureCore::from_config(&config, ModelRole::Planner).unwrap();

        assert_eq!(core.provider.default_model(), "deepseek-v4-flash");
        assert_eq!(core.reasoning_effort.unwrap().as_str(), "high");
    }

    #[test]
    fn format_instructions_without_workspace() {
        assert_eq!(format_instructions("base", None), "base");
    }

    #[test]
    fn format_instructions_with_workspace() {
        assert_eq!(
            format_instructions("base", Some("project rules")),
            "base\n\n# 项目记忆\nproject rules"
        );
    }

    #[test]
    fn format_instructions_ignores_empty_workspace() {
        assert_eq!(format_instructions("base", Some("")), "base");
        assert_eq!(format_instructions("base", Some("   ")), "base");
    }
}
