use pl_model::{
    CompletionRequest, ModelProvider, ProviderInfo, ReasoningConfig, ReasoningSummary,
    SharedModelProvider, create_provider, create_provider_with_models,
};
use pl_protocol::{AgentEvent, AgentEventSender, Result};

use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::session::CoreSession;
use crate::tool::{BashTool, SubagentTool, ToolInput, ToolRegistry};
use crate::turn::{
    DEFAULT_MAX_TOOL_ITERATIONS, ToolApprovalDecision, ToolApprovalPolicy, ToolApprovalRequest,
    TurnOptions, TurnRequest, TurnResult,
};

/// 生成唯一的会话 ID（毫秒时间戳 + 序列号）。
fn generate_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{ts:x}-{seq:x}")
}

/// Pure-Lang 核心逻辑层。
///
/// 负责组合会话状态、模型 provider、工具注册表和单轮编译请求。
/// 不执行命令、不写文件，也不依赖独立执行层。
#[derive(Debug)]
pub struct PureCore {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
    tools: ToolRegistry,
}

impl PureCore {
    pub fn new(provider: SharedModelProvider) -> Self {
        Self {
            provider,
            reasoning_effort: None,
            tools: ToolRegistry::new(),
        }
    }

    pub fn with_reasoning_effort(
        provider: SharedModelProvider,
        reasoning_effort: ReasoningEffort,
    ) -> Self {
        Self {
            provider,
            reasoning_effort: Some(reasoning_effort),
            tools: ToolRegistry::new(),
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

    /// 注册一个工具。
    pub fn register_tool(&mut self, tool: impl crate::tool::Tool + 'static) {
        self.tools.register(tool);
    }

    /// 注册默认工具集合。
    ///
    /// 当前包含 `bash` 和 `subagent`。调用方应通过 `TurnOptions` 控制审批策略。
    pub fn register_default_tools(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
    ) {
        self.register_tool(BashTool::new(workspace_root.into()));
        self.register_tool(SubagentTool::new(
            self.provider.clone(),
            self.reasoning_effort.clone(),
            workspace_instructions,
        ));
    }

    pub fn run_turn<'a>(
        &'a self,
        session: &'a mut CoreSession,
        request: TurnRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<TurnResult>> + Send + 'a {
        self.run_turn_with_options(session, request, event_tx, TurnOptions::default())
    }

    pub fn run_turn_with_options<'a>(
        &'a self,
        session: &'a mut CoreSession,
        request: TurnRequest,
        event_tx: AgentEventSender,
        options: TurnOptions,
    ) -> impl std::future::Future<Output = Result<TurnResult>> + Send + 'a {
        let provider = self.provider.clone();
        let reasoning_effort = self.reasoning_effort.clone();
        let tool_schemas = self.tools.schemas();
        let max_iterations = if self.tools.is_empty() {
            1
        } else {
            request
                .max_tool_iterations
                .clamp(1, DEFAULT_MAX_TOOL_ITERATIONS)
        };

        async move {
            let _ = event_tx.send(AgentEvent::TurnStarted);
            let session_id = generate_session_id();
            session.push_user_prompt(request.prompt);
            let model = provider.default_model().to_string();

            let mut last_content = String::new();
            let mut last_reasoning_content = None;
            let mut last_model = model.clone();
            let mut total_usage = pl_model::TokenUsage::default();
            let mut session_message_count = 0;

            let instructions = format_instructions(
                request.mode.instructions(),
                request.workspace_instructions.as_deref(),
            );
            let reasoning = reasoning_effort.as_ref().map(|effort| ReasoningConfig {
                effort: Some(effort.as_str().to_string()),
                summary: Some(if effort.is_none() {
                    ReasoningSummary::Disabled
                } else {
                    ReasoningSummary::Enabled
                }),
            });

            let mut messages = session.messages().to_vec();
            for _ in 0..max_iterations {
                let completion_request = CompletionRequest {
                    model: model.clone(),
                    instructions: Some(instructions.clone()),
                    messages: messages.clone(),
                    tools: tool_schemas.clone(),
                    tool_choice: "auto".to_string(),
                    parallel_tool_calls: false,
                    temperature: None,
                    max_tokens: None,
                    reasoning: reasoning.clone(),
                    stream: true,
                };

                let response = provider
                    .stream_complete(completion_request, event_tx.clone())
                    .await?;

                let content = response.content.unwrap_or_default();
                let reasoning_content = response.reasoning_content.clone();
                let tool_calls = response.tool_calls;

                total_usage.prompt_tokens += response.usage.prompt_tokens;
                total_usage.completion_tokens += response.usage.completion_tokens;

                if !response.model.is_empty() {
                    last_model = response.model;
                }

                if tool_calls.is_empty() {
                    session.push_assistant_response(content.clone(), reasoning_content.clone());
                    last_content = content;
                    last_reasoning_content = reasoning_content;
                    session_message_count = session.messages().len();
                    break;
                }

                session.push_assistant_tool_calls(
                    if content.is_empty() {
                        None
                    } else {
                        Some(content.clone())
                    },
                    tool_calls.clone(),
                    reasoning_content.clone(),
                );
                last_content = content;
                last_reasoning_content = reasoning_content;

                for tool_call in &tool_calls {
                    let _ = event_tx.send(AgentEvent::ToolCallComplete {
                        id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        arguments: serde_json::to_string(&tool_call.arguments).unwrap_or_default(),
                    });

                    let result = match self.tools.get(&tool_call.name) {
                        Some(tool) => {
                            let approval_request = approval_request(tool_call);
                            match approve_tool_call(&options, &approval_request, event_tx.clone())
                                .await
                            {
                                ToolApprovalDecision::Approved => {
                                    let _ = event_tx.send(AgentEvent::ToolApprovalGranted {
                                        id: approval_request.id.clone(),
                                        name: approval_request.name.clone(),
                                    });
                                    let tool_input = ToolInput {
                                        arguments: tool_call.arguments.clone(),
                                        session_id: session_id.clone(),
                                        tool_id: tool_call.id.clone(),
                                    };
                                    match tool.execute(tool_input).await {
                                        Ok(output) => output.description,
                                        Err(e) => format!("Tool execution error: {e}"),
                                    }
                                }
                                ToolApprovalDecision::Denied { reason } => {
                                    let _ = event_tx.send(AgentEvent::ToolApprovalDenied {
                                        id: approval_request.id.clone(),
                                        name: approval_request.name.clone(),
                                        reason: reason.clone(),
                                    });
                                    format!("Tool execution denied: {reason}")
                                }
                            }
                        }
                        None => format!("Unknown tool: {}", tool_call.name),
                    };

                    session.push_tool_result(
                        tool_call
                            .call_id
                            .clone()
                            .unwrap_or_else(|| tool_call.id.clone()),
                        tool_call.name.clone(),
                        result,
                    );
                }

                session_message_count = session.messages().len();
                messages = session.messages().to_vec();
            }

            total_usage.total_tokens = total_usage.prompt_tokens + total_usage.completion_tokens;

            let _ = event_tx.send(AgentEvent::Done);

            Ok(TurnResult {
                content: last_content,
                reasoning_content: last_reasoning_content,
                model: last_model,
                usage: total_usage,
                mode: request.mode,
                session_message_count,
            })
        }
    }
}

