use std::future::Future;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{AgentSessionPlanToolBinding, AgentSessionPlanToolRuntime, operation_id};
use crate::session::plan::AgentSessionPlanRestartCommand;
use crate::{
    PureError, StaticTool, StaticToolDefinition, ToolBatchPolicy, ToolCallContext, ToolEffect,
    ToolName, ToolPolicy, ToolResult, TurnWorkingSetHandle,
};

pub const TOOL_PLAN_RESTART: &str = "plan_restart";

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRestartInput {
    /// CAS revision returned by plan_current.
    expected_revision: u64,
    /// Why the current Plan lifecycle must be discarded.
    reason: String,
}

#[derive(Debug, Clone)]
pub struct PlanRestartTool(AgentSessionPlanToolRuntime);

impl PlanRestartTool {
    pub(crate) fn new(
        working_set: TurnWorkingSetHandle,
        binding: AgentSessionPlanToolBinding,
    ) -> Self {
        Self(AgentSessionPlanToolRuntime::new(working_set, binding))
    }
}

impl StaticTool for PlanRestartTool {
    type Input = PlanRestartInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin(TOOL_PLAN_RESTART),
            "Restart the fixed Plan state machine from an approved or revision-requested Plan. Requires revision CAS and a reason; this must be the only tool call in the response.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
            .with_effect(ToolEffect::AgentControl)
            .with_batch_policy(ToolBatchPolicy::Solo)
    }

    fn execute(
        &self,
        input: PlanRestartInput,
        context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            let arguments =
                serde_json::to_value(&input).map_err(|error| PureError::ToolExecutionFailed {
                    tool: TOOL_PLAN_RESTART.to_string(),
                    error: format!("failed to hash Plan restart arguments: {error}"),
                })?;
            let argument_hash = crate::canonical_json_hash(&arguments);
            let response = self.0.mutate(|machine| {
                let response = machine.restart(AgentSessionPlanRestartCommand {
                    expected_revision: input.expected_revision,
                    reason: input.reason,
                    operation_id: operation_id(context.identity()),
                    argument_hash,
                    restarted_at: crate::time::unix_seconds(),
                });
                let accepted = response.accepted;
                (response, accepted)
            })?;
            ToolResult::json(response)
        }
    }
}
