//! 固定 Plan 状态机的成套模型工具。

mod common;
mod current;
mod history;
mod next;
mod restart;
mod submit;

pub(crate) use common::AgentSessionPlanToolBinding;
pub use current::{PlanCurrentTool, TOOL_PLAN_CURRENT};
pub use history::{PlanHistoryTool, TOOL_PLAN_HISTORY};
pub use next::{PlanNextTool, TOOL_PLAN_NEXT};
pub use restart::{PlanRestartTool, TOOL_PLAN_RESTART};
pub use submit::{PlanSubmitTool, TOOL_PLAN_SUBMIT};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StaticTool, ToolBatchPolicy, ToolEffect, TurnWorkingSetHandle};

    fn assert_query_policy(tool: &impl StaticTool, expected_name: &str) {
        assert_eq!(tool.definition().name().wire_name(), expected_name);
        let policy = tool.policy();
        assert_eq!(policy.effect(), Some(ToolEffect::Read));
        assert_eq!(policy.batch_policy(), ToolBatchPolicy::Coexist);
        assert!(policy.supports_parallel_tool_calls());
    }

    fn assert_mutation_policy(tool: &impl StaticTool, expected_name: &str) {
        assert_eq!(tool.definition().name().wire_name(), expected_name);
        let policy = tool.policy();
        assert_eq!(policy.effect(), Some(ToolEffect::AgentControl));
        assert_eq!(policy.batch_policy(), ToolBatchPolicy::Solo);
        assert!(!policy.supports_parallel_tool_calls());
    }

    #[test]
    fn plan_tool_group_uses_the_unified_static_tool_contract() {
        let working_set = TurnWorkingSetHandle::default();
        let binding = AgentSessionPlanToolBinding::new(crate::AgentSessionPlanOptions::default());

        assert_query_policy(
            &PlanCurrentTool::new(working_set.clone(), binding.clone()),
            TOOL_PLAN_CURRENT,
        );
        assert_query_policy(
            &PlanNextTool::new(working_set.clone(), binding.clone()),
            TOOL_PLAN_NEXT,
        );
        assert_query_policy(
            &PlanHistoryTool::new(working_set.clone(), binding.clone()),
            TOOL_PLAN_HISTORY,
        );
        assert_mutation_policy(
            &PlanSubmitTool::new(working_set.clone(), binding.clone()),
            TOOL_PLAN_SUBMIT,
        );
        let submit_description = PlanSubmitTool::new(working_set.clone(), binding.clone())
            .definition()
            .description()
            .to_string();
        assert!(
            submit_description.contains("only tool for asking the user to approve implementation")
        );
        assert!(submit_description.contains("do not first ask whether to implement"));
        assert!(submit_description.contains("request_user_input or final text"));
        assert_mutation_policy(
            &PlanRestartTool::new(working_set, binding),
            TOOL_PLAN_RESTART,
        );
    }
}
