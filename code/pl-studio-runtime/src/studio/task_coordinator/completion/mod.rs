use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_protocol::{TodoListSnapshot, TodoStatus};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    ReviewScope, ReviewVerdict, TaskCoordinator, TaskRunPhase, TaskRunRecord, TaskStopOrigin,
    TaskStopReason, ThreadExecutionStatus, WorkUnitStatus,
};
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};
use crate::{AgentLifecycleState, AgentRuntimeHandle, ToolEffect};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CompleteTaskInput {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StopTaskInput {
    /// Human-readable reason for stopping the task.
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskCompletionOutput {
    run: TaskRunRecord,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
enum TaskCompleteOutcome {
    Completed(Box<TaskCompletionOutput>),
    Rejected {
        code: &'static str,
        recoverable: bool,
        message: String,
    },
}

impl TaskCompleteOutcome {
    fn rejected(code: &'static str, message: impl Into<String>) -> Self {
        Self::Rejected {
            code,
            recoverable: true,
            message: message.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskStopOutput {
    status: &'static str,
    run: TaskRunRecord,
    deferred_agent_id: Option<String>,
}

impl TaskCoordinator {
    pub(crate) fn task_complete_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<CompleteTaskInput>::new(
            "task_complete",
            "Complete a design-consistent task whose deliveries are merged or recorded as \
             no-delivery. Requires a current integrated review when source merges exist and an \
             all-completed todo list when one exists.",
        )
        .registered(move |_: CompleteTaskInput, context| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            async move {
                let todo = context.working_set.current_todo();
                let outcome = coordinator.complete_task(&thread_id, todo.as_ref()).await?;
                let output = serde_json::to_string(&outcome)?;
                Ok::<ToolExecutionResult<serde_json::Value>, anyhow::Error>(match outcome {
                    TaskCompleteOutcome::Completed(_) => {
                        ToolExecutionResult::<serde_json::Value>::success(output).ending_turn()
                    }
                    TaskCompleteOutcome::Rejected { .. } => {
                        ToolExecutionResult::<serde_json::Value>::failure(output)
                    }
                })
            }
        })
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) fn task_stop_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<StopTaskInput>::new(
            "task_stop",
            "Stop the current task after safely settling agents and branch state.",
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
        let mut terminal_facts = self.subscribe_terminal_facts();
        let requested = {
            let branch_guard = self.lock_branch_mutation().await;
            self.ensure_branch_mutation_guard(&branch_guard)?;
            let _allocation_guard = self.allocation_lock.lock().await;
            let run = self
                .preflight_task_stop_request_locked(thread_id, &branch_guard)
                .await?;
            self.store
                .request_task_stop(&run.id, &run.expected_head, origin, &reason)
                .await?
        };
        interrupt_task_agents(runtime, thread_id, origin).await?;
        wait_for_terminal_outcomes(self, &requested.id, &mut terminal_facts).await?;
        let run = self
            .store
            .read_task_run(&requested.id)
            .await?
            .context("task run disappeared after stop request")?;
        if let Some(executor_thread_id) = self.awaiting_completion_executor(&run).await? {
            return Ok(TaskStopOutput {
                status: "deferred",
                run,
                deferred_agent_id: Some(executor_thread_id),
            });
        }
        let run = {
            let branch_guard = self.lock_branch_mutation().await;
            self.ensure_branch_mutation_guard(&branch_guard)?;
            let _allocation_guard = self.allocation_lock.lock().await;
            let run = self
                .preflight_task_stop_locked(thread_id, &branch_guard)
                .await?;
            self.store
                .begin_task_stop(&run.id, &run.expected_head, requested.task_generation)
                .await?
        };
        self.store
            .settle_agents_for_task_stop(&run.id, requested.task_generation, reason.as_str())
            .await?;
        close_task_children(runtime, thread_id).await?;
        let branch_guard = self.lock_branch_mutation().await;
        let stopped = self
            .stop_task_locked(
                &run.id,
                requested.task_generation,
                reason.as_str(),
                &branch_guard,
            )
            .await?;
        Ok(TaskStopOutput {
            status: "cancelled",
            run: stopped,
            deferred_agent_id: None,
        })
    }

    async fn complete_task(
        &self,
        thread_id: &str,
        todo: Option<&TodoListSnapshot>,
    ) -> Result<TaskCompleteOutcome> {
        let guard = self.lock_branch_mutation().await;
        self.ensure_branch_mutation_guard(&guard)?;
        let run = self
            .store
            .read_active_task_run_for_root_thread(thread_id)
            .await?;
        if run.phase != TaskRunPhase::Reviewing {
            return Ok(TaskCompleteOutcome::rejected(
                "wrongPhase",
                format!(
                    "task_complete requires reviewing phase; current phase is {}",
                    run.phase.as_str()
                ),
            ));
        }
        if run.stop_requested {
            return Ok(TaskCompleteOutcome::rejected(
                "stopRequested",
                "task_complete is unavailable after task_stop was requested",
            ));
        }
        if let Some(message) = incomplete_todo_message(todo) {
            return Ok(TaskCompleteOutcome::rejected("todoIncomplete", message));
        }
        if run.design_commit.as_deref() != Some(run.expected_head.as_str()) {
            return Ok(TaskCompleteOutcome::rejected(
                "repositoryDrift",
                "task design is not recorded at the current task HEAD",
            ));
        }
        let work_units = self.store.list_work_units(&run.id).await?;
        if work_units.iter().any(|unit| {
            !matches!(
                unit.status,
                WorkUnitStatus::Merged | WorkUnitStatus::NoDelivery
            )
        }) {
            return Ok(TaskCompleteOutcome::rejected(
                "deliveriesIncomplete",
                "all executor deliveries must be merged or recorded as no-delivery",
            ));
        }
        // 无 source merge（全部 NoDelivery）时没有可审查的集成 diff，
        // 不再强制 integrated review。
        let has_source_merge = !self.store.list_merge_records(&run.id).await?.is_empty();
        if has_source_merge {
            let latest_review = self.store.list_review_rounds(&run.id).await?.pop();
            if !latest_review.is_some_and(|review| {
                review.scope == ReviewScope::Integrated
                    && review.reviewed_head == run.expected_head
                    && review.verdict == ReviewVerdict::Pass
            }) {
                return Ok(TaskCompleteOutcome::rejected(
                    "reviewMissing",
                    "latest integrated review must pass for the current task HEAD",
                ));
            }
        }
        // 子 Thread 的 WorkUnit 已全部结算，其残留 Interaction 不再阻塞完成；
        // 只有 root 自身的 pending Interaction 仍是未闭合的用户边界。
        let pending_interactions = self.store.list_pending_interactions(thread_id).await?;
        if !pending_interactions.is_empty() {
            const PREVIEW_LIMIT: usize = 8;
            let total = pending_interactions.len();
            let preview = pending_interactions
                .iter()
                .take(PREVIEW_LIMIT)
                .map(|interaction| format!("{}/{}", thread_id, interaction.interaction_id))
                .collect::<Vec<_>>()
                .join(", ");
            let remaining = total.saturating_sub(PREVIEW_LIMIT);
            let suffix = if remaining == 0 {
                String::new()
            } else {
                format!("，另有 {remaining} 条")
            };
            return Ok(TaskCompleteOutcome::rejected(
                "pendingInteraction",
                format!(
                    "Task root Thread 仍有 {total} 条 pending Interaction：{preview}{suffix}；请先解决或取消后重试 task_complete"
                ),
            ));
        }
        if let Err(error) = self.ensure_process_lease_owned(&run) {
            return Ok(TaskCompleteOutcome::rejected(
                "repositoryDrift",
                error.to_string(),
            ));
        }
        if let Err(error) = super::review::validate_review_repository(&run).await {
            return Ok(TaskCompleteOutcome::rejected(
                "repositoryDrift",
                error.to_string(),
            ));
        }
        let completed = self
            .store
            .complete_reviewed_task(thread_id, &run.expected_head)
            .await;
        let completed = match completed {
            Ok(completed) => completed,
            Err(error) => {
                if let Some(pending) =
                    error.downcast_ref::<crate::studio::store::PendingTaskInteractions>()
                {
                    return Ok(TaskCompleteOutcome::rejected(
                        "pendingInteraction",
                        pending.user_message(),
                    ));
                }
                return Ok(TaskCompleteOutcome::rejected(
                    "repositoryDrift",
                    format!("task completion state changed before commit: {error}"),
                ));
            }
        };
        self.release_owned_process_lease(&run.id);
        Ok(TaskCompleteOutcome::Completed(Box::new(
            TaskCompletionOutput { run: completed },
        )))
    }

    pub(super) async fn preflight_task_stop_locked(
        &self,
        thread_id: &str,
        guard: &super::BranchMutationGuard<'_>,
    ) -> Result<TaskRunRecord> {
        self.ensure_branch_mutation_guard(guard)?;
        let run = self
            .store
            .read_active_task_run_for_root_thread(thread_id)
            .await?;
        self.validate_stop_request(&run).await?;
        if let Some(executor_thread_id) = self.awaiting_completion_executor(&run).await? {
            bail!(
                "task_stop deferred: executor {} finished without report_completion; \
                 request an explicit completion before stopping the task",
                executor_thread_id
            );
        }
        Ok(run)
    }

    async fn preflight_task_stop_request_locked(
        &self,
        thread_id: &str,
        guard: &super::BranchMutationGuard<'_>,
    ) -> Result<TaskRunRecord> {
        self.ensure_branch_mutation_guard(guard)?;
        let run = self
            .store
            .read_active_task_run_for_root_thread(thread_id)
            .await?;
        self.validate_stop_request(&run).await?;
        Ok(run)
    }

    pub(super) async fn stop_task_locked(
        &self,
        task_run_id: &str,
        expected_generation: u64,
        reason: &str,
        guard: &super::BranchMutationGuard<'_>,
    ) -> Result<TaskRunRecord> {
        self.ensure_branch_mutation_guard(guard)?;
        let mut run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found while stopping")?;
        self.validate_stop_request(&run).await?;
        if let Some(executor_thread_id) = self.awaiting_completion_executor(&run).await? {
            bail!(
                "task_stop deferred: executor {} still requires report_completion",
                executor_thread_id
            );
        }
        let has_source_merge = !self.store.list_merge_records(&run.id).await?.is_empty();
        if !has_source_merge {
            if run.design_commit.is_some() {
                self.revert_design_for_no_source_cancel_locked(&run.id, guard)
                    .await?;
                run = self
                    .store
                    .read_task_run(&run.id)
                    .await?
                    .context("task run disappeared after design revert")?;
            }
        } else if run.design_commit.as_deref() != Some(run.expected_head.as_str()) {
            bail!("task_stop requires a final design consistency update after source merges");
        }
        super::review::validate_review_repository(&run).await?;
        let cancelled = self
            .store
            .cancel_task_and_release_lease(&run.id, &run.expected_head, expected_generation, reason)
            .await?;
        self.release_owned_process_lease(&run.id);
        self.publish_terminal_fact(&cancelled.id);
        Ok(cancelled)
    }

    async fn awaiting_completion_executor(&self, run: &TaskRunRecord) -> Result<Option<String>> {
        let work_unit = self
            .store
            .list_work_units(&run.id)
            .await?
            .into_iter()
            .find(|work_unit| work_unit.status == super::WorkUnitStatus::AwaitingCompletion);
        let Some(work_unit) = work_unit else {
            return Ok(None);
        };
        Ok(work_unit.executor_thread_id)
    }

    async fn validate_stop_request(&self, run: &TaskRunRecord) -> Result<()> {
        self.ensure_process_lease_owned(run)?;
        if run.phase == TaskRunPhase::Merging {
            bail!("task_stop requires Planner Git integration to be recorded first");
        }
        super::review::validate_review_repository(run).await?;
        let merges = self.store.list_merge_records(&run.id).await?;
        if !merges.is_empty() && run.design_commit.as_deref() != Some(run.expected_head.as_str()) {
            bail!("task_stop requires a final design consistency update after source merges");
        }
        Ok(())
    }
}

/// 存在未完成条目时返回拒绝说明；无 todo 或全部 completed 返回 `None`。
fn incomplete_todo_message(todo: Option<&TodoListSnapshot>) -> Option<String> {
    let snapshot = todo?;
    let unfinished = snapshot
        .items
        .iter()
        .filter(|item| item.status != TodoStatus::Completed)
        .collect::<Vec<_>>();
    if unfinished.is_empty() {
        return None;
    }
    let preview = unfinished
        .iter()
        .map(|item| format!("[{:?}] {}", item.status, item.step))
        .collect::<Vec<_>>()
        .join("; ");
    Some(format!(
        "todo list has {} unfinished item(s): {preview}; mark them completed via update_todo_list before task_complete",
        unfinished.len()
    ))
}

async fn interrupt_task_agents(
    runtime: &AgentRuntimeHandle,
    thread_id: &str,
    origin: TaskStopOrigin,
) -> Result<()> {
    let root = crate::studio::agent_host::root_agent_id(thread_id);
    let children = runtime
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
                snapshot.lifecycle,
                AgentLifecycleState::Closing | AgentLifecycleState::Closed
            )
        })
        .collect::<Vec<_>>();
    for child in &children {
        if let Some(turn_id) = child.active_turn_id.clone() {
            runtime
                .cancel_turn(child.identity.id.clone(), turn_id)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
    }
    Ok(())
}

async fn wait_for_terminal_outcomes(
    coordinator: &TaskCoordinator,
    task_run_id: &str,
    terminal_facts: &mut tokio::sync::broadcast::Receiver<String>,
) -> Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let active_executor = coordinator
                .store
                .list_work_units(task_run_id)
                .await?
                .into_iter()
                .any(|unit| {
                    matches!(
                        unit.execution_status,
                        ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running
                    )
                });
            let active_reviewer = coordinator
                .store
                .list_review_rounds(task_run_id)
                .await?
                .into_iter()
                .any(|round| {
                    matches!(
                        round.reviewer_status,
                        ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running
                    )
                });
            let active = active_executor || active_reviewer;
            if !active {
                return Ok::<(), anyhow::Error>(());
            }
            match terminal_facts.recv().await {
                Ok(changed_task_run_id) if changed_task_run_id == task_run_id => {}
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    bail!("task terminal fact subscription closed");
                }
            }
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for task agent terminal facts"))??;
    Ok(())
}

async fn close_task_children(runtime: &AgentRuntimeHandle, thread_id: &str) -> Result<()> {
    let root = crate::studio::agent_host::root_agent_id(thread_id);
    let children = runtime
        .list()
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .into_iter()
        .filter(|snapshot| snapshot.identity.parent_id.as_ref() == Some(&root))
        .filter(|snapshot| {
            !matches!(
                snapshot.lifecycle,
                AgentLifecycleState::Closing | AgentLifecycleState::Closed
            )
        })
        .map(|snapshot| snapshot.identity.id)
        .collect::<Vec<_>>();
    for child in children {
        runtime
            .close(child)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }
    Ok(())
}
