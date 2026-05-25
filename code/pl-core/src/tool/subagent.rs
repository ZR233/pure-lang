use std::path::PathBuf;

use pl_model::SharedModelProvider;
use pl_protocol::PureError;
use serde::{Deserialize, Serialize};

use super::truncation::{OutputTruncation, TruncatedOutput};
use super::{Tool, ToolInput, ToolOutput};
use crate::PureCore;
use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::session::CoreSession;
use crate::turn::CompileMode;

/// 子代理工具。
///
/// 将子任务委托给独立的 LLM 会话执行。
/// 子代理拥有独立的会话历史，共享父代理的 provider，不携带工具。
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
         to parallelize independent subtasks or isolate context. Optionally \
         choose one of the fixed model roles."
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
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let subagent_input = Self::parse_input(input.arguments)?;
            let role = Self::role(&subagent_input)?;
            let core = match &self.config {
                Some(config) => PureCore::from_config(config, role)?,
                None => match &self.reasoning_effort {
                    Some(effort) => {
                        PureCore::with_reasoning_effort(self.provider.clone(), effort.clone())
                    }
                    None => PureCore::new(self.provider.clone()),
                },
            };

            let mut request = crate::turn::TurnRequest::new(subagent_input.task, CompileMode::Auto);
            if let Some(max) = subagent_input.max_iterations {
                request = request.with_max_tool_iterations(max as usize);
            }
            if let Some(ref instructions) = self.workspace_instructions {
                request = request.with_workspace_instructions(instructions.clone());
            }

            let mut session = CoreSession::new();
            let (event_tx, _event_rx) = tokio::sync::broadcast::channel(256);

            let result = core.run_turn(&mut session, request, event_tx).await?;

            let description = if result.content.trim().is_empty() {
                "Subagent completed with no output.".to_string()
            } else {
                result.content.trim().to_string()
            };

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
