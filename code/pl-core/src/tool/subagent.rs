use std::path::PathBuf;

use pl_model::SharedModelProvider;
use pl_protocol::{AgentStatus, PureError};
use serde::{Deserialize, Serialize};

use super::multi_agent::{AgentRunConfig, current_agent_path, emit_agent_record, run_agent_turn};
use super::truncation::{OutputTruncation, TruncatedOutput};
use super::{Tool, ToolContext, ToolInput, ToolOutput};
use crate::agent::AgentSpawnInput;
use crate::config::{ModelRole, PureConfig, ReasoningEffort};

/// Subagent 便捷工具。
///
/// 这是完整 agent 协作工具集的同步包装：创建一个 managed agent，
/// 等待它执行完成，并返回该 agent 的最终摘要。
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
    /// 最大工具迭代次数。
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
        "Synchronous convenience tool for spawn_agent + wait_agent. It creates a \
         managed sub-agent, waits for its turn to finish, and returns the final summary."
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
                    "description": "Maximum tool iterations for the subagent turn"
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
            let subagent_input = Self::parse_input(input.arguments)?;
            let role = Self::role(&subagent_input)?;
            let task_name = task_name_from_tool_id(&input.tool_id);
            let handle = context
                .agent_control
                .spawn_agent(AgentSpawnInput {
                    task_name,
                    message: subagent_input.task.clone(),
                    role: role.key().to_string(),
                    parent_path: Some(current_agent_path(&context)),
                })
                .await?;
            let record = context
                .agent_control
                .record(&handle.id)
                .await
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "subagent".to_string(),
                    error: "spawned agent disappeared".to_string(),
                })?;
            emit_agent_record(&context.event_tx, &record);

            run_agent_turn(AgentRunConfig {
                provider: self.provider.clone(),
                reasoning_effort: self.reasoning_effort.clone(),
                config: self.config.clone(),
                workspace_instructions: context
                    .workspace_instructions
                    .clone()
                    .or_else(|| self.workspace_instructions.clone()),
                workspace_root: context.workspace_root.clone(),
                options: context.options.clone(),
                agent_control: context.agent_control.clone(),
                event_tx: context.event_tx.clone(),
                agent_id: handle.id.clone(),
                agent_path: handle.path.clone(),
                role: role.key().to_string(),
                message: subagent_input.task,
                max_tool_iterations: subagent_input.max_iterations.map(|value| value as usize),
            })
            .await;

            let record = context
                .agent_control
                .record(&handle.id)
                .await
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "subagent".to_string(),
                    error: "completed agent disappeared".to_string(),
                })?;
            let description = record
                .summary
                .clone()
                .filter(|summary| !summary.trim().is_empty())
                .unwrap_or_else(|| "Subagent completed with no output.".to_string());
            match record.status {
                AgentStatus::Completed => Ok(ToolOutput {
                    description,
                    truncated: empty_truncation(),
                    output_file: PathBuf::new(),
                    exit_code: None,
                    timed_out: false,
                }),
                AgentStatus::Interrupted => Err(PureError::ToolExecutionFailed {
                    tool: "subagent".to_string(),
                    error: "subagent interrupted by user".to_string(),
                }),
                AgentStatus::Failed => Err(PureError::ToolExecutionFailed {
                    tool: "subagent".to_string(),
                    error: record
                        .error
                        .unwrap_or_else(|| "subagent failed".to_string()),
                }),
                AgentStatus::Queued
                | AgentStatus::Running
                | AgentStatus::Waiting
                | AgentStatus::Closed => Err(PureError::ToolExecutionFailed {
                    tool: "subagent".to_string(),
                    error: format!("subagent ended in invalid status: {:?}", record.status),
                }),
            }
        })
    }
}

fn task_name_from_tool_id(tool_id: &str) -> String {
    let mut result = String::from("subagent");
    for ch in tool_id.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push('_');
            result.push(ch.to_ascii_lowercase());
        }
    }
    result
}

fn empty_truncation() -> OutputTruncation {
    OutputTruncation {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_model::{ProviderInfo, create_provider};
    use pretty_assertions::assert_eq;

    fn test_tool() -> SubagentTool {
        SubagentTool::new(
            create_provider(ProviderInfo::default_provider()).unwrap(),
            None,
            None,
            None,
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

    #[test]
    fn task_name_uses_agent_path_charset() {
        assert_eq!(task_name_from_tool_id("Call-42"), "subagent_c_a_l_l_4_2");
        assert_eq!(task_name_from_tool_id(""), "subagent");
    }

    #[test]
    fn tool_is_constructible() {
        assert_eq!(test_tool().name(), "subagent");
    }
}
