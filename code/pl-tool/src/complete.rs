//! Root turn completion tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    /// Concise plain text or Markdown. Use actual paragraph newlines; do not JSON-encode the text a second time. Keep literal backslashes only in code or paths.
    #[schemars(length(min = 1, max = 8192))]
    summary: String,
    /// Optional evidence supporting the completion summary.
    #[serde(default)]
    #[schemars(length(max = 16))]
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompleteOutput {
    status: CompletionStatus,
    summary: String,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum CompletionStatus {
    Completed,
}

/// Reads the original completion summary for historical display without granting Turn authority.
/// Unknown formats or versions return `None` and remain available as raw records.
/// # Errors
/// Returns the producer decoding error when a recognized saved receipt is malformed.
pub fn saved_completion_summary(
    payload: &pl_core::context::OpaquePayload,
) -> std::result::Result<Option<String>, serde_json::Error> {
    if payload.format() != "pl.tool.complete" || payload.version() != 1 {
        return Ok(None);
    }
    let receipt: CompleteOutput = serde_json::from_str(payload.content())?;
    Ok(Some(receipt.summary))
}

fn completion_result(input: CompleteInput) -> Result<CompleteOutput> {
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
        status: CompletionStatus::Completed,
        summary,
        evidence,
    };
    Ok(output)
}

/// Constructs the completion tool for a protocol-independent Thread, with explicit ending authority.
/// The declaration is encoded by the host/model boundary.
///
/// # Errors
/// Propagates invalid registration identity.
pub fn registration(
    declaration: pl_core::context::OpaquePayload,
) -> std::result::Result<pl_core::tool::opaque::Registration, pl_core::tool::opaque::RegistryError>
{
    Ok(
        pl_core::tool::opaque::Registration::new(TOOL_COMPLETE.into(), declaration, CompleteTool)?
            .with_turn_completion(),
    )
}

impl pl_core::tool::opaque::Tool for CompleteTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        _context: pl_core::tool::opaque::CallContext,
    ) -> std::result::Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        let input: CompleteInput =
            serde_json::from_str(input.content()).map_err(pl_core::tool::opaque::ToolError::new)?;
        let output = completion_result(input).map_err(pl_core::tool::opaque::ToolError::new)?;
        let payload =
            serde_json::to_string(&output).map_err(pl_core::tool::opaque::ToolError::new)?;
        let payload = pl_core::context::OpaquePayload::new("pl.tool.complete", 1, payload)
            .map_err(pl_core::tool::opaque::ToolError::new)?;
        Ok(pl_core::tool::ToolOutput::new(
            payload,
            vec![pl_core::context::ContextContent::Text {
                text: std::sync::Arc::from(output.summary),
            }],
        )
        .ending_turn())
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
    use crate::test_support::{ToolTestExt, input};
    use pl_core::tool::opaque::CallContext;
    use pretty_assertions::assert_eq;

    fn context() -> CallContext {
        crate::test_support::thread_context()
    }

    #[tokio::test]
    async fn completion_preserves_markdown_newlines_and_literal_code_escapes() {
        for summary in [
            "Result\n\nVerified.",
            r"Literal \n in code",
            "```python\nvalue = r'C:\\new'\n```",
        ] {
            let output = pl_core::tool::opaque::Tool::execute(
                &CompleteTool,
                input(serde_json::json!({"summary": summary})),
                context(),
            )
            .await
            .unwrap();
            assert_eq!(
                saved_completion_summary(output.payload())
                    .unwrap()
                    .as_deref(),
                Some(summary)
            );
        }
    }

    #[test]
    fn schema_requires_summary_and_rejects_unknown_fields() {
        let schema = schemars::schema_for!(CompleteInput).to_value();
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
                input(serde_json::json!({
                    "summary": "  Work is complete.  ",
                    "evidence": ["cargo test passed"]
                })),
                context(),
            )
            .await
            .expect("completion should succeed");

        assert_eq!(result.control(), pl_core::tool::ToolControl::EndTurn);
        assert!(result.payload().content().contains("Work is complete."));
        assert!(result.payload().content().contains("cargo test passed"));
    }

    #[tokio::test]
    async fn invalid_completion_is_rejected() {
        let result = CompleteTool
            .execute_raw(input(serde_json::json!({"summary": "  "})), context())
            .await
            .expect_err("empty summary must be rejected");
        assert!(result.to_string().contains("summary must be non-empty"));

        let result = CompleteTool
            .execute_raw(
                input(serde_json::json!({
                    "summary": "done",
                    "unexpected": true
                })),
                context(),
            )
            .await
            .expect_err("unknown fields must be rejected");
        assert!(result.to_string().contains("unknown field `unexpected`"));

        let result = CompleteTool
            .execute_raw(
                input(serde_json::json!({
                    "summary": "done",
                    "evidence": vec!["ok"; MAX_EVIDENCE_ITEMS + 1]
                })),
                context(),
            )
            .await
            .expect_err("oversized evidence lists must be rejected");
        assert!(result.to_string().contains("at most 16 items"));
    }
    #[tokio::test]
    async fn dynamic_completion_returns_opaque_business_payload_and_typed_turn_control() {
        let input = pl_core::context::OpaquePayload::new(
            "application/json",
            1,
            r#"{"summary":"  Completed work  ","evidence":["verified"]}"#,
        )
        .unwrap();
        let context = pl_core::tool::opaque::CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: tokio_util::sync::CancellationToken::new(),
            extensions: Default::default(),
            extension_sequence: 0,
            catalog: std::sync::Arc::from([]),
        };
        let output = pl_core::tool::opaque::Tool::execute(&CompleteTool, input, context)
            .await
            .unwrap();
        assert_eq!(output.control(), pl_core::tool::ToolControl::EndTurn);
        assert_eq!(output.payload().format(), "pl.tool.complete");
        let payload: serde_json::Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(payload["summary"], "Completed work");
        assert_eq!(payload["evidence"], serde_json::json!(["verified"]));
        assert_eq!(
            output.context(),
            &[pl_core::context::ContextContent::Text {
                text: std::sync::Arc::from("Completed work")
            }]
        );
    }
}
