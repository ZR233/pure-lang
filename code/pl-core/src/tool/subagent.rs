use std::path::PathBuf;

use pl_model::SharedModelProvider;
use pl_protocol::{AgentEvent, PureError, SubagentStatus};
use serde::{Deserialize, Serialize};

use super::truncation::{OutputTruncation, TruncatedOutput};
use super::{SubagentContext, Tool, ToolContext, ToolInput, ToolOutput};
use crate::PureCore;
use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::core::{compact_text, emit_subagent_state};
use crate::session::CoreSession;
use crate::turn::{CompileMode, TurnResultStatus};

const MAX_SUBAGENT_DEPTH: u32 = 3;

/// 子代理工具。
///
/// 将子任务委托给独立的 LLM 会话执行。
/// 子代理拥有独立的会话历史，并注册完整默认工具能力。
#[derive(Debug, Clone)]
pub struct SubagentTool {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
    config: Option<PureConfig>,
    workspace_instructions: Option<String>,
}

/// SubagentTool 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentInput {
    /// 子任务描述。
    pub task: String,
    /// 子代理使用的模型角色，缺省为执行者。
    #[serde(default)]
    pub role: Option<String>,
    /// 最大迭代次数（当前版本未使用，预留扩展）。
    #[serde(default)]
    pub max_iterations: Option<u32>,
}

impl SubagentTool {
    pub fn new(
        provider: SharedModelProvider,
        reasoning_effort: Option<ReasoningEffort>,
        config: Option<PureConfig>,
        workspace_instructions: Option<String>,
    ) -> Self {
        Self {
            provider,
            reasoning_effort,
            config,
            workspace_instructions,
        }
    }

    fn parse_input(arguments: serde_json::Value) -> Result<SubagentInput, PureError> {
        serde_json::from_value(arguments).map_err(|e| PureError::ToolExecutionFailed {
            tool: "subagent".to_string(),
            error: format!("invalid input: {e}"),
        })
    }

    fn role(input: &SubagentInput) -> Result<ModelRole, PureError> {
        match input
            .role
            .as_deref()
            .map(str::trim)
            .filter(|role| !role.is_empty())
        {
            Some(role) => ModelRole::from_key(role).ok_or_else(|| PureError::ToolExecutionFailed {
                tool: "subagent".to_string(),
                error: format!("unsupported role: {role}"),
            }),
            None => Ok(ModelRole::Executor),
        }
    }
}

impl Tool for SubagentTool {
    fn name(&self) -> &str {
        "subagent"
    }