fn approval_request(tool_call: &pl_model::ToolCall) -> ToolApprovalRequest {
    let working_directory = tool_call
        .arguments
        .get("workingDirectory")
        .or_else(|| tool_call.arguments.get("working_directory"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    ToolApprovalRequest {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments: tool_call.arguments.clone(),
        working_directory,
    }
}

async fn approve_tool_call(
    options: &TurnOptions,
    request: &ToolApprovalRequest,
    event_tx: AgentEventSender,
) -> ToolApprovalDecision {
    match options.tool_approval_policy {
        ToolApprovalPolicy::AutoAllow => ToolApprovalDecision::Approved,
        ToolApprovalPolicy::DenyAll => ToolApprovalDecision::Denied {
            reason: "tool execution denied by policy".to_string(),
        },
        ToolApprovalPolicy::Manual => {
            let _ = event_tx.send(AgentEvent::ToolApprovalRequested {
                id: request.id.clone(),
                name: request.name.clone(),
                arguments: serde_json::to_string(&request.arguments).unwrap_or_default(),
                working_directory: request.working_directory.clone(),
            });
            match &options.tool_approval_callback {
                Some(callback) => callback(request.clone()).await,
                None => ToolApprovalDecision::Denied {
                    reason: "manual approval required but no approver is configured".to_string(),
                },
            }
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
    use pl_model::ToolCall;
    use pretty_assertions::assert_eq;

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

    #[test]
    fn default_turn_options_auto_allow_tools() {
        let options = TurnOptions::default();

        assert_eq!(options.tool_approval_policy, ToolApprovalPolicy::AutoAllow);
        assert!(options.tool_approval_callback.is_none());
    }

    #[tokio::test]
    async fn manual_tool_approval_can_approve() {
        let options = TurnOptions::manual(std::sync::Arc::new(|request| {
            Box::pin(async move {
                assert_eq!(request.name, "bash");
                ToolApprovalDecision::Approved
            })
        }));
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hi"}),
            working_directory: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);

        let decision = approve_tool_call(&options, &request, event_tx).await;
        let event = event_rx.recv().await.unwrap();

        assert_eq!(decision, ToolApprovalDecision::Approved);
        assert!(matches!(
            event,
            AgentEvent::ToolApprovalRequested {
                id,
                name,
                ..
            } if id == "call-1" && name == "bash"
        ));
    }

    #[tokio::test]
    async fn deny_all_tool_approval_denies_without_request_event() {
        let options = TurnOptions::deny_all();
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hi"}),
            working_directory: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);

        let decision = approve_tool_call(&options, &request, event_tx).await;

        assert_eq!(
            decision,
            ToolApprovalDecision::Denied {
                reason: "tool execution denied by policy".to_string()
            }
        );
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn approval_request_extracts_working_directory() {
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({
                "command": "pwd",
                "workingDirectory": "C:/work"
            }),
            call_id: None,
        };

        let request = approval_request(&call);

        assert_eq!(request.working_directory.as_deref(), Some("C:/work"));
    }

    #[test]
    fn default_tools_register_bash_and_subagent() {
        let mut core = PureCore::default_provider().unwrap();

        core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()));

        assert!(core.tools.get("bash").is_some());
        assert!(core.tools.get("subagent").is_some());
    }
}
