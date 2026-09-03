use std::{future::Future, sync::Arc};

use super::common::{RestartInput, WorkflowToolRuntime};
use crate::{
    PureError, RegisteredThreadMode, StaticTool, StaticToolDefinition, ToolBatchPolicy,
    ToolCallContext, ToolEffect, ToolName, ToolPolicy, ToolResult, TurnWorkingSetHandle,
    deserialize_tool_input,
};

pub const TOOL_WORKFLOW_RESTART: &str = "workflow_restart";

#[derive(Debug, Clone)]
pub struct WorkflowRestartTool(WorkflowToolRuntime);

impl WorkflowRestartTool {
    pub fn new(working_set: TurnWorkingSetHandle, mode: Arc<RegisteredThreadMode>) -> Self {
        Self(WorkflowToolRuntime::new(working_set, mode))
    }
}

impl StaticTool for WorkflowRestartTool {
    type Input = RestartInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin(TOOL_WORKFLOW_RESTART),
            "Archive the canonical workflow run and start a new lineage from the registered graph. Requires run, revision, and current-state CAS; this must be the only tool call in the response.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
            .with_effect(ToolEffect::AgentControl)
            .with_batch_policy(ToolBatchPolicy::Solo)
    }

    fn execute(
        &self,
        input: RestartInput,
        context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            let arguments =
                serde_json::to_value(&input).map_err(|error| PureError::ToolExecutionFailed {
                    tool: TOOL_WORKFLOW_RESTART.to_string(),
                    error: format!("failed to hash workflow restart arguments: {error}"),
                })?;
            let hash = crate::canonical_json_hash(&arguments);
            ToolResult::json(self.0.apply_restart(input, context.identity(), hash)?)
        }
    }
}

pub fn validate_workflow_restart_arguments(arguments: serde_json::Value) -> Result<(), PureError> {
    deserialize_tool_input::<RestartInput>(TOOL_WORKFLOW_RESTART, arguments).map(|_| ())
}