    fn description(&self) -> &str {
        "Delegate a subtask to an independent LLM session. The subagent \
         receives the task, processes it, and returns the result. Use this \
         to parallelize independent subtasks or isolate context. When the \
         user explicitly asks for subagents or per-crate exploration, call \
         this tool instead of replacing that request with shell/file tools. \
         Optionally choose one of the fixed model roles."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task description for the subagent to execute"
                },
                "role": {
                    "type": "string",
                    "enum": ["explorer", "planner", "executor", "reviewer"],
                    "description": "The model role used by the subagent. Defaults to executor."
                },
                "maxIterations": {
                    "type": "integer",
                    "description": "Maximum iterations for the subagent (reserved for future use)"
                }
            },
            "required": ["task"]
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let depth = context
                .active_subagent
                .as_ref()
                .map(|subagent| subagent.depth + 1)
                .unwrap_or(1);
            let fallback_context = SubagentContext {
                id: input.tool_id.clone(),
                parent_id: context
                    .active_subagent
                    .as_ref()
                    .map(|subagent| subagent.id.clone()),
                agent_path: context
                    .active_subagent
                    .as_ref()
                    .and_then(|subagent| subagent.agent_path.clone()),
                role: input
                    .arguments
                    .get("role")
                    .and_then(serde_json::Value::as_str)
                    .filter(|role| !role.trim().is_empty())
                    .unwrap_or("executor")
                    .to_string(),
                task: input
                    .arguments
                    .get("task")
                    .and_then(serde_json::Value::as_str)
                    .map(compact_text)
                    .unwrap_or_else(|| "(missing task)".to_string()),
                depth,
            };
            let subagent_input = match Self::parse_input(input.arguments) {
                Ok(input) => input,
                Err(error) => {
                    emit_subagent_state(
                        &context.event_tx,
                        &fallback_context,
                        SubagentStatus::Failed,
                        None,
                        Some(error.to_string()),
                    );
                    return Err(error);
                }
            };
            let role = match Self::role(&subagent_input) {
                Ok(role) => role,
                Err(error) => {
                    emit_subagent_state(
                        &context.event_tx,
                        &fallback_context,
                        SubagentStatus::Failed,
                        None,
                        Some(error.to_string()),
                    );
                    return Err(error);
                }
            };
            let subagent_context = SubagentContext {
                id: input.tool_id.clone(),
                parent_id: fallback_context.parent_id.clone(),
                agent_path: fallback_context.agent_path.clone(),
                role: role.key().to_string(),
                task: compact_text(&subagent_input.task),
                depth,
            };

            if depth > MAX_SUBAGENT_DEPTH {
                let error = format!("subagent nesting depth exceeds {MAX_SUBAGENT_DEPTH}");
                emit_subagent_state(
                    &context.event_tx,
                    &subagent_context,
                    SubagentStatus::Failed,
                    None,
                    Some(error.clone()),
                );
                return Err(PureError::ToolExecutionFailed {
                    tool: "subagent".to_string(),
                    error,
                });
            }

            emit_subagent_state(
                &context.event_tx,
                &subagent_context,
                SubagentStatus::Running,
                Some("子代理已启动".to_string()),
                None,
            );

            let core_result = match &self.config {
                Some(config) => PureCore::from_config(config, role),
                None => Ok(match &self.reasoning_effort {
                    Some(effort) => {
                        PureCore::with_reasoning_effort(self.provider.clone(), effort.clone())
                    }
                    None => PureCore::new(self.provider.clone()),
                }),
            };
            let mut core = match core_result {
                Ok(core) => core
                    .with_agent_control(context.agent_control.clone())
                    .with_subagent_context(subagent_context.clone()),
                Err(error) => {
                    emit_subagent_state(
                        &context.event_tx,
                        &subagent_context,
                        SubagentStatus::Failed,
                        None,
                        Some(error.to_string()),
                    );
                    return Err(error);
                }
            };
            core.register_default_tools(
                context.workspace_root.clone(),
                context
                    .workspace_instructions
                    .clone()
                    .or_else(|| self.workspace_instructions.clone()),
            );

            let mut request = crate::turn::TurnRequest::new(subagent_input.task, CompileMode::Auto);
            if let Some(max) = subagent_input.max_iterations {
                request = request.with_max_tool_iterations(max as usize);
            }
            if let Some(instructions) = context
                .workspace_instructions
                .clone()
                .or_else(|| self.workspace_instructions.clone())
            {
                request = request.with_workspace_instructions(instructions.clone());
            }

            let mut session = CoreSession::new();
            let (subagent_event_tx, subagent_event_rx) = tokio::sync::broadcast::channel(256);
            let drain_task = tokio::spawn(forward_subagent_events(
                subagent_event_rx,
                context.event_tx.clone(),
            ));

            let result = core
                .run_turn_with_options(
                    &mut session,
                    request,
                    subagent_event_tx.clone(),
                    context.options.clone(),
                )
                .await;
            drop(subagent_event_tx);
            let _ = drain_task.await;
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    let message = error.to_string();
                    emit_subagent_state(
                        &context.event_tx,
                        &subagent_context,
                        SubagentStatus::Failed,
                        None,
                        Some(message.clone()),
                    );
                    return Err(error);
                }
            };

            let description = if result.content.trim().is_empty() {
                "Subagent completed with no output.".to_string()
            } else {
                result.content.trim().to_string()
            };
            if result.status == TurnResultStatus::Interrupted {
                emit_subagent_state(
                    &context.event_tx,
                    &subagent_context,
                    SubagentStatus::Failed,
                    Some(compact_text(&description)),
                    Some("interrupted by user".to_string()),
                );
                return Err(PureError::ToolExecutionFailed {
                    tool: "subagent".to_string(),
                    error: "subagent interrupted by user".to_string(),
                });
            }
            emit_subagent_state(
                &context.event_tx,
                &subagent_context,
                SubagentStatus::Succeeded,
                Some(compact_text(&description)),
                None,
            );

            Ok(ToolOutput {
                description,
                truncated: OutputTruncation {
                    stdout: TruncatedOutput {
                        content: String::new(),
                        was_truncated: false,
                        original_length: 0,
                    },
                    stderr: TruncatedOutput {
                        content: String::new(),
                        was_truncated: false,
                        original_length: 0,
                    },
                },
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
            })
        })
    }
}

