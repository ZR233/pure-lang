use pl_protocol::{AgentStatus, PureError, SubAgentActivityKind};
use tokio::time::Duration;

use crate::tool::ToolRuntimeLockPolicy;

use super::super::schema::AgentControlToolKind;
use super::super::types::{
    ListAgentsArgs, ListAgentsResult, ListAgentsTool, WaitAgentArgs, WaitAgentResult, WaitAgentTool,
};
use super::super::{
    BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, agent_tool_records, current_agent_path,
    json_output,
};
use crate::agent::{
    AgentRecord, AgentWaitLoopError, AgentWaitLoopOptions, AgentWaitSnapshot,
    wait_for_agent_completion,
};

struct AgentWaitGroups {
    completed: Vec<AgentRecord>,
    pending: Vec<AgentRecord>,
}

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
                    target: None,
                    targets: Vec::new(),
                    timeout_ms: None,
                });
            let sender_path = current_agent_path(&context);
            let initial = context
                .agent_supervisor
                .list_agents(None)
                .await
                .map_err(wait_projection_error)?;
            let targets =
                resolve_wait_targets(&args, &context.agent_supervisor, &sender_path, &initial)
                    .await?;
            let timeout_ms = args
                .timeout_ms
                .unwrap_or(super::super::types::DEFAULT_WAIT_TIMEOUT_MS)
                .clamp(250, 120_000) as u64;
            let supervisor = context.agent_supervisor.clone();
            let cancellation_token = context
                .options
                .cancellation_token
                .clone()
                .unwrap_or_default();
            let outcome = wait_for_agent_completion(
                || {
                    let supervisor = supervisor.clone();
                    let targets = targets.clone();
                    async move {
                        let agents = supervisor
                            .list_agents(None)
                            .await
                            .map_err(wait_projection_error)?;
                        let groups = partition_agents(agents, &targets);
                        Ok::<_, PureError>((
                            AgentWaitSnapshot::from_group_counts(
                                groups.completed.len(),
                                groups.pending.len(),
                            ),
                            groups,
                        ))
                    }
                },
                AgentWaitLoopOptions::new(Duration::from_millis(timeout_ms)),
                &cancellation_token,
            )
            .await
            .map_err(|error| match error {
                AgentWaitLoopError::Cancelled => PureError::ToolExecutionFailed {
                    tool: "wait_agent".to_string(),
                    error: "wait_agent was cancelled".to_string(),
                },
                AgentWaitLoopError::Read(error) => error,
            })?;
            let timed_out = outcome.timed_out;
            crate::agent::emit_subagent_activity(
                &context.event_tx,
                input.tool_id,
                None,
                SubAgentActivityKind::WaitCompleted,
                Some(format!(
                    "{sender_path}: wait_agent returned {} completed and {} pending agents",
                    outcome.value.completed.len(),
                    outcome.value.pending.len()
                )),
                Some(timed_out),
                None,
            );
            json_output(WaitAgentResult {
                completed: agent_tool_records(&outcome.value.completed),
                pending: agent_tool_records(&outcome.value.pending),
                timed_out,
            })
        })
    }
}

async fn resolve_wait_targets(
    args: &WaitAgentArgs,
    supervisor: &crate::AgentSupervisor,
    sender_path: &str,
    agents: &[AgentRecord],
) -> Result<Vec<AgentRecord>, PureError> {
    let mut requested = Vec::new();
    for target in args.target.iter().chain(args.targets.iter()) {
        if !requested.contains(target) {
            requested.push(target.clone());
        }
    }
    if requested.is_empty() {
        return Ok(agents.to_vec());
    }
    let mut resolved = Vec::with_capacity(requested.len());
    for target in requested {
        let agent_id = supervisor
            .resolve_agent(sender_path, &target)
            .await
            .map_err(|_| PureError::ToolExecutionFailed {
                tool: "wait_agent".to_string(),
                error: format!("target agent not found: {target}"),
            })?;
        if !resolved
            .iter()
            .any(|agent: &AgentRecord| agent.id == agent_id)
        {
            let record = agents
                .iter()
                .find(|agent| agent.id == agent_id)
                .cloned()
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "wait_agent".to_string(),
                    error: format!("target agent disappeared during resolution: {target}"),
                })?;
            resolved.push(record);
        }
    }
    Ok(resolved)
}

