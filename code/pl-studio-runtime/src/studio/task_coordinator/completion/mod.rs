use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::git::changed_files_between;
use super::merge::{ProductionMergeVerifier, select_merge_verification_commands};
use super::{
    ReviewScope, ReviewVerdict, TaskCoordinator, TaskRunPhase, TaskRunRecord, TaskStopOrigin,
    TaskStopReason, ThreadExecutionStatus, WorkUnitStatus,
};
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::{AgentLifecycleState, AgentRuntimeHandle, ToolEffect};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteTaskInput {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StopTaskInput {
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskCompletionOutput {
    run: TaskRunRecord,
    verification: Vec<super::MergeVerificationStep>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
enum TaskCompleteOutcome {
    Completed(Box<TaskCompletionOutput>),
    Rejected {
        code: &'static str,
        recoverable: bool,
        message: String,
        verification: Vec<super::MergeVerificationStep>,
    },
}

impl TaskCompleteOutcome {
    fn rejected(code: &'static str, message: impl Into<String>) -> Self {
        Self::Rejected {
            code,
            recoverable: true,
            message: message.into(),
            verification: Vec::new(),
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
        RegisteredTool::from_typed_fallible_execution_result(
            "task_complete",
            "Complete a fully merged, design-consistent and reviewer-approved task.",
            strict_tool_input_schema([]),
            move |_: CompleteTaskInput, _context| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                async move {
                    let outcome = coordinator.complete_task(&thread_id).await?;
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
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) fn task_stop_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "task_stop",
            "Stop the current task after safely settling agents and branch state.",
            strict_tool_input_schema([ToolInputSchemaField::required(
                "reason",
                serde_json::json!({"type":"string"}),
            )]),
            move |input: StopTaskInput, _context| {
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
            },
        )
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

    async fn complete_task(&self, thread_id: &str) -> Result<TaskCompleteOutcome> {
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
        let changed_files = changed_files_between(
            Path::new(&run.workspace_root),
            &run.base_commit,
            &run.expected_head,
        )
        .await?;
        let commands =
            select_merge_verification_commands(Path::new(&run.workspace_root), &changed_files);
        let verification = ProductionMergeVerifier::verify_commands(commands).await;
        if verification.iter().any(|step| !step.success) {
            return Ok(TaskCompleteOutcome::Rejected {
                code: "verificationFailed",
                recoverable: true,
                message: "task completion verification failed; inspect verification steps"
                    .to_string(),
                verification,
            });
        }
        let summary = if verification.is_empty() {
            "task completed; no additional final checks were required".to_string()
        } else {
            format!(
                "task completed after {} final verification checks",
                verification.len()
            )
        };
        let completed = self
            .store
            .complete_reviewed_task(thread_id, &run.expected_head, &summary)
            .await;
        let completed = match completed {
            Ok(completed) => completed,
            Err(error) => {
                return Ok(TaskCompleteOutcome::rejected(
                    "repositoryDrift",
                    format!("task completion state changed before commit: {error}"),
                ));
            }
        };
        self.release_owned_process_lease(&run.id);
        Ok(TaskCompleteOutcome::Completed(Box::new(
            TaskCompletionOutput {
                run: completed,
                verification,
            },
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
