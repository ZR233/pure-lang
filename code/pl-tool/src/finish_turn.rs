//! Explicit Turn handoff. Ending a Turn does not assert that the assigned task is complete.
use pl_core::{
    context::OpaquePayload,
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const TOOL_FINISH_TURN: &str = "finish_turn";

/// Full report, retained without truncation or whitespace normalization.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinishTurnInput {
    /// Complete Markdown report: outcome, evidence, problems and requested next action. No application length limit.
    #[schemars(length(min = 1))]
    pub message: String,
}

#[derive(Debug)]
pub struct FinishTurnTool;

#[derive(Debug, thiserror::Error)]
#[error("message must contain a non-whitespace character")]
struct EmptyReport;

/// Reads an immutable saved report without granting execution authority.
/// # Errors
/// Returns a decoding error for a malformed recognized payload.
pub fn saved_message(payload: &OpaquePayload) -> Result<Option<String>, serde_json::Error> {
    if payload.format() != "pl.tool.finish-turn" || payload.version() != 1 {
        return Ok(None);
    }
    Ok(Some(
        serde_json::from_str::<FinishTurnInput>(payload.content())?.message,
    ))
}

/// Registers explicit Turn-ending authority.
/// # Errors
/// Propagates invalid registration identity.
pub fn registration(declaration: OpaquePayload) -> Result<Registration, RegistryError> {
    Ok(
        Registration::new(TOOL_FINISH_TURN.into(), declaration, FinishTurnTool)?
            .with_turn_completion(),
    )
}

impl Tool for FinishTurnTool {
    async fn execute(&self, input: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
        let report: FinishTurnInput =
            serde_json::from_str(input.content()).map_err(ToolError::new)?;
        if report.message.trim().is_empty() {
            return Err(ToolError::new(EmptyReport));
        }
        let payload = OpaquePayload::new(
            "pl.tool.finish-turn",
            1,
            serde_json::to_string(&report).map_err(ToolError::new)?,
        )
        .map_err(ToolError::new)?;
        Ok(ToolOutput::new(
            payload,
            vec![pl_core::context::ContextContent::Text {
                text: report.message.into(),
            }],
        )
        .ending_turn())
    }
}
