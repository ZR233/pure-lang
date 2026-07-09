use pl_protocol::{PureError, SubAgentActivityKind};

use crate::tool::ToolRuntimeLockPolicy;

use super::super::schema::AgentControlToolKind;
use super::super::types::{
    ListAgentsArgs, ListAgentsResult, ListAgentsTool, WaitAgentArgs, WaitAgentResult, WaitAgentTool,
};
use super::super::{
    BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, agent_tool_records, current_agent_path,
    json_output,
};

impl Tool for WaitAgentTool {
    fn name(&self) -> &str {
        "wait_agent"
    }

    fn description(&self) -> &str {
        "Wait for managed sub-agent activity or completion. Use this after spawning agents."
    }

    fn input_schema(&self) -> serde_json::Value {
        AgentControlToolKind::WaitAgent.input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        ToolRuntimeLockPolicy::None
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: WaitAgentArgs =
                serde_json::from_value(input.arguments).unwrap_or(WaitAgentArgs {
                    _target: None,
                    _targets: Vec::new(),
                    timeout_ms: None,
                });
            let sender_path = current_agent_path(&context);
            let outcome = context
                .agent_supervisor
                .wait_for_activity(
                    args.timeout_ms
                        .unwrap_or(super::super::types::DEFAULT_WAIT_TIMEOUT_MS),
                )
                .await;
            let timed_out = outcome.timed_out();
            let message = if timed_out {
                "wait_agent timed out before new agent activity.".to_string()
            } else {
                "wait_agent observed agent activity.".to_string()
            };
            crate::agent::emit_subagent_activity(
                &context.event_tx,
                input.tool_id,
                None,
                SubAgentActivityKind::WaitCompleted,
                Some(format!("{sender_path}: {message}")),
                Some(timed_out),
                None,
            );
            json_output(WaitAgentResult { message, timed_out })
        })
    }
}

impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "List known managed sub-agents in the current collaboration tree."
    }

    fn input_schema(&self) -> serde_json::Value {
        AgentControlToolKind::ListAgents.input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: ListAgentsArgs = serde_json::from_value(input.arguments)
                .unwrap_or(ListAgentsArgs { path_prefix: None });
            let agents = context
                .agent_supervisor
                .list_agents(args.path_prefix.as_deref())
                .await;
            json_output(ListAgentsResult {
                agents: agent_tool_records(&agents),
            })
        })
    }
}
