use std::{future::Future, sync::Arc};

use super::common::{EmptyInput, WorkflowToolRuntime};
use crate::{
    RegisteredThreadMode, StaticTool, StaticToolDefinition, ToolCallContext, ToolName, ToolPolicy,
    ToolResult, TurnWorkingSetHandle,
};

pub const TOOL_WORKFLOW_CURRENT: &str = "workflow_current";

#[derive(Debug, Clone)]
pub struct WorkflowCurrentTool(WorkflowToolRuntime);

impl WorkflowCurrentTool {
    pub fn new(working_set: TurnWorkingSetHandle, mode: Arc<RegisteredThreadMode>) -> Self {
        Self(WorkflowToolRuntime::new(working_set, mode))
    }
}

impl StaticTool for WorkflowCurrentTool {
    type Input = EmptyInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin(TOOL_WORKFLOW_CURRENT),
            "Read the canonical Thread Mode workflow run, current state, graph revision, and CAS revision.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only().with_parallel_tool_calls()
    }

    fn execute(
        &self,
        _input: EmptyInput,
        _context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            let state = self.0.state();
            ToolResult::json(self.0.current_snapshot(&state))
        }
    }
}
