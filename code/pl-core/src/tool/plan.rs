use std::path::PathBuf;

use futures::FutureExt;
use pl_protocol::PureError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::truncation::OutputTruncation;
use super::{
    BoxFuture, Tool, ToolCallContext, ToolDirective, ToolInput, ToolResult, TypedTool,
    deserialize_tool_input,
};
use crate::turn::ToolEffect;

#[derive(Debug, Default)]
pub struct PlanExitTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanExitInput {
    /// The complete final plan in Markdown.
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
        TypedTool::<PlanExitInput>::new(self.name(), self.description()).input_schema()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Read)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            let args = deserialize_tool_input::<PlanExitInput>(self.name(), input.arguments)?;
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
            Ok(ToolResult::from_runtime_text(
                description,
                OutputTruncation::empty(),
                PathBuf::new(),
                Some(0),
                false,
                vec![ToolDirective::PlanCompleted {
                    content: args.content.trim().to_string(),
                }],
            ))
        }.boxed()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn context() -> ToolCallContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolCallContext::test(event_tx)
    }

    fn input(content: &str) -> ToolInput {
        ToolInput {
            arguments: serde_json::json!({ "content": content }),
        }
    }

    #[tokio::test]
    async fn submits_completed_plan() {
        let output = PlanExitTool
            .execute(input("# Plan\n\n- Do it"), context())
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<PlanExitResult>(&output.canonical_output()).unwrap(),
            PlanExitResult {
                status: "submitted".to_string(),
                message: "Plan submitted for user confirmation. Return a brief final acknowledgement and stop.".to_string(),
            }
        );
        assert_eq!(
            output.runtime_events,
            vec![ToolDirective::PlanCompleted {
                content: "# Plan\n\n- Do it".to_string(),
            }]
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