async fn forward_subagent_events(
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    parent_event_tx: pl_protocol::AgentEventSender,
) {
    loop {
        match event_rx.recv().await {
            Ok(event @ AgentEvent::SubagentStateChanged { .. }) => {
                let _ = parent_event_tx.send(event);
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_model::{ProviderInfo, create_provider};

    fn test_tool() -> SubagentTool {
        SubagentTool::new(
            create_provider(ProviderInfo::default_provider()).unwrap(),
            None,
            None,
            None,
        )
    }

    fn test_context(
        active_subagent: Option<SubagentContext>,
    ) -> (ToolContext, tokio::sync::broadcast::Receiver<AgentEvent>) {
        let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
        (
            ToolContext {
                event_tx,
                options: crate::turn::TurnOptions::default(),
                workspace_root: std::env::temp_dir(),
                workspace_instructions: None,
                active_subagent,
                agent_control: crate::AgentControl::default(),
            },
            event_rx,
        )
    }

    #[test]
    fn subagent_input_defaults_to_executor_role() {
        let input = SubagentTool::parse_input(serde_json::json!({
            "task": "inspect files"
        }))
        .unwrap();

        assert_eq!(SubagentTool::role(&input).unwrap(), ModelRole::Executor);
    }

    #[test]
    fn subagent_input_accepts_explicit_role() {
        let input = SubagentTool::parse_input(serde_json::json!({
            "task": "review changes",
            "role": "reviewer"
        }))
        .unwrap();

        assert_eq!(SubagentTool::role(&input).unwrap(), ModelRole::Reviewer);
    }

    #[test]
    fn subagent_input_rejects_unknown_role() {
        let input = SubagentTool::parse_input(serde_json::json!({
            "task": "review changes",
            "role": "critic"
        }))
        .unwrap();

        let error = SubagentTool::role(&input).unwrap_err().to_string();

        assert!(error.contains("unsupported role"));
    }

    #[tokio::test]
    async fn subagent_depth_limit_emits_failed_state() {
        let tool = test_tool();
        let (context, mut event_rx) = test_context(Some(SubagentContext {
            id: "parent".to_string(),
            parent_id: None,
            agent_path: None,
            role: "executor".to_string(),
            task: "parent task".to_string(),
            depth: MAX_SUBAGENT_DEPTH,
        }));

        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({"task": "too deep"}),
                    session_id: "session".to_string(),
                    tool_id: "child".to_string(),
                },
                context,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(
            event_rx.recv().await.unwrap(),
            AgentEvent::SubagentStateChanged {
                id,
                parent_id,
                status: SubagentStatus::Failed,
                depth,
                ..
            } if id == "child" && parent_id.as_deref() == Some("parent") && depth == 4
        ));
    }

    #[tokio::test]
    async fn subagent_invalid_role_emits_failed_state() {
        let tool = test_tool();
        let (context, mut event_rx) = test_context(None);

        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "task": "review this",
                        "role": "critic"
                    }),
                    session_id: "session".to_string(),
                    tool_id: "child".to_string(),
                },
                context,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(
            event_rx.recv().await.unwrap(),
            AgentEvent::SubagentStateChanged {
                id,
                role,
                status: SubagentStatus::Failed,
                task,
                ..
            } if id == "child" && role == "critic" && task == "review this"
        ));
    }
}
