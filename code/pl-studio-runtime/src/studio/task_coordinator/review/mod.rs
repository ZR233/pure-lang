mod coverage;
mod exit;
mod gate;
pub(crate) use gate::integrated_review_gate;
pub(crate) mod prompt;
mod read;
mod trace;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};

use super::transition::TransitionPath;
use super::{
    BeginIntegratedReview, MergeCandidate, MergeRecord, ReviewRoundRecord, ReviewRoundStateKind,
    ReviewScope, ReviewVerdict, StudioSpawnIntent, TaskCoordinator, TaskExecutorHandoff,
    TaskOutcome, TaskRun, TaskRunState, TaskRunStateKind, WorkCompletionKind, WorkCompletionRecord,
    WorkCompletionStatus, WorkUnit, WorkUnitState, WorkUnitStateKind, current_work_units,
};
use crate::agent::worktree::git_compatible_path;
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};
use crate::{
    AgentRoleId, AgentRuntimeHandle, AgentSpawnRequest, StudioIntegratedReviewGate,
    ThreadContextState, ThreadId, ToolEffect,
};

const REVIEWER_CONSTRAINT: &str = include_str!("../../../prompts/review/constraint.md");

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestDeliveryReviewInput {
    /// Executor whose latest completion should be reviewed.
    executor_agent_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskStatusInput {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadWorkUnitHandoffInput {
    work_unit_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RequestReviewOutput {
    review_round_id: String,
    reviewer_agent_id: String,
    scope: ReviewScope,
    reviewed_head: String,
    completion_id: Option<String>,
    completion_revision: Option<u32>,
    round: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskStatusOutput {
    task: ModelTaskStatus,
    execution_activity: ModelExecutionActivity,
    progress: ModelTaskProgress,
    issues: Vec<super::TaskIssueRecord>,
    completion_gate: ModelCompletionGate,
    available_actions: Vec<TransitionPath>,
    latest_completed_task: Option<ModelTaskStatus>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelTaskStatus {
    task_run_id: String,
    state: TaskRunState,
    outcome: Option<TaskOutcome>,
    revision: u64,
    generation: u64,
}

impl ModelTaskStatus {
    fn from_run(run: &TaskRun) -> Self {
        Self {
            task_run_id: run.id.clone(),
            state: run.state.clone(),
            outcome: run.state.outcome().cloned(),
            revision: run.revision,
            generation: run.generation(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ModelExecutionActivity {
    planner_turns: Vec<ModelAgentExecution>,
    executor_turns: Vec<ModelAgentExecution>,
    reviewer_turns: Vec<ModelAgentExecution>,
    queued_turns: Vec<ModelQueuedTurn>,
    last_stop: Option<ModelStopEvent>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelAgentExecution {
    agent_id: String,
    role: String,
    active_turn_id: Option<String>,
    pending_inputs: usize,
    state: serde_json::Value,
    updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelQueuedTurn {
    agent_id: String,
    count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelStopEvent {
    generation: u64,
    origin: String,
    reason: String,
    created_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelTaskProgress {
    work_units: Vec<ModelWorkUnit>,
    completions: Vec<ModelCompletion>,
    merge_candidates: Vec<MergeCandidate>,
    merge_records: Vec<MergeRecord>,
    /// 概览：省略 findings 明细（由 `read_review_round` 分页全量读取，保证不截断）。
    review_rounds: Vec<ModelReviewOverview>,
    pending_interactions: Vec<pl_protocol::InteractionRequest>,
    todo: Option<pl_protocol::TodoListSnapshot>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelCompletionGate {
    available: bool,
    review_gate: StudioIntegratedReviewGate,
    blockers: Vec<String>,
}

/// 单轮审查的概览投影：保留裁决与摘要，省略 findings 明细。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelReviewOverview {
    id: String,
    round: u32,
    scope: ReviewScope,
    work_unit_id: Option<String>,
    completion_id: Option<String>,
    completion_revision: Option<u32>,
    reviewed_head: String,
    verdict: ReviewVerdict,
    reviewer_thread_id: Option<String>,
    reviewer_state: ReviewRoundStateKind,
    reviewer_error: Option<String>,
    summary: Option<String>,
    findings_count: usize,
    has_recommendations: bool,
    coverage_known: bool,
    expected_file_count: usize,
    reviewed_file_count: usize,
    coverage_complete: bool,
    diagnostics_revision: Option<u64>,
    created_at: i64,
    updated_at: i64,
}

impl From<ReviewRoundRecord> for ModelReviewOverview {
    fn from(record: ReviewRoundRecord) -> Self {
        let has_recommendations = record
            .findings
            .iter()
            .any(|finding| !finding.recommendation.trim().is_empty());
        let findings_count = record.findings.len();
        let coverage_known = record.file_reviews.is_some();
        let expected_file_count = record
            .file_reviews
            .as_ref()
            .map_or(0, |coverage| coverage.files.len());
        let reviewed_file_count = record
            .file_reviews
            .as_ref()
            .map_or(0, super::ReviewFileCoverage::reviewed_count);
        let coverage_complete = record
            .file_reviews
            .as_ref()
            .is_some_and(super::ReviewFileCoverage::is_complete);
        let diagnostics_revision = record
            .file_reviews
            .as_ref()
            .map(|coverage| coverage.diagnostics_revision);
        let verdict = record.verdict();
        let reviewer_state = record.kind();
        let reviewer_thread_id = record.reviewer_thread_id().map(str::to_string);
        let reviewer_error = record.reviewer_error().map(str::to_string);
        let summary = record.summary().map(str::to_string);
        Self {
            id: record.id,
            round: record.round,
            scope: record.scope,
            work_unit_id: record.work_unit_id,
            completion_id: record.completion_id,
            completion_revision: record.completion_revision,
            reviewed_head: record.reviewed_head,
            verdict,
            reviewer_thread_id,
            reviewer_state,
            reviewer_error,
            summary,
            findings_count,
            has_recommendations,
            coverage_known,
            expected_file_count,
            reviewed_file_count,
            coverage_complete,
            diagnostics_revision,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ModelWorkUnit {
    id: String,
    task_run_id: String,
    title: String,
    state: WorkUnitState,
    scope_hints: Vec<String>,
    base_commit: String,
    relative_worktree_path: Option<String>,
    branch: String,
    attempt: u32,
    supersedes_work_unit_id: Option<String>,
    executor_thread_id: Option<String>,
    requested_by_call_id: String,
    budget_slice_limit: u32,
    created_at: i64,
    updated_at: i64,
    blueprint_fingerprint: Option<String>,
    objective: Option<String>,
    implementation_step_count: usize,
    acceptance_criterion_count: usize,
    verification_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ModelCompletion {
    id: String,
    task_run_id: String,
    work_unit_id: String,
    executor_agent_id: String,
    revision: u32,
    kind: WorkCompletionKind,
    status: WorkCompletionStatus,
    base_commit: String,
    head_commit: Option<String>,
    changed_files: Vec<String>,
    verification_summary: String,
    relative_worktree_path: Option<String>,
    branch: String,
    created_at: i64,
    updated_at: i64,
}

impl TaskCoordinator {
    pub(crate) fn task_request_delivery_review_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<RequestDeliveryReviewInput>::new(
            "task_request_delivery_review",
            "Start one fresh read-only reviewer for the latest completion of an executor.",
        )
        .registered(move |input: RequestDeliveryReviewInput, context| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            let runtime = runtime.clone();
            async move {
                let call_id = provider_call_id(
                    context.provider_call_id.as_deref(),
                    "task_request_delivery_review",
                )?;
                let round = coordinator
                    .store
                    .begin_delivery_review(&thread_id, input.executor_agent_id.trim(), &call_id)
                    .await?;
                let output = coordinator
                    .spawn_reviewer(&thread_id, round, &call_id, &runtime)
                    .await?;
                ToolExecutionResult::<serde_json::Value>::json(output)
                    .map(ToolExecutionResult::ending_turn)
                    .map_err(anyhow::Error::from)
            }
        })
        .with_effect(ToolEffect::BranchControl)
    }

    pub(super) async fn begin_integrated_review_transition(
        self: &Arc<Self>,
        thread_id: &str,
        call_id: &str,
        expected_revision: u64,
        expected_generation: u64,
        runtime: &AgentRuntimeHandle,
    ) -> Result<RequestReviewOutput> {
        let guard = self.lock_branch_mutation().await;
        let run = self
            .preflight_integrated_review_locked(thread_id, &guard)
            .await?;
        let (reviewed_head, changed_files) = if let Some(target) = run.state.review_target() {
            (target.reviewed_head.clone(), target.changed_files.clone())
        } else {
            let merges = self.store.list_merge_records(&run.id).await?;
            let completions = self.store.list_work_completions(&run.id).await?;
            let reviewed_head = merges
                .iter()
                .max_by_key(|merge| (merge.created_at, &merge.id))
                .map(|merge| merge.resulting_head.clone())
                .context("integrated review requires durable merge evidence")?;
            let merged_completion_ids = merges
                .iter()
                .map(|merge| merge.completion_id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let mut changed_files = completions
                .iter()
                .filter(|completion| merged_completion_ids.contains(completion.id.as_str()))
                .flat_map(|completion| completion.changed_files().iter().cloned())
                .filter(|path| !path.starts_with("design/"))
                .collect::<Vec<_>>();
            changed_files.sort();
            changed_files.dedup();
            (reviewed_head, changed_files)
        };
        let round = self
            .store
            .begin_integrated_review(
                thread_id,
                BeginIntegratedReview {
                    requested_by_call_id: call_id.to_string(),
                    expected_revision,
                    expected_generation,
                    reviewed_head: reviewed_head.clone(),
                    changed_files,
                },
            )
            .await?;
        drop(guard);
        debug_assert_eq!(round.reviewed_head, reviewed_head);
        self.spawn_reviewer(thread_id, round, call_id, runtime)
            .await
    }

    pub(crate) fn task_status_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: Option<AgentRuntimeHandle>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<TaskStatusInput>::new(
            "task_status",
            "Read the canonical durable Task state, merge candidates, completions, reviews, merges, and findings.",
        )
        .registered(move |_: TaskStatusInput, _| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                let runtime = runtime.clone();
                async move {
                    let run = match coordinator
                        .store
                        .find_active_task_run_for_root_thread(&thread_id)
                        .await?
                    {
                        Some(run) => run,
                        None => coordinator
                            .store
                            .find_latest_task_run_for_root_thread(&thread_id)
                            .await?
                            .context("TaskRun not found for this root Thread")?,
                    };
                    let work_units = coordinator.store.list_work_units(&run.id).await?;
                    let completions = coordinator.store.list_work_completions(&run.id).await?;
                    let merges = coordinator.store.list_merge_records(&run.id).await?;
                    let merge_candidates = coordinator
                        .merge_candidates(
                            &run,
                            &work_units,
                            &completions,
                            &merges,
                            runtime.as_ref(),
                        )
                        .await?;
                    let review_records = coordinator.store.list_review_rounds(&run.id).await?;
                    let integrated_review_gate = integrated_review_gate(
                        &run,
                        &work_units,
                        &completions,
                        &merges,
                        &review_records,
                    )
                    .await;
                    let reviews = review_records
                        .iter()
                        .cloned()
                        .map(ModelReviewOverview::from)
                        .collect::<Vec<_>>();
                    let mut model_work_units = Vec::with_capacity(work_units.len());
                    for work_unit in &work_units {
                        let handoff = coordinator
                            .store
                            .read_work_unit_handoff(&work_unit.id)
                            .await
                            .ok()
                            .flatten()
                            .map(|(_, handoff)| handoff);
                        model_work_units.push(ModelWorkUnit::new(
                            &run,
                            work_unit,
                            handoff.as_ref(),
                        ));
                    }
                    let pending_interactions = coordinator
                        .store
                        .list_pending_interactions(&run.root_thread_id)
                        .await?;
                    let todo = coordinator.store.read_thread_todo(&run.root_thread_id).await?;
                    let issues = coordinator.store.list_task_issues(&run.id).await?;
                    let execution_activity = coordinator
                        .model_execution_activity(&run, runtime.as_ref())
                        .await?;
                    let completion_blockers = completion_blockers(
                        &run,
                        &work_units,
                        &review_records,
                        &pending_interactions,
                        todo.as_ref(),
                        &integrated_review_gate,
                        &execution_activity,
                    );
                    let completion_gate = ModelCompletionGate {
                        available: completion_blockers.is_empty(),
                        review_gate: integrated_review_gate.clone(),
                        blockers: completion_blockers.clone(),
                    };
                    let available_actions = coordinator
                        .transition_paths(&run, runtime.as_ref())
                        .await?;
                    let latest_completed_task = coordinator
                        .store
                        .list_task_runs_for_project(&run.project_id)
                        .await?
                        .into_iter()
                        .filter(|candidate| {
                            candidate.id != run.id && candidate.kind().is_terminal()
                        })
                        .max_by_key(|candidate| (candidate.updated_at, candidate.id.clone()))
                        .as_ref()
                        .map(ModelTaskStatus::from_run);
                    let output = TaskStatusOutput {
                        task: ModelTaskStatus::from_run(&run),
                        execution_activity,
                        progress: ModelTaskProgress {
                        work_units: model_work_units,
                        completions: completions
                            .iter()
                            .map(|completion| ModelCompletion::new(&run, completion))
                            .collect(),
                        merge_candidates,
                            merge_records: merges,
                            review_rounds: reviews,
                            pending_interactions,
                            todo,
                        },
                        issues,
                        completion_gate,
                        available_actions,
                        latest_completed_task,
                    };
                    // task_status 是只读概览；放宽预算保证中小任务一次读全，超大任务
                    // 的 findings 明细由 read_review_round 分页补充。
                    ToolExecutionResult::<serde_json::Value>::json_with_budget(
                        output,
                        /* max_output_tokens */ 12_000,
                        /* max_output_bytes */ 48 * 1024,
                    )
                    .map_err(anyhow::Error::from)
                }
            })
        .with_effect(ToolEffect::Read)
    }

    pub(crate) fn read_work_unit_handoff_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<ReadWorkUnitHandoffInput>::new(
            "read_work_unit_handoff",
            "Read the complete durable implementation blueprint for one Task work unit.",
        )
        .registered(move |input: ReadWorkUnitHandoffInput, _| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            async move {
                let work_unit_id = input.work_unit_id.trim();
                if work_unit_id.is_empty() {
                    bail!("workUnitId must not be empty")
                }
                let run = coordinator
                    .store
                    .read_active_task_run_for_root_thread(&thread_id)
                    .await?;
                let (work_unit, handoff) = coordinator
                    .store
                    .read_work_unit_handoff(work_unit_id)
                    .await?
                    .context("Task work unit handoff not found")?;
                if work_unit.task_run_id != run.id {
                    bail!("workUnitId does not belong to the active Task")
                }
                ToolExecutionResult::<serde_json::Value>::json_with_budget(
                    handoff,
                    8_000,
                    32 * 1024,
                )
                .map_err(anyhow::Error::from)
            }
        })
        .with_effect(ToolEffect::Read)
    }

    async fn spawn_reviewer(
        self: &Arc<Self>,
        thread_id: &str,
        round: ReviewRoundRecord,
        call_id: &str,
        runtime: &AgentRuntimeHandle,
    ) -> Result<RequestReviewOutput> {
        let prompt = match prompt::build_review_prompt(self, &round).await {
            Ok(prompt) => prompt,
            Err(error) => {
                self.store
                    .fail_reviewer_spawn(thread_id, None, call_id, &error.to_string())
                    .await?;
                return Err(error);
            }
        };
        let reviewer_thread_id = ThreadId::generate();
        let intent = StudioSpawnIntent::task_reviewer(
            thread_id,
            format!("{}_review_round_{}", round.scope.as_str(), round.round),
            call_id,
            round.id.clone(),
            REVIEWER_CONSTRAINT,
        );
        let spawn = runtime
            .spawn(AgentSpawnRequest {
                thread_id: reviewer_thread_id,
                parent_id: crate::studio::agent_host::root_agent_id(thread_id),
                role: AgentRoleId::new("reviewer")
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?,
                session: ThreadContextState::empty(),
                initial_turn_id: None,
                initial_message: Some(prompt),
                metadata: serde_json::to_value(intent)?,
            })
            .await;
        let handle = match spawn {
            Ok(handle) => handle,
            Err(error) => {
                self.store
                    .fail_reviewer_spawn(thread_id, None, call_id, &error.to_string())
                    .await?;
                return Err(anyhow::anyhow!(error.to_string()));
            }
        };
        Ok(RequestReviewOutput {
            review_round_id: round.id,
            reviewer_agent_id: handle.snapshot.identity.id.to_string(),
            scope: round.scope,
            reviewed_head: round.reviewed_head,
            completion_id: round.completion_id,
            completion_revision: round.completion_revision,
            round: round.round,
        })
    }

    pub(super) async fn preflight_integrated_review_locked(
        &self,
        thread_id: &str,
        guard: &super::BranchMutationGuard<'_>,
    ) -> Result<TaskRun> {
        self.ensure_branch_mutation_guard(guard)?;
        let run = self
            .store
            .read_active_task_run_for_root_thread(thread_id)
            .await?;
        if !matches!(
            run.kind(),
            TaskRunStateKind::Working | TaskRunStateKind::Reviewing
        ) {
            bail!("integrated review requires working or reviewing state");
        }
        Ok(run)
    }

    pub(super) async fn model_execution_activity(
        &self,
        run: &TaskRun,
        runtime: Option<&AgentRuntimeHandle>,
    ) -> Result<ModelExecutionActivity> {
        let root = crate::studio::agent_host::root_agent_id(&run.root_thread_id);
        let snapshots = match runtime {
            Some(runtime) => runtime.list().await.map_err(anyhow::Error::msg)?,
            None => Vec::new(),
        };
        let mut planner_turns = Vec::new();
        let mut executor_turns = Vec::new();
        let mut reviewer_turns = Vec::new();
        let mut queued_turns = Vec::new();
        for snapshot in snapshots.into_iter().filter(|snapshot| {
            snapshot.identity.id == root || snapshot.identity.parent_id.as_ref() == Some(&root)
        }) {
            let role = if snapshot.identity.id == root {
                "planner".to_string()
            } else {
                snapshot.identity.role.as_str().to_string()
            };
            if snapshot.pending_inputs > 0 {
                queued_turns.push(ModelQueuedTurn {
                    agent_id: snapshot.identity.id.to_string(),
                    count: snapshot.pending_inputs,
                });
            }
            let execution = ModelAgentExecution {
                agent_id: snapshot.identity.id.to_string(),
                role: role.clone(),
                active_turn_id: snapshot.active_turn_id().map(ToString::to_string),
                pending_inputs: snapshot.pending_inputs,
                state: serde_json::to_value(&snapshot.state)?,
                updated_at: snapshot.updated_at,
            };
            match role.as_str() {
                "planner" => planner_turns.push(execution),
                "executor" => executor_turns.push(execution),
                "reviewer" => reviewer_turns.push(execution),
                _ => {}
            }
        }
        let last_stop = crate::studio::entity::task_stop_event::Entity::find()
            .filter(crate::studio::entity::task_stop_event::Column::TaskRunId.eq(run.id.clone()))
            .order_by_desc(crate::studio::entity::task_stop_event::Column::Generation)
            .order_by_desc(crate::studio::entity::task_stop_event::Column::CreatedAt)
            .one(self.store.database())
            .await?
            .map(|event| -> Result<ModelStopEvent> {
                Ok(ModelStopEvent {
                    generation: u64::try_from(event.generation)
                        .context("task stop generation is negative")?,
                    origin: event.origin,
                    reason: event.reason,
                    created_at: event.created_at,
                })
            })
            .transpose()?;
        Ok(ModelExecutionActivity {
            planner_turns,
            executor_turns,
            reviewer_turns,
            queued_turns,
            last_stop,
        })
    }

    async fn merge_candidates(
        &self,
        run: &TaskRun,
        work_units: &[WorkUnit],
        completions: &[WorkCompletionRecord],
        merges: &[MergeRecord],
        runtime: Option<&AgentRuntimeHandle>,
    ) -> Result<Vec<MergeCandidate>> {
        if run.kind() != TaskRunStateKind::Working {
            return Ok(Vec::new());
        }
        let mut candidates = Vec::new();
        for work_unit in work_units {
            if work_unit.kind() != WorkUnitStateKind::ReviewPassed {
                continue;
            }
            let Some(executor_agent_id) = work_unit.executor_thread_id.as_deref() else {
                continue;
            };
            if let Some(runtime) = runtime {
                self.await_closed_agent_projection(runtime, executor_agent_id)
                    .await?;
            }
            let executor = self
                .store
                .read_thread(executor_agent_id)
                .await?
                .context("approved executor canonical Thread not found")?;
            if executor.role != "executor" || executor.status != pl_protocol::ThreadStatus::Closed {
                continue;
            }
            let Some(completion) = completions
                .iter()
                .filter(|completion| completion.work_unit_id == work_unit.id)
                .max_by_key(|completion| completion.revision)
            else {
                continue;
            };
            if completion.kind() != WorkCompletionKind::Delivery
                || completion.status() != WorkCompletionStatus::Approved
            {
                continue;
            }
            let already_recorded = merges.iter().any(|merge| {
                merge.executor_agent_id == executor_agent_id
                    || (merge.completion_id == completion.id
                        && merge.completion_revision == completion.revision)
            });
            if already_recorded {
                continue;
            }
            let head_commit = completion
                .head_commit()
                .map(str::to_string)
                .context("approved delivery Completion has no head commit")?;
            let expected_task_head = merges
                .iter()
                .max_by_key(|merge| (merge.created_at, &merge.id))
                .map_or_else(
                    || completion.base_commit.clone(),
                    |merge| merge.resulting_head.clone(),
                );
            candidates.push(MergeCandidate {
                executor_agent_id: executor_agent_id.to_string(),
                completion_revision: completion.revision,
                relative_worktree_path: relative_worktree_locator(
                    &run.workspace_root,
                    &work_unit.worktree_path,
                )?,
                branch: work_unit.branch.clone(),
                base_commit: completion.base_commit.clone(),
                head_commit,
                expected_task_head,
            });
        }
        Ok(candidates)
    }
}

pub(super) fn completion_blockers(
    run: &TaskRun,
    work_units: &[WorkUnit],
    reviews: &[ReviewRoundRecord],
    pending_interactions: &[pl_protocol::InteractionRequest],
    todo: Option<&pl_protocol::TodoListSnapshot>,
    review_gate: &StudioIntegratedReviewGate,
    execution: &ModelExecutionActivity,
) -> Vec<String> {
    let mut blockers = Vec::new();
    if !matches!(
        run.kind(),
        TaskRunStateKind::Working | TaskRunStateKind::Reviewing
    ) {
        blockers.push("成功完成要求任务处于 working 或 reviewing".to_string());
    }
    for unit in current_work_units(work_units) {
        if unit.kind() != WorkUnitStateKind::Completed {
            blockers.push(format!(
                "当前有效工作单 {} 尚未结算，状态为 {}",
                unit.id,
                unit.kind().as_str()
            ));
        }
    }
    for review in reviews.iter().filter(|review| review.kind().is_active()) {
        blockers.push(format!("审查轮 {} 尚未结束", review.id));
    }
    for interaction in pending_interactions {
        blockers.push(format!("用户交互 {} 尚未处理", interaction.interaction_id));
    }
    if let Some(todo) = todo {
        for item in todo
            .items
            .iter()
            .filter(|item| item.status != pl_protocol::TodoStatus::Completed)
        {
            blockers.push(format!("待办尚未完成：{}", item.step));
        }
    }
    for activity in execution
        .executor_turns
        .iter()
        .chain(&execution.reviewer_turns)
        .filter(|activity| activity.active_turn_id.is_some() || activity.pending_inputs > 0)
    {
        blockers.push(format!(
            "{} {} 仍有模型执行活动",
            activity.role, activity.agent_id
        ));
    }
    if let StudioIntegratedReviewGate::Required { reason } = review_gate {
        blockers.push(format!("综合审查门槛尚未满足：{reason}"));
    }
    blockers
}

pub(super) fn integrated_review_blockers(
    run: &TaskRun,
    work_units: &[WorkUnit],
    reviews: &[ReviewRoundRecord],
    merges: &[MergeRecord],
) -> Vec<String> {
    let mut blockers = Vec::new();
    if !matches!(
        run.kind(),
        TaskRunStateKind::Working | TaskRunStateKind::Reviewing
    ) {
        blockers.push("综合审查要求任务处于 working 或 reviewing".to_string());
    }
    for unit in current_work_units(work_units) {
        if unit.kind() != WorkUnitStateKind::Completed {
            blockers.push(format!("当前有效工作单 {} 尚未结算", unit.id));
        }
    }
    for review in reviews.iter().filter(|review| review.kind().is_active()) {
        blockers.push(format!("审查轮 {} 尚未结束", review.id));
    }
    if run.kind() == TaskRunStateKind::Working && merges.is_empty() {
        blockers.push("没有可冻结的合并记录；无交付任务应直接完成".to_string());
    }
    blockers
}

impl ModelWorkUnit {
    pub(super) fn new(
        run: &TaskRun,
        work_unit: &WorkUnit,
        handoff: Option<&TaskExecutorHandoff>,
    ) -> Self {
        Self {
            id: work_unit.id.clone(),
            task_run_id: work_unit.task_run_id.clone(),
            title: work_unit.title.clone(),
            state: work_unit.state.clone(),
            scope_hints: work_unit.scope_hints.clone(),
            base_commit: work_unit.base_commit.clone(),
            relative_worktree_path: model_worktree_locator(
                &run.workspace_root,
                &work_unit.worktree_path,
            ),
            branch: work_unit.branch.clone(),
            attempt: work_unit.attempt,
            supersedes_work_unit_id: work_unit.supersedes_work_unit_id.clone(),
            executor_thread_id: work_unit.executor_thread_id.clone(),
            requested_by_call_id: work_unit.requested_by_call_id.clone(),
            budget_slice_limit: super::MAX_EXECUTOR_BUDGET_SLICES,
            created_at: work_unit.created_at,
            updated_at: work_unit.updated_at,
            blueprint_fingerprint: handoff.map(|handoff| handoff.blueprint_fingerprint.clone()),
            objective: handoff.map(|handoff| handoff.blueprint.objective.clone()),
            implementation_step_count: handoff
                .map_or(0, |handoff| handoff.blueprint.implementation_steps.len()),
            acceptance_criterion_count: handoff
                .map_or(0, |handoff| handoff.blueprint.acceptance_criteria.len()),
            verification_count: handoff.map_or(0, |handoff| handoff.blueprint.verification_count()),
        }
    }
}

impl ModelCompletion {
    pub(super) fn new(run: &TaskRun, completion: &WorkCompletionRecord) -> Self {
        Self {
            id: completion.id.clone(),
            task_run_id: completion.task_run_id.clone(),
            work_unit_id: completion.work_unit_id.clone(),
            executor_agent_id: completion.executor_agent_id.clone(),
            revision: completion.revision,
            kind: completion.kind(),
            status: completion.status(),
            base_commit: completion.base_commit.clone(),
            head_commit: completion.head_commit().map(str::to_string),
            changed_files: completion.changed_files().to_vec(),
            verification_summary: completion.verification_summary.clone(),
            relative_worktree_path: model_worktree_locator(
                &run.workspace_root,
                &completion.worktree_path,
            ),
            branch: completion.branch.clone(),
            created_at: completion.created_at,
            updated_at: completion.updated_at,
        }
    }
}

fn relative_worktree_locator(workspace_root: &str, worktree_path: &str) -> Result<String> {
    let workspace_root = git_compatible_path(
        std::fs::canonicalize(workspace_root)
            .context("failed to resolve Task workspace for merge candidate")?,
    );
    let worktree_path = git_compatible_path(
        std::fs::canonicalize(worktree_path)
            .context("failed to resolve executor worktree for merge candidate")?,
    );
    let relative = worktree_path
        .strip_prefix(&workspace_root)
        .context("executor worktree is outside the Task workspace")?;
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if components.len() != 4
        || components[0] != ".pure"
        || components[1] != "worktrees"
        || components.last().is_some_and(String::is_empty)
    {
        bail!("executor worktree is not a canonical .pure/worktrees leaf");
    }
    Ok(components.join("/"))
}

fn model_worktree_locator(workspace_root: &str, worktree_path: &str) -> Option<String> {
    let workspace_root = git_compatible_path(
        std::fs::canonicalize(workspace_root).unwrap_or_else(|_| PathBuf::from(workspace_root)),
    );
    let worktree_path = git_compatible_path(
        std::fs::canonicalize(worktree_path).unwrap_or_else(|_| PathBuf::from(worktree_path)),
    );
    let relative = worktree_path.strip_prefix(workspace_root).ok()?;
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    (components.len() == 4
        && components[0] == ".pure"
        && components[1] == "worktrees"
        && components[2..]
            .iter()
            .all(|component| !component.is_empty() && component != "." && component != ".."))
    .then(|| components.join("/"))
}

fn provider_call_id(value: Option<&str>, tool: &str) -> Result<String> {
    let value = value
        .context(format!("{tool} requires a provider call id"))?
        .trim();
    if value.is_empty() {
        bail!("{tool} requires a non-empty provider call id");
    }
    Ok(value.to_string())
}
