use std::{future::Future, sync::Arc};

use super::common::{TransitionInput, WorkflowToolRuntime};
use crate::{
    PureError, RegisteredThreadMode, StaticTool, StaticToolDefinition, ToolBatchPolicy,
    ToolCallContext, ToolEffect, ToolName, ToolPolicy, ToolResult, TurnWorkingSetHandle,
    deserialize_tool_input,
};

pub const TOOL_WORKFLOW_TRANSITION: &str = "workflow_transition";

#[derive(Debug, Clone)]
pub struct WorkflowTransitionTool(WorkflowToolRuntime);

impl WorkflowTransitionTool {
    pub fn new(working_set: TurnWorkingSetHandle, mode: Arc<RegisteredThreadMode>) -> Self {
        Self(WorkflowToolRuntime::new(working_set, mode))
    }
}

impl StaticTool for WorkflowTransitionTool {
    type Input = TransitionInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin(TOOL_WORKFLOW_TRANSITION),
            "Transition the canonical workflow across one registered direct edge. Requires run, revision, and current-state CAS plus completion evidence; this must be the only tool call in the response.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
            .with_effect(ToolEffect::AgentControl)
            .with_batch_policy(ToolBatchPolicy::Solo)
    }

    fn execute(
        &self,
        input: TransitionInput,
        context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            let arguments =
                serde_json::to_value(&input).map_err(|error| PureError::ToolExecutionFailed {
                    tool: TOOL_WORKFLOW_TRANSITION.to_string(),
                    error: format!("failed to hash workflow transition arguments: {error}"),
                })?;
            let hash = crate::canonical_json_hash(&arguments);
            ToolResult::json(self.0.apply_transition(input, context.identity(), hash)?)
        }
    }
}

pub fn validate_workflow_transition_arguments(
    arguments: serde_json::Value,
) -> Result<(), PureError> {
    deserialize_tool_input::<TransitionInput>(TOOL_WORKFLOW_TRANSITION, arguments).map(|_| ())
}