fn partition_agents(agents: Vec<AgentRecord>, targets: &[AgentRecord]) -> AgentWaitGroups {
    let mut completed = Vec::new();
    let mut pending = Vec::new();
    let mut current = agents
        .into_iter()
        .map(|agent| (agent.id.clone(), agent))
        .collect::<std::collections::HashMap<_, _>>();
    for target in targets {
        let agent = current
            .remove(&target.id)
            .unwrap_or_else(|| missing_wait_target(target));
        match agent.status {
            AgentStatus::Completed
            | AgentStatus::Errored
            | AgentStatus::Interrupted
            | AgentStatus::Shutdown
            | AgentStatus::NotFound => completed.push(agent),
            AgentStatus::Queued | AgentStatus::Running | AgentStatus::Waiting => {
                pending.push(agent);
            }
        }
    }
    AgentWaitGroups { completed, pending }
}

fn missing_wait_target(target: &AgentRecord) -> AgentRecord {
    AgentRecord {
        status: AgentStatus::NotFound,
        summary: None,
        error: Some("target agent disappeared while waiting".to_string()),
        reason: None,
        budget_limit_kind: None,
        budget_usage: None,
        ..target.clone()
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
                .await
                .map_err(|error| PureError::ToolExecutionFailed {
                    tool: "list_agents".to_string(),
                    error: format!("durable agent projection failed: {error}"),
                })?;
            json_output(ListAgentsResult {
                agents: agent_tool_records(&agents),
            })
        })
    }
}

fn wait_projection_error(error: PureError) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "wait_agent".to_string(),
        error: format!("durable agent projection failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn explicit_unknown_wait_target_returns_actionable_error() {
        let supervisor = crate::AgentSupervisor::default();
        let error = resolve_wait_targets(
            &WaitAgentArgs {
                target: Some("missing".to_string()),
                targets: Vec::new(),
                timeout_ms: Some(250),
            },
            &supervisor,
            "/root",
            &[],
        )
        .await
        .expect_err("unknown explicit target must fail");

        assert!(
            error
                .to_string()
                .contains("target agent not found: missing")
        );
    }

    #[test]
    fn durable_waiting_snapshot_is_pending_while_completed_snapshot_is_completed() {
        let waiting = AgentRecord {
            id: "waiting".to_string(),
            path: "/root/waiting".to_string(),
            parent_path: Some("/root".to_string()),
            role: "executor".to_string(),
            task: "deliver".to_string(),
            status: AgentStatus::Waiting,
            summary: None,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            depth: 1,
            updated_at: 1,
        };
        let completed = AgentRecord {
            id: "completed".to_string(),
            path: "/root/completed".to_string(),
            status: AgentStatus::Completed,
            ..waiting.clone()
        };

        let targets = vec![waiting.clone(), completed.clone()];
        let groups = partition_agents(vec![waiting, completed], &targets);

        assert_eq!(groups.pending[0].id, "waiting");
        assert_eq!(groups.completed[0].id, "completed");
    }

    #[test]
    fn resolved_wait_target_missing_from_poll_is_not_an_empty_success() {
        let target = AgentRecord {
            id: "agent-closed".to_string(),
            path: "/root/closed".to_string(),
            parent_path: Some("/root".to_string()),
            role: "executor".to_string(),
            task: "implement".to_string(),
            status: AgentStatus::Running,
            summary: None,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            depth: 1,
            updated_at: 1,
        };
        let groups = partition_agents(Vec::new(), &[target]);

        assert_eq!(groups.completed.len(), 1);
        assert_eq!(groups.completed[0].id, "agent-closed");
        assert_eq!(groups.completed[0].status, AgentStatus::NotFound);
        assert!(groups.pending.is_empty());
    }
}
