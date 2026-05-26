use std::path::PathBuf;

use pl_model::{
    CompletionRequest, ModelProvider, ProviderInfo, ReasoningConfig, ReasoningSummary,
    SharedModelProvider, create_provider, create_provider_with_models,
};
use pl_protocol::{AgentEvent, AgentEventSender, Result, SubagentStatus};

use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::session::CoreSession;
use crate::tool::{
    ApplyPatchTool, BashTool, CopyPathTool, CreateDirectoryTool, DeletePathTool, ListFilesTool,
    MovePathTool, ReadFileTool, SearchFilesTool, StatPathTool, SubagentContext, SubagentTool,
    ToolContext, ToolInput, ToolRegistry, WriteFileTool,
};
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
/// 工具能力由调用方显式注册，并通过 `TurnOptions` 控制审批策略。
#[derive(Debug)]
pub struct PureCore {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
    config: Option<PureConfig>,
    workspace_root: Option<PathBuf>,
    workspace_instructions: Option<String>,
    active_subagent: Option<SubagentContext>,
    tools: ToolRegistry,
}

impl PureCore {
    pub fn new(provider: SharedModelProvider) -> Self {
        Self {
            provider,
            reasoning_effort: None,
            config: None,
            workspace_root: None,
            workspace_instructions: None,
            active_subagent: None,
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
            config: None,
            workspace_root: None,
            workspace_instructions: None,
            active_subagent: None,
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
        Ok(Self {
            provider,
            reasoning_effort: Some(resolved.role_config.effort),
            config: Some(config.clone()),
            workspace_root: None,
            workspace_instructions: None,
            active_subagent: None,
            tools: ToolRegistry::new(),
        })
    }

    pub(crate) fn with_subagent_context(mut self, context: SubagentContext) -> Self {
        self.active_subagent = Some(context);
        self
    }

    /// 注册一个工具。
    pub fn register_tool(&mut self, tool: impl crate::tool::Tool + 'static) {
        self.tools.register(tool);
    }

    /// 注册默认工具集合。
    ///
    /// 当前包含 shell、subagent 和 workspace 文件工具。调用方应通过 `TurnOptions` 控制审批策略。
    pub fn register_default_tools(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
    ) {
        let workspace_root = workspace_root.into();
        self.workspace_root = Some(workspace_root.clone());
        self.workspace_instructions = workspace_instructions.clone();
        self.register_tool(BashTool::new(workspace_root));
        self.register_tool(ReadFileTool::new());
        self.register_tool(WriteFileTool);
        self.register_tool(ListFilesTool);
        self.register_tool(SearchFilesTool);
        self.register_tool(StatPathTool);
        self.register_tool(CreateDirectoryTool);
        self.register_tool(DeletePathTool);
        self.register_tool(CopyPathTool);
        self.register_tool(MovePathTool);
        self.register_tool(ApplyPatchTool);
        self.register_tool(SubagentTool::new(
            self.provider.clone(),
            self.reasoning_effort.clone(),
            self.config.clone(),
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
        let workspace_root = self
            .workspace_root
            .clone()
            .unwrap_or_else(default_workspace_root);
        let workspace_instructions = self.workspace_instructions.clone();
        let active_subagent = self.active_subagent.clone();
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
                        arguments: tool_call.payload_text(),
                    });

                    let result = match self.tools.get(&tool_call.name) {
                        Some(tool) => {
                            let tool_context = ToolContext {
                                event_tx: event_tx.clone(),
                                options: options.clone(),
                                workspace_root: workspace_root.clone(),
                                workspace_instructions: workspace_instructions.clone(),
                                active_subagent: active_subagent.clone(),
                            };
                            if tool_call.name == "subagent" {
                                emit_subagent_tool_state(
                                    tool_call,
                                    &tool_context,
                                    SubagentStatus::Queued,
                                    None,
                                    None,
                                );
                            }
                            let approval_request = approval_request(tool_call, &tool_context);
                            if tool_call.name == "subagent"
                                && matches!(
                                    options.tool_approval_policy,
                                    ToolApprovalPolicy::Manual
                                )
                            {
                                emit_subagent_tool_state(
                                    tool_call,
                                    &tool_context,
                                    SubagentStatus::AwaitingApproval,
                                    Some("等待工具审批".to_string()),
                                    None,
                                );
                            }
                            match approve_tool_call(
                                &options,
                                &approval_request,
                                event_tx.clone(),
                                &tool_context,
                            )
                            .await
                            {
                                ToolApprovalDecision::Approved => {
                                    let _ = event_tx.send(AgentEvent::ToolApprovalGranted {
                                        id: approval_request.id.clone(),
                                        name: approval_request.name.clone(),
                                    });
                                    let tool_input = ToolInput {
                                        arguments: tool_call.arguments_for_tool(),
                                        session_id: session_id.clone(),
                                        tool_id: tool_call.id.clone(),
                                    };
                                    match tool.execute(tool_input, tool_context).await {
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
                                    if tool_call.name == "subagent" {
                                        emit_subagent_tool_state(
                                            tool_call,
                                            &tool_context,
                                            SubagentStatus::Denied,
                                            None,
                                            Some(reason.clone()),
                                        );
                                    }
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
                        tool_call.kind(),
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

fn approval_request(tool_call: &pl_model::ToolCall, context: &ToolContext) -> ToolApprovalRequest {
    let arguments = tool_call.arguments_for_display();
    let tool_arguments = tool_call.arguments_for_tool();
    let working_directory =
        get_working_directory(&tool_arguments).or_else(|| get_working_directory(&arguments));
    ToolApprovalRequest {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments,
        working_directory,
        parent_subagent_id: context
            .active_subagent
            .as_ref()
            .map(|subagent| subagent.id.clone()),
    }
}

fn get_working_directory(arguments: &serde_json::Value) -> Option<String> {
    arguments
        .get("workingDirectory")
        .or_else(|| arguments.get("working_directory"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

async fn approve_tool_call(
    options: &TurnOptions,
    request: &ToolApprovalRequest,
    event_tx: AgentEventSender,
    context: &ToolContext,
) -> ToolApprovalDecision {
    match options.tool_approval_policy {
        ToolApprovalPolicy::AutoAllow => ToolApprovalDecision::Approved,
        ToolApprovalPolicy::DenyAll => ToolApprovalDecision::Denied {
            reason: "tool execution denied by policy".to_string(),
        },
        ToolApprovalPolicy::Manual => {
            if request.name != "subagent"
                && let Some(subagent) = &context.active_subagent
            {
                emit_subagent_state(
                    &event_tx,
                    subagent,
                    SubagentStatus::AwaitingToolApproval,
                    Some(format!("等待工具审批：{}", request.name)),
                    None,
                );
            }
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

fn emit_subagent_tool_state(
    tool_call: &pl_model::ToolCall,
    context: &ToolContext,
    status: SubagentStatus,
    summary: Option<String>,
    error: Option<String>,
) {
    let (role, task) = subagent_tool_parts(tool_call);
    let depth = context
        .active_subagent
        .as_ref()
        .map(|subagent| subagent.depth + 1)
        .unwrap_or(1);
    let parent_id = context
        .active_subagent
        .as_ref()
        .map(|subagent| subagent.id.clone());
    let _ = context.event_tx.send(AgentEvent::SubagentStateChanged {
        id: tool_call.id.clone(),
        parent_id,
        role,
        task,
        status,
        summary,
        depth,
        error,
        updated_at: unix_seconds(),
    });
}

pub(crate) fn emit_subagent_state(
    event_tx: &AgentEventSender,
    subagent: &SubagentContext,
    status: SubagentStatus,
    summary: Option<String>,
    error: Option<String>,
) {
    let _ = event_tx.send(AgentEvent::SubagentStateChanged {
        id: subagent.id.clone(),
        parent_id: subagent.parent_id.clone(),
        role: subagent.role.clone(),
        task: subagent.task.clone(),
        status,
        summary,
        depth: subagent.depth,
        error,
        updated_at: unix_seconds(),
    });
}

fn subagent_tool_parts(tool_call: &pl_model::ToolCall) -> (String, String) {
    let arguments = tool_call.arguments_for_tool();
    let role = arguments
        .get("role")
        .and_then(serde_json::Value::as_str)
        .filter(|role| !role.trim().is_empty())
        .unwrap_or("executor")
        .to_string();
    let task = arguments
        .get("task")
        .and_then(serde_json::Value::as_str)
        .map(compact_text)
        .unwrap_or_else(|| "(missing task)".to_string());
    (role, task)
}

pub(crate) fn compact_text(text: &str) -> String {
    const MAX_CHARS: usize = 240;
    let trimmed = text.trim();
    let mut result = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= MAX_CHARS {
            result.push_str("...");
            return result;
        }
        result.push(ch);
    }
    result
}

fn default_workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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

    fn test_tool_context(event_tx: AgentEventSender) -> ToolContext {
        ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            active_subagent: None,
        }
    }

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
            parent_subagent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let context = test_tool_context(event_tx.clone());

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;
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
            parent_subagent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let context = test_tool_context(event_tx.clone());

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;

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
        let call = ToolCall::function(
            "call-1",
            "bash",
            serde_json::json!({
                "command": "pwd",
                "workingDirectory": "C:/work"
            }),
            None,
        );

        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let request = approval_request(&call, &test_tool_context(event_tx));

        assert_eq!(request.working_directory.as_deref(), Some("C:/work"));
    }

    #[test]
    fn approval_request_marks_parent_subagent() {
        let call = ToolCall::function(
            "call-1",
            "bash",
            serde_json::json!({"command": "pwd"}),
            None,
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut context = test_tool_context(event_tx);
        context.active_subagent = Some(SubagentContext {
            id: "subagent-1".to_string(),
            parent_id: None,
            role: "executor".to_string(),
            task: "inspect".to_string(),
            depth: 1,
        });

        let request = approval_request(&call, &context);

        assert_eq!(request.parent_subagent_id.as_deref(), Some("subagent-1"));
    }

    #[test]
    fn subagent_state_helpers_emit_default_lifecycle() {
        let call = ToolCall::function(
            "subagent-1",
            "subagent",
            serde_json::json!({"task": "inspect workspace"}),
            None,
        );
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let context = test_tool_context(event_tx.clone());
        let subagent = SubagentContext {
            id: "subagent-1".to_string(),
            parent_id: None,
            role: "executor".to_string(),
            task: "inspect workspace".to_string(),
            depth: 1,
        };

        emit_subagent_tool_state(&call, &context, SubagentStatus::Queued, None, None);
        emit_subagent_state(
            &event_tx,
            &subagent,
            SubagentStatus::Running,
            Some("started".to_string()),
            None,
        );
        emit_subagent_state(
            &event_tx,
            &subagent,
            SubagentStatus::Succeeded,
            Some("done".to_string()),
            None,
        );

        let statuses = [
            event_rx.try_recv().unwrap(),
            event_rx.try_recv().unwrap(),
            event_rx.try_recv().unwrap(),
        ]
        .map(|event| match event {
            AgentEvent::SubagentStateChanged { status, .. } => status,
            other => panic!("unexpected event: {other:?}"),
        });

        assert_eq!(
            statuses,
            [
                SubagentStatus::Queued,
                SubagentStatus::Running,
                SubagentStatus::Succeeded,
            ]
        );
    }

    #[test]
    fn default_tools_register_bash_and_subagent() {
        let mut core = PureCore::default_provider().unwrap();

        core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()));

        assert!(core.tools.get("bash").is_some());
        assert!(core.tools.get("subagent").is_some());
        assert!(core.tools.get("read_file").is_some());
        assert!(core.tools.get("apply_patch").is_some());
    }
}
