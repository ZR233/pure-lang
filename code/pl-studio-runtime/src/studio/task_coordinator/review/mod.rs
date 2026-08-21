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
use serde::{Deserialize, Serialize};

use super::git::{GitDiffSelection, changed_files_between_selected};
use super::{
    BeginIntegratedReview, MergeCandidate, MergeRecord, ReviewRoundRecord, ReviewScope,
    ReviewVerdict, StudioSpawnIntent, TaskCoordinator, TaskExecutorHandoff, TaskRun,
    TaskRunStateKind, TaskWorktreeDisposition, ThreadExecutionStatus, WorkCompletionKind,
    WorkCompletionRecord, WorkCompletionStatus, WorkUnit, WorkUnitStatus,
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
struct RequestIntegratedReviewInput {}

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
struct RequestReviewOutput {
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
    run: TaskRun,
    integrated_review_gate: StudioIntegratedReviewGate,
    work_units: Vec<ModelWorkUnit>,
    completions: Vec<ModelCompletion>,
    merge_candidates: Vec<MergeCandidate>,
    merges: Vec<MergeRecord>,
    /// 概览：省略 findings 明细（由 `read_review_round` 分页全量读取，保证不截断）。
    reviews: Vec<ModelReviewOverview>,
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
    reviewer_status: ThreadExecutionStatus,
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
        let reviewer_status = record.reviewer_status();
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
            reviewer_thread_id: record.reviewer_thread_id,
            reviewer_status,
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
    status: WorkUnitStatus,
    scope_hints: Vec<String>,
    base_commit: String,
    relative_worktree_path: Option<String>,
    branch: String,
    worktree_disposition: TaskWorktreeDisposition,
    attempt: u32,
    executor_thread_id: Option<String>,
    requested_by_call_id: String,
    execution_status: ThreadExecutionStatus,
    execution_summary: Option<String>,
    execution_error: Option<String>,
    budget_limit: Option<pl_protocol::BudgetLimitSnapshot>,
    budget_slice_count: u32,
    budget_slice_limit: u32,
    continuation_state: super::ExecutorContinuationState,
    continuation_source_turn_id: Option<String>,
    continuation_revision: u64,
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
                coordinator
                    .spawn_reviewer(&thread_id, round, &call_id, &runtime)
                    .await
            }
        })
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) fn task_request_integrated_review_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<RequestIntegratedReviewInput>::new(
            "task_request_integrated_review",
            "Start one fresh read-only integrated review for the current Task HEAD.",
        )
        .registered(move |_: RequestIntegratedReviewInput, context| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            let runtime = runtime.clone();
            async move {
                let call_id = provider_call_id(
                    context.provider_call_id.as_deref(),
                    "task_request_integrated_review",
                )?;
                let guard = coordinator.lock_branch_mutation().await;
                let run = coordinator
                    .preflight_integrated_review_locked(&thread_id, &guard)
                    .await?;
                let changed_files = changed_files_between_selected(
                    &run.workspace_root,
                    &run.base_commit,
                    &run.expected_head,
                    GitDiffSelection::ExcludeDesign,
                )
                .await?;
                let round = coordinator
                    .store
                    .begin_integrated_review(
                        &thread_id,
                        BeginIntegratedReview {
                            requested_by_call_id: call_id.clone(),
                            reviewed_head: run.expected_head.clone(),
                            changed_files,
                        },
                    )
                    .await?;
                drop(guard);
                debug_assert_eq!(round.reviewed_head, run.expected_head);
                coordinator
                    .spawn_reviewer(&thread_id, round, &call_id, &runtime)
                    .await
            }
        })
        .with_effect(ToolEffect::BranchControl)
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
                    let run = coordinator
                        .store
                        .read_active_task_run_for_root_thread(&thread_id)
                        .await?;
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
                        .into_iter()
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
                    let output = TaskStatusOutput {
                        work_units: model_work_units,
                        completions: completions
                            .iter()
                            .map(|completion| ModelCompletion::new(&run, completion))
                            .collect(),
                        merge_candidates,
                        merges,
                        reviews,
                        integrated_review_gate,
                        run,
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
    ) -> Result<ToolExecutionResult> {
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
        let mut output = ToolExecutionResult::<serde_json::Value>::json(RequestReviewOutput {
            review_round_id: round.id,
            reviewer_agent_id: handle.snapshot.identity.id.to_string(),
            scope: round.scope,
            reviewed_head: round.reviewed_head,
            completion_id: round.completion_id,
            completion_revision: round.completion_revision,
            round: round.round,
        })
        .map_err(anyhow::Error::from)?;
        // Reviewer completion can change the Planner workspace mutability and tool policy.
        // End this turn so the product-owned continuation prepares a fresh canonical workspace.
        output.ends_turn = true;
        Ok(output)
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
            TaskRunStateKind::Implementing | TaskRunStateKind::Reworking
        ) {
            bail!("integrated review requires implementing or reworking");
        }
        self.ensure_process_lease_owned(&run)?;
        validate_review_repository(&run).await?;
        Ok(run)
    }

    async fn merge_candidates(
        &self,
        run: &TaskRun,
        work_units: &[WorkUnit],
        completions: &[WorkCompletionRecord],
        merges: &[MergeRecord],
        runtime: Option<&AgentRuntimeHandle>,
    ) -> Result<Vec<MergeCandidate>> {
        if run.kind() != TaskRunStateKind::Merging {
            return Ok(Vec::new());
        }
        let mut candidates = Vec::new();
        for work_unit in work_units {
            if work_unit.status() != WorkUnitStatus::Approved
                || work_unit.execution_status() != ThreadExecutionStatus::Completed
            {
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
            if completion.kind != WorkCompletionKind::Delivery
                || completion.status != WorkCompletionStatus::Approved
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
                .head_commit
                .clone()
                .context("approved delivery Completion has no head commit")?;
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
                expected_task_head: run.expected_head.clone(),
            });
        }
        Ok(candidates)
    }
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
            status: work_unit.status(),
            scope_hints: work_unit.scope_hints.clone(),
            base_commit: work_unit.base_commit.clone(),
            relative_worktree_path: model_worktree_locator(
                &run.workspace_root,
                &work_unit.worktree_path,
            ),
            branch: work_unit.branch.clone(),
            worktree_disposition: work_unit.worktree_disposition(),
            attempt: work_unit.attempt,
            executor_thread_id: work_unit.executor_thread_id.clone(),
            requested_by_call_id: work_unit.requested_by_call_id.clone(),
            execution_status: work_unit.execution_status(),
            execution_summary: work_unit.execution_summary().map(str::to_string),
            execution_error: work_unit.execution_error().map(str::to_string),
            budget_limit: work_unit.budget_limit().cloned(),
            budget_slice_count: work_unit.budget_slice_count(),
            budget_slice_limit: super::MAX_EXECUTOR_BUDGET_SLICES,
            continuation_state: work_unit.continuation_state(),
            continuation_source_turn_id: work_unit
                .continuation_source_turn_id()
                .map(str::to_string),
            continuation_revision: work_unit.continuation_revision(),
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
            kind: completion.kind,
            status: completion.status,
            base_commit: completion.base_commit.clone(),
            head_commit: completion.head_commit.clone(),
            changed_files: completion.changed_files.clone(),
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
    let workspace_root = git_compatible_path(PathBuf::from(workspace_root));
    let worktree_path = git_compatible_path(PathBuf::from(worktree_path));
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

pub(super) async fn validate_review_repository(run: &TaskRun) -> Result<()> {
    let snapshot = super::git::inspect_repository(&run.workspace_root, true).await?;
    let common = git_compatible_path(
        std::fs::canonicalize(&snapshot.git_common_dir).unwrap_or(snapshot.git_common_dir),
    )
    .to_string_lossy()
    .replace('\\', "/");
    let expected_common = git_compatible_path(
        std::fs::canonicalize(&run.git_common_dir)
            .unwrap_or_else(|_| std::path::PathBuf::from(&run.git_common_dir)),
    )
    .to_string_lossy()
    .replace('\\', "/");
    let equal_common = if cfg!(windows) {
        common.eq_ignore_ascii_case(&expected_common)
    } else {
        common == expected_common
    };
    if !equal_common || snapshot.branch != run.branch || snapshot.head != run.expected_head {
        bail!("task branch identity or HEAD drifted before review");
    }
    Ok(())
}
