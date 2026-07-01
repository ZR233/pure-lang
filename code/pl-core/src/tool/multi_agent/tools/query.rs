use pl_protocol::PureError;
use pl_trace::AgentEvent;

use crate::tool::ToolRuntimeLockPolicy;
use crate::tool::recoverable::{
    recoverable_subagent_failures, recoverable_subagent_failures_message,
};

use super::super::types::{
    ListAgentsArgs, ListAgentsResult, ListAgentsTool, WaitAgentArgs, WaitAgentResult, WaitAgentTool,
};
use super::super::{
    BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, agent_tool_records, current_agent_path,
    json_output, unix_seconds,
};

impl Tool for WaitAgentTool {
    fn name(&self) -> &str {
        "wait_agent"
    }

    fn description(&self) -> &str {
        "Wait for managed sub-agent activity or completion. Use this after spawning agents."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "timeoutMs": {
                    "type": "integer",
                    "description": "Wait timeout in milliseconds. Defaults to 30000."
                },
                "includeDetails": {
                    "type": "boolean",
                    "description": "Return full AgentRecord entries instead of compact summaries. Defaults to false."
                }
            }
        })
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
                    timeout_ms: None,
                    include_details: false,
                });
            let sender_path = current_agent_path(&context);
            let _ = context.event_tx.send(AgentEvent::CollabWaitingBegin {
                call_id: input.tool_id.clone(),
                started_at: unix_seconds(),
                sender_path: sender_path.clone(),
            });
            let outcome = context
                .agent_control
                .wait_for_activity(
                    args.timeout_ms
                        .unwrap_or(super::super::types::DEFAULT_WAIT_TIMEOUT_MS),
                )
                .await;
            let _ = context.event_tx.send(AgentEvent::CollabWaitingEnd {
                call_id: input.tool_id,
                completed_at: unix_seconds(),
                sender_path,
                timed_out: outcome.timed_out,
            });
            let message = if outcome.timed_out {
                "wait_agent timed out before new agent activity.".to_string()
            } else {
                "wait_agent observed agent activity.".to_string()
            };
            let recoverable_failures = recoverable_subagent_failures(&outcome.agents);
            let message = if recoverable_failures.is_empty() {
                message
            } else {
                recoverable_subagent_failures_message(recoverable_failures.len())
            };
            json_output(WaitAgentResult {
                message,
                timed_out: outcome.timed_out,
                agents: agent_tool_records(&outcome.agents, args.include_details),
                recoverable_failures,
            })
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
        serde_json::json!({
            "type": "object",
            "properties": {
                "pathPrefix": {
                    "type": "string",
                    "description": "Optional canonical path prefix, such as /root/research."
                },
                "includeDetails": {
                    "type": "boolean",
                    "description": "Return full AgentRecord entries instead of compact summaries. Defaults to false."
                }
            }
        })
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
            let args: ListAgentsArgs =
                serde_json::from_value(input.arguments).unwrap_or(ListAgentsArgs {
                    path_prefix: None,
                    include_details: false,
                });
            let agents = context
                .agent_control
                .list_agents(args.path_prefix.as_deref())
                .await;
            let recoverable_failures = recoverable_subagent_failures(&agents);
            json_output(ListAgentsResult {
                agents: agent_tool_records(&agents, args.include_details),
                recoverable_failures,
            })
        })
    }
}
