//! Task 执行停止控制。任务完成统一由 `task_transition` 提交。

use std::sync::Arc;

use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{TaskCoordinator, TaskRun, TaskStopOrigin, TaskStopReason};
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};
use crate::{AgentRuntimeHandle, AgentState, ToolEffect};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StopTaskInput {
    /// Human-readable reason for interrupting the current task executions.
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskStopOutput {
    status: &'static str,
    run: TaskRun,
}

impl TaskCoordinator {
    pub(crate) fn task_stop_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<StopTaskInput>::new(
            "task_stop",
            "Persist a stop event, advance the execution generation, and interrupt active model turns without changing the Task state.",
        )
        .registered(move |input: StopTaskInput, _context| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            let runtime = runtime.clone();
            async move {
                let Some(reason) = TaskStopReason::new(input.reason) else {
                    bail!("task_stop reason must not be empty");
                };
                let output = coordinator
                    .stop_task(
                        &thread_id,
                        &runtime,
                        TaskStopOrigin::PlannerDecision,
                        reason,
                    )
                    .await?;
                ToolExecutionResult::<serde_json::Value>::json(output)
                    .map(ToolExecutionResult::ending_turn)
                    .map_err(anyhow::Error::from)
            }
        })
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) async fn stop_task(
        &self,
        thread_id: &str,
        runtime: &AgentRuntimeHandle,
        origin: TaskStopOrigin,
        reason: TaskStopReason,
    ) -> Result<TaskStopOutput> {
        let stopped = self
            .task_runtime
            .stop_task(thread_id, origin, &reason)
            .await?;
        interrupt_task_agents(runtime, thread_id, origin).await?;
        Ok(TaskStopOutput {
            status: "interrupted",
            run: stopped,
        })
    }
}

async fn interrupt_task_agents(
    runtime: &AgentRuntimeHandle,
    thread_id: &str,
    origin: TaskStopOrigin,
) -> Result<()> {
    let root = crate::studio::agent_host::root_agent_id(thread_id);
    let agents = runtime
        .list()
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .into_iter()
        .filter(|snapshot| {
            snapshot.identity.parent_id.as_ref() == Some(&root)
                || (origin.stops_root_turn() && snapshot.identity.id == root)
        })
        .filter(|snapshot| {
            !matches!(
                snapshot.state,
                AgentState::Closing(_) | AgentState::Closed(_)
            )
        })
        .collect::<Vec<_>>();
    for agent in agents {
        if let Some(turn_id) = agent.active_turn_id().cloned() {
            runtime
                .cancel_turn(agent.identity.id, turn_id)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
    }
    Ok(())
}
