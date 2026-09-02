//! Root turn completion tool.

use std::future::Future;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{StaticTool, ToolBatchPolicy, ToolCallContext, ToolPolicy, ToolResult};
use crate::turn::ToolEffect;
use pl_protocol::{PureError, Result};

pub const TOOL_COMPLETE: &str = "complete";

const MAX_SUMMARY_BYTES: usize = 8 * 1024;
const MAX_EVIDENCE_ITEMS: usize = 16;
const MAX_EVIDENCE_BYTES: usize = 2 * 1024;

/// Completes the current root turn with a concise, auditable result.
#[derive(Debug, Default)]
pub struct CompleteTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompleteInput {
    /// A concise summary of the result delivered by this turn.
    #[schemars(length(min = 1, max = 8192))]
    summary: String,
    /// Optional evidence supporting the completion summary.
    #[serde(default)]
    #[schemars(length(max = 16))]
    evidence: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompleteOutput {
    status: &'static str,
    summary: String,
    evidence: Vec<String>,
}

impl StaticTool for CompleteTool {
    type Input = CompleteInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin(TOOL_COMPLETE),
            "Complete the current root turn with a summary and optional evidence. This must be the final and only tool call in the provider response.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
            .with_effect(ToolEffect::Read)
            .with_batch_policy(ToolBatchPolicy::Solo)
    }

    fn execute(
        &self,
        input: CompleteInput,
        _context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult>> + Send {
        async move {
            let summary = input.summary.trim().to_string();
            if summary.is_empty() || summary.len() > MAX_SUMMARY_BYTES {
                return Err(invalid_completion(format!(
                    "summary must be non-empty and at most {MAX_SUMMARY_BYTES} bytes"
                )));
            }
            if input.evidence.len() > MAX_EVIDENCE_ITEMS {
                return Err(invalid_completion(format!(
                    "evidence may contain at most {MAX_EVIDENCE_ITEMS} items"
                )));
            }
            let evidence = input
                .evidence
                .into_iter()
                .map(|item| item.trim().to_string())
                .collect::<Vec<_>>();
            if evidence
                .iter()
                .any(|item| item.is_empty() || item.len() > MAX_EVIDENCE_BYTES)
            {
                return Err(invalid_completion(format!(
                    "each evidence item must be non-empty and at most {MAX_EVIDENCE_BYTES} bytes"
                )));
            }
            let output = CompleteOutput {
                status: "completed",
                summary: summary.clone(),
                evidence,
            };
            Ok(ToolResult::json(output)?.ending_turn_with_content(summary))
        }
    }
}

fn invalid_completion(message: String) -> PureError {
    PureError::ToolExecutionFailed {
        tool: TOOL_COMPLETE.to_string(),
        error: message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{StaticToolTestExt, ToolInput};
    use pretty_assertions::assert_eq;

    fn context() -> ToolCallContext {
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        ToolCallContext::new(Default::default(), event_tx)
    }

    #[test]
    fn schema_requires_summary_and_rejects_unknown_fields() {
        let schema = CompleteTool.input_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], serde_json::json!(["summary"]));
        assert_eq!(schema["properties"]["summary"]["minLength"], 1);
        assert_eq!(schema["properties"]["summary"]["maxLength"], 8192);
        assert!(schema["properties"].get("evidence").is_some());
        assert_eq!(schema["properties"]["evidence"]["maxItems"], 16);
        assert_eq!(schema["additionalProperties"], false);
    }

    #[tokio::test]
    async fn valid_completion_returns_summary_and_ends_turn() {
        let result = CompleteTool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "summary": "  Work is complete.  ",
                        "evidence": ["cargo test passed"]
                    }),
                },
                context(),
            )
            .await
            .expect("completion should succeed");
        assert!(result.success);
        assert!(result.ends_turn());
        assert_eq!(result.end_turn_content(), Some("Work is complete."));
        assert!(result.canonical_output().contains("cargo test passed"));
    }

    #[tokio::test]
    async fn invalid_completion_is_rejected() {
        let result = CompleteTool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({"summary": "  "}),
                },
                context(),
            )
            .await
            .expect_err("empty summary must be rejected");
        assert!(result.to_string().contains("summary must be non-empty"));

        let result = CompleteTool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "summary": "done",
                        "unexpected": true
                    }),
                },
                context(),
            )
            .await
            .expect_err("unknown fields must be rejected");
        assert!(result.to_string().contains("unknown field `unexpected`"));

        let result = CompleteTool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "summary": "done",
                        "evidence": vec!["ok"; MAX_EVIDENCE_ITEMS + 1]
                    }),
                },
                context(),
            )
            .await
            .expect_err("oversized evidence lists must be rejected");
        assert!(result.to_string().contains("at most 16 items"));
    }
}
