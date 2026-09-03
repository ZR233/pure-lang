use std::future::Future;

use super::common::{AgentSessionPlanToolBinding, AgentSessionPlanToolRuntime, EmptyInput};
use crate::{
    StaticTool, StaticToolDefinition, ToolCallContext, ToolName, ToolPolicy, ToolResult,
    TurnWorkingSetHandle,
};

pub const TOOL_PLAN_HISTORY: &str = "plan_history";

#[derive(Debug, Clone)]
pub struct PlanHistoryTool(AgentSessionPlanToolRuntime);

impl PlanHistoryTool {
    pub(crate) fn new(
        working_set: TurnWorkingSetHandle,
        binding: AgentSessionPlanToolBinding,
    ) -> Self {
        Self(AgentSessionPlanToolRuntime::new(working_set, binding))
    }
}

impl StaticTool for PlanHistoryTool {
    type Input = EmptyInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin(TOOL_PLAN_HISTORY),
            "Read the canonical bounded Plan transition history and archived-history summary.",
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
            let state = self.0.read_state();
            ToolResult::json(serde_json::json!({
                "revision": state.revision,
                "state": state.state,
                "history": state.history_tail,
                "archivedTransitionCount": state.archived_transition_count,
                "archivedTransitionDigest": state.archived_transition_digest,
            }))
        }
    }
}
