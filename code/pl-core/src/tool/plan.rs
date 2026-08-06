use std::path::PathBuf;

use pl_protocol::PureError;
use serde::{Deserialize, Serialize};

use super::truncation::OutputTruncation;
use super::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};

#[derive(Debug, Default)]
pub struct PlanExitTool;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanExitInput {
    content: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct PlanExitResult {
    status: String,
    message: String,
}

impl Tool for PlanExitTool {
    fn name(&self) -> &str {
        "plan_exit"
    }

    fn description(&self) -> &str {
        "Submit the final Task Mode Markdown plan for user confirmation. \
         Use only after the plan is complete; this tool does not execute the plan."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The complete final plan in Markdown."
                }
            },
            "required": ["content"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: PlanExitInput = serde_json::from_value(input.arguments).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: format!("invalid input: {error}"),
                }
            })?;
            if args.content.trim().is_empty() {
                return Err(PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: "content must not be empty".to_string(),
                });
            }

            let description = serde_json::to_string(&PlanExitResult {
                status: "submitted".to_string(),
                message: "Plan submitted for user confirmation. Return a brief final acknowledgement and stop.".to_string(),
            })?;
            Ok(ToolOutput {
                description,
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::WorkspaceAccess;
    use crate::{AgentSession, TurnOptions};

    fn context() -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            lsp_runtime: None,
            parent_session: Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        }
    }

    fn input(content: &str) -> ToolInput {
        ToolInput {
            arguments: serde_json::json!({ "content": content }),
            session_id: "session-1".to_string(),
            tool_id: "call-1".to_string(),
            revision_base: 0,
        }
    }

    #[tokio::test]
    async fn submits_completed_plan() {
        let output = PlanExitTool
            .execute(input("# Plan\n\n- Do it"), context())
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<PlanExitResult>(&output.description).unwrap(),
            PlanExitResult {
                status: "submitted".to_string(),
                message: "Plan submitted for user confirmation. Return a brief final acknowledgement and stop.".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn rejects_empty_content() {
        let error = PlanExitTool
            .execute(input("  "), context())
            .await
            .unwrap_err();

        assert!(error.to_string().contains("content must not be empty"));
    }
}
