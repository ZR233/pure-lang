use std::future::Future;

use super::common::{AgentSessionPlanToolBinding, AgentSessionPlanToolRuntime, EmptyInput};
use crate::{
    StaticTool, StaticToolDefinition, ToolCallContext, ToolName, ToolPolicy, ToolResult,
    TurnWorkingSetHandle,
};

pub const TOOL_PLAN_NEXT: &str = "plan_next";

#[derive(Debug, Clone)]
pub struct PlanNextTool(AgentSessionPlanToolRuntime);

impl PlanNextTool {
    pub(crate) fn new(
        working_set: TurnWorkingSetHandle,
        binding: AgentSessionPlanToolBinding,
    ) -> Self {
        Self(AgentSessionPlanToolRuntime::new(working_set, binding))
    }
}

impl StaticTool for PlanNextTool {
    type Input = EmptyInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin(TOOL_PLAN_NEXT),
            "Read every fixed transition available from the canonical Plan state, including actor, condition, target state, and exact next action.",
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
            let machine = self.0.read_machine()?;
            ToolResult::json(serde_json::json!({
                "revision": machine.state().revision,
                "state": machine.state().state,
                "transitions": machine.available_transitions(),
            }))
        }
    }
}
