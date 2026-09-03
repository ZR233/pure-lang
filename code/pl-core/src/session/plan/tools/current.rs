use std::future::Future;

use super::common::{AgentSessionPlanToolBinding, AgentSessionPlanToolRuntime, EmptyInput};
use crate::{
    StaticTool, StaticToolDefinition, ToolCallContext, ToolName, ToolPolicy, ToolResult,
    TurnWorkingSetHandle,
};

pub const TOOL_PLAN_CURRENT: &str = "plan_current";

#[derive(Debug, Clone)]
pub struct PlanCurrentTool(AgentSessionPlanToolRuntime);

impl PlanCurrentTool {
    pub(crate) fn new(
        working_set: TurnWorkingSetHandle,
        binding: AgentSessionPlanToolBinding,
    ) -> Self {
        Self(AgentSessionPlanToolRuntime::new(working_set, binding))
    }
}

impl StaticTool for PlanCurrentTool {
    type Input = EmptyInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin(TOOL_PLAN_CURRENT),
            "Read this AgentSession's canonical Plan state, revision, complete document, pending confirmation, revision feedback, and all currently allowed transitions.",
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
        async move { ToolResult::json(self.0.read_machine()?.snapshot()) }
    }
}
