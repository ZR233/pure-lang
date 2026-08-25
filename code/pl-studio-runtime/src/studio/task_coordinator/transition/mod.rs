//! 六态 TaskRun 的唯一计划者状态动作入口。

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    TaskCommand, TaskCoordinator, TaskFailureKind, TaskOutcome, TaskReviewGate, TaskRun,
    TaskRunStateKind,
};
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};
use crate::{AgentRuntimeHandle, StudioIntegratedReviewGate, ToolEffect};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TransitionAction {
    SubmitPlan,
    FinishDocumentEditing,
    BeginIntegratedReview,
    CancelIntegratedReview,
    Complete,
    ResolveIssue,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) enum CompletionOutcomeInput {
    Succeeded,
    Failed,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskTransitionInput {
    action: TransitionAction,
    expected_revision: u64,
    expected_generation: u64,
    summary: Option<String>,
    review_round_id: Option<String>,
    reason: Option<String>,
    outcome: Option<CompletionOutcomeInput>,
    evidence: Option<String>,
    cause: Option<String>,
    issue_id: Option<String>,
    resolution_evidence: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
enum TaskTransitionOutput {
    Accepted {
        action: TransitionAction,
        previous_state: TaskRunStateKind,
        current_state: TaskRunStateKind,
        task_run_id: String,
        revision: u64,
        generation: u64,
        durable_effects: Vec<serde_json::Value>,
        external_effects: Vec<serde_json::Value>,
        available_actions: Vec<TransitionPath>,
    },
    Rejected {
        code: &'static str,
        message: String,
        current_state: TaskRunStateKind,
        requested_action: TransitionAction,
        requested_state: &'static str,
        task_run_id: String,
        revision: u64,
        generation: u64,
        reasons: Vec<TransitionBlocker>,
        available_paths: Vec<TransitionPath>,
    },
    Failed {
        code: &'static str,
        phase: &'static str,
        recoverable: bool,
        message: String,
        task_run_id: String,
        work_unit_id: Option<String>,
        review_round_id: Option<String>,
        agent_id: Option<String>,
        resource: Option<&'static str>,
        cause: String,
        compensation: serde_json::Value,
        current_state: TaskRunStateKind,
        revision: u64,
        generation: u64,
        available_paths: Vec<TransitionPath>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransitionBlocker {
    code: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TransitionPath {
    action: TransitionAction,
    target_state: &'static str,
    outcome: Option<CompletionOutcomeInput>,
    immediately_available: bool,
    required_inputs: Vec<&'static str>,
    satisfied_facts: Vec<String>,
    missing_facts: Vec<String>,
}

impl TaskCoordinator {
    pub(crate) fn task_transition_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<TaskTransitionInput>::new(
            "task_transition",
            "Submit one canonical Task state action with an expected record revision and execution generation.",
        )
        .registered(move |input: TaskTransitionInput, context| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            let runtime = runtime.clone();
            async move {
                let provider_call_id = context
                    .provider_call_id
                    .as_deref()
                    .context("task_transition requires a provider call id")?;
                let current = coordinator
                    .task_runtime
                    .aggregate(&thread_id)
                    .await
                    .context("active TaskRun is not resident in TaskRuntime")?
                    .facts
                    .run;
                let action = input.action;
                let requested_outcome = input.outcome;
                let requested_summary = input.summary.clone();
                let previous_state = current.kind();
                let input_blockers = input.validation_blockers();
                if !input_blockers.is_empty() {
                    let paths = coordinator
                        .transition_paths(&current, Some(&runtime))
                        .await
                        .unwrap_or_else(|_| state_only_transition_paths(current.kind()));
                    return transition_result(TaskTransitionOutput::rejected(
                        &current,
                        action,
                        "invalidInput",
                        "状态动作缺少必要输入",
                        input_blockers,
                        paths,
                    ));
                }
                let result = coordinator
                    .apply_transition(&thread_id, provider_call_id, input, &runtime)
                    .await;
                match result {
                    Ok((updated, durable_effects, external_effects, ends_turn)) => {
                        let available_actions = match coordinator
                            .transition_paths(&updated, Some(&runtime))
                            .await
                        {
                            Ok(paths) => paths,
                            Err(error) => {
                                return transition_result(
                                    TaskTransitionOutput::failed_during_result_assembly(
                                        &updated,
                                        error.to_string(),
                                    ),
                                );
                            }
                        };
                        let output = TaskTransitionOutput::Accepted {
                            action,
                            previous_state,
                            current_state: updated.kind(),
                            task_run_id: updated.id.clone(),
                            revision: updated.revision,
                            generation: updated.generation(),
                            durable_effects,
                            external_effects,
                            available_actions,
                        };
                        finalize_transition_result(
                            transition_result(output)?,
                            action,
                            ends_turn,
                            requested_summary,
                        )
                    }
                    Err(error) => {
                        let latest = match coordinator.task_runtime.aggregate(&thread_id).await {
                            Some(latest) => latest.facts.run,
                            None => {
                                return transition_result(TaskTransitionOutput::failed(
                                    &current,
                                    action,
                                    format!(
                                        "{error}; transition audit could not find TaskRun {}",
                                        current.id
                                    ),
                                    state_only_transition_paths(current.kind()),
                                    false,
                                ));
                            }
                        };
                        let paths = coordinator
                            .transition_paths(&latest, Some(&runtime))
                            .await
                            .unwrap_or_else(|_| state_only_transition_paths(latest.kind()));
                        if task_record_changed(&current, &latest) {
                            return transition_result(TaskTransitionOutput::failed(
                                &latest,
                                action,
                                error.to_string(),
                                paths,
                                true,
                            ));
                        }
                        let mut reasons =
                            blockers_for_rejected_action(action, requested_outcome, &paths);
                        push_unique_blocker(
                            &mut reasons,
                            TransitionBlocker {
                                code: "transitionRejected",
                                message: error.to_string(),
                            },
                        );
                        transition_result(TaskTransitionOutput::rejected(
                            &latest,
                            action,
                            "transitionRejected",
                            error.to_string(),
                            reasons,
                            paths,
                        ))
                    }
                }
            }
        })
        .with_effect(ToolEffect::BranchControl)
    }

    async fn apply_transition(
        self: &Arc<Self>,
        thread_id: &str,
        requested_by_call_id: &str,
        input: TaskTransitionInput,
        runtime: &AgentRuntimeHandle,
    ) -> Result<(
        TaskRun,
        Vec<serde_json::Value>,
        Vec<serde_json::Value>,
        bool,
    )> {
        let version = (input.expected_revision, input.expected_generation);
        match input.action {
            TransitionAction::SubmitPlan => {
                let plan = required(
                    input.summary.as_deref(),
                    "submitPlan requires summary as the complete plan",
                )?;
                let run = self
                    .task_runtime
                    .submit_plan(thread_id, plan, version.0, version.1)
                    .await?;
                let plan_revision = run
                    .plan
                    .as_ref()
                    .context("submitted Task plan disappeared")?
                    .revision;
                let interaction_id = format!("plan-confirmation-{}-{requested_by_call_id}", run.id);
                let now = crate::studio::unix_seconds();
                let interaction = crate::InteractionRequest::plan_confirmation(
                    interaction_id.clone(),
                    crate::InteractionScope {
                        thread_id: thread_id.to_string(),
                        turn_id: requested_by_call_id.to_string(),
                        item_id: Some(interaction_id.clone()),
                        tool_id: Some(requested_by_call_id.to_string()),
                        agent_path: Some(thread_id.to_string()),
                    },
                    format!("{}:{plan_revision}", run.id),
                    plan,
                    now,
                );
                runtime
                    .record_thread_facts(
                        crate::studio::agent_host::root_agent_id(thread_id),
                        pl_core::ThreadId::new(thread_id.to_string())?,
                        vec![pl_core::ThreadNotificationFact::durable(
                            interaction.updated_at,
                            pl_protocol::ThreadNotification::InteractionChanged {
                                interaction: Box::new(interaction.clone()),
                            },
                        )],
                    )
                    .await
                    .map_err(anyhow::Error::msg)?;
                Ok((
                    run,
                    vec![
                        serde_json::json!({"planRevision": plan_revision, "interactionId": interaction.interaction_id}),
                    ],
                    vec![serde_json::json!({"kind": "waitForPlanConfirmation"})],
                    true,
                ))
            }
            TransitionAction::FinishDocumentEditing => {
                let summary = required(
                    input.summary.as_deref(),
                    "finishDocumentEditing requires summary",
                )?;
                let run = self
                    .task_runtime
                    .apply_run_command(
                        thread_id,
                        version.0,
                        version.1,
                        TaskCommand::FinishDocumentEditing {
                            summary: summary.to_string(),
                        },
                    )
                    .await?;
                let mail_id = format!("task-working:{}:{}", run.id, run.revision);
                runtime
                    .submit(
                        crate::studio::agent_host::root_agent_id(thread_id),
                        pl_core::AgentSubmitRequest::start(
                            pl_core::ThreadId::new(thread_id.to_string())?,
                            "文档编辑已经提交，任务进入 working。先调用 task_status 核对持久化事实，再拆分并派发工作单。",
                        )
                        .with_presentation(pl_core::MailboxPresentation::Hidden)
                        .with_mail_id(mail_id)
                        .with_turn_policy(pl_core::AgentTurnSubmitPolicy::StartOrQueue),
                    )
                    .await
                    .map_err(anyhow::Error::msg)?;
                Ok((
                    run,
                    vec![serde_json::json!({"documentEditSummary": summary})],
                    vec![serde_json::json!({"kind": "plannerContinuationQueued"})],
                    true,
                ))
            }
            TransitionAction::BeginIntegratedReview => {
                let review = self
                    .begin_integrated_review_transition(
                        thread_id,
                        requested_by_call_id,
                        version.0,
                        version.1,
                        runtime,
                    )
                    .await?;
                let run = self
                    .task_runtime
                    .aggregate(thread_id)
                    .await
                    .context("integrated-review Task aggregate is not resident")?
                    .facts
                    .run;
                Ok((
                    run,
                    vec![serde_json::to_value(&review)?],
                    vec![serde_json::json!({"kind": "reviewerStarted"})],
                    true,
                ))
            }
            TransitionAction::CancelIntegratedReview => {
                let review_round_id = required(
                    input.review_round_id.as_deref(),
                    "cancelIntegratedReview requires reviewRoundId",
                )?;
                let reason = required(
                    input.reason.as_deref(),
                    "cancelIntegratedReview requires reason",
                )?;
                let run = self
                    .task_runtime
                    .cancel_integrated_review(
                        thread_id,
                        review_round_id,
                        reason,
                        version.0,
                        version.1,
                    )
                    .await?;
                Ok((
                    run,
                    vec![serde_json::json!({"reviewRoundId": review_round_id, "reason": reason})],
                    Vec::new(),
                    false,
                ))
            }
            TransitionAction::Complete => {
                let summary = required(input.summary.as_deref(), "complete requires summary")?;
                let outcome = input.outcome.context("complete requires outcome")?;
                let run = match outcome {
                    CompletionOutcomeInput::Succeeded => {
                        let aggregate = self
                            .task_runtime
                            .aggregate(thread_id)
                            .await
                            .context("active Task aggregate is not resident")?;
                        let current = aggregate.facts.run;
                        let work_units = aggregate.facts.work_units;
                        let completions = aggregate.facts.completions;
                        let merges = aggregate.facts.merges;
                        let reviews = aggregate.facts.reviews;
                        let pending_interactions =
                            self.store.list_pending_interactions(thread_id).await?;
                        let todo = self.store.read_thread_todo(thread_id).await?;
                        let execution = self
                            .model_execution_activity(&current, Some(runtime))
                            .await?;
                        let readiness = super::review::completion_readiness(
                            super::review::CompletionReadinessInput {
                                run: &current,
                                work_units: &work_units,
                                completions: &completions,
                                reviews: &reviews,
                                merges: &merges,
                                pending_interactions: &pending_interactions,
                                todo: todo.as_ref(),
                                execution: &execution,
                            },
                        );
                        if !readiness.is_available() {
                            bail!(
                                "success completion gate is not satisfied: {}",
                                readiness.blockers().join("; ")
                            );
                        }
                        self.task_runtime
                            .complete_task(
                                thread_id,
                                version.0,
                                version.1,
                                TaskOutcome::Succeeded {
                                    summary: summary.to_string(),
                                    completed_at: crate::studio::unix_seconds(),
                                    review_gate: task_review_gate(readiness.review_gate())?,
                                },
                            )
                            .await?
                    }
                    CompletionOutcomeInput::Failed => {
                        self.task_runtime
                            .complete_task(
                                thread_id,
                                version.0,
                                version.1,
                                TaskOutcome::Failed {
                                    kind: TaskFailureKind::UnableToProceed,
                                    summary: summary.to_string(),
                                    evidence: required(
                                        input.evidence.as_deref(),
                                        "failed completion requires evidence",
                                    )?
                                    .to_string(),
                                    cause: required(
                                        input.cause.as_deref(),
                                        "failed completion requires cause",
                                    )?
                                    .to_string(),
                                    completed_at: crate::studio::unix_seconds(),
                                },
                            )
                            .await?
                    }
                };
                Ok((
                    run,
                    vec![serde_json::json!({"outcome": outcome, "summary": summary})],
                    vec![serde_json::json!({"kind": "finishTaskExecutions"})],
                    true,
                ))
            }
            TransitionAction::ResolveIssue => {
                let issue_id =
                    required(input.issue_id.as_deref(), "resolveIssue requires issueId")?;
                let summary = required(input.summary.as_deref(), "resolveIssue requires summary")?;
                let evidence = required(
                    input.resolution_evidence.as_deref(),
                    "resolveIssue requires resolutionEvidence",
                )?;
                let run = self
                    .task_runtime
                    .resolve_issue(
                        thread_id,
                        crate::studio::task_runtime::ResolveTaskIssue {
                            issue_id,
                            operation_id: requested_by_call_id,
                            summary,
                            evidence,
                            expected_revision: version.0,
                            expected_generation: version.1,
                        },
                    )
                    .await?;
                Ok((
                    run,
                    vec![
                        serde_json::json!({"issueId": issue_id, "summary": summary, "resolutionEvidence": evidence}),
                    ],
                    Vec::new(),
                    false,
                ))
            }
        }
    }

    pub(super) async fn transition_paths(
        &self,
        run: &TaskRun,
        runtime: Option<&AgentRuntimeHandle>,
    ) -> Result<Vec<TransitionPath>> {
        let aggregate = self
            .task_runtime
            .aggregate(&run.root_thread_id)
            .await
            .context("Task aggregate is not resident while assembling transition paths")?;
        anyhow::ensure!(
            aggregate.facts.run.id == run.id,
            "resident Task aggregate changed while assembling transition paths"
        );
        let work_units = aggregate.facts.work_units;
        let completions = aggregate.facts.completions;
        let merges = aggregate.facts.merges;
        let reviews = aggregate.facts.reviews;
        let pending_interactions = self
            .store
            .list_pending_interactions(&run.root_thread_id)
            .await?;
        let todo = self.store.read_thread_todo(&run.root_thread_id).await?;
        let issues = aggregate.facts.issues;
        let execution = self.model_execution_activity(run, runtime).await?;
        let completion_readiness =
            super::review::completion_readiness(super::review::CompletionReadinessInput {
                run,
                work_units: &work_units,
                completions: &completions,
                reviews: &reviews,
                merges: &merges,
                pending_interactions: &pending_interactions,
                todo: todo.as_ref(),
                execution: &execution,
            });
        let review_blockers =
            super::review::integrated_review_blockers(run, &work_units, &reviews, &merges);
        let cancel_blockers = if reviews.iter().any(|review| {
            review.scope == super::ReviewScope::Integrated && review.kind().is_active()
        }) {
            Vec::new()
        } else {
            vec!["没有可撤销的活动综合审查轮".to_string()]
        };
        let issue_blockers = if issues
            .iter()
            .any(|issue| issue.state.kind() != super::TaskIssueStateKind::Resolved)
        {
            Vec::new()
        } else {
            vec!["没有可解决的未关闭问题".to_string()]
        };

        let mut paths = Vec::new();
        for action in available_actions(run.kind()) {
            if action == TransitionAction::Complete {
                paths.push(transition_path(
                    action,
                    Some(CompletionOutcomeInput::Succeeded),
                    completion_readiness.blockers().to_vec(),
                ));
                paths.push(transition_path(
                    action,
                    Some(CompletionOutcomeInput::Failed),
                    Vec::new(),
                ));
                continue;
            }
            let missing_facts = match action {
                TransitionAction::BeginIntegratedReview => review_blockers.clone(),
                TransitionAction::CancelIntegratedReview => cancel_blockers.clone(),
                TransitionAction::ResolveIssue => issue_blockers.clone(),
                TransitionAction::SubmitPlan
                | TransitionAction::FinishDocumentEditing
                | TransitionAction::Complete => Vec::new(),
            };
            paths.push(transition_path(action, None, missing_facts));
        }
        Ok(paths)
    }
}

fn task_review_gate(gate: &StudioIntegratedReviewGate) -> Result<TaskReviewGate> {
    Ok(match gate {
        StudioIntegratedReviewGate::NotRequiredNoDelivery => TaskReviewGate::NotRequiredNoDelivery,
        StudioIntegratedReviewGate::NotRequiredSingleExecutorEquivalent {
            work_unit_id, ..
        } => TaskReviewGate::NotRequiredSingleExecutor {
            work_unit_id: work_unit_id.clone(),
        },
        StudioIntegratedReviewGate::SatisfiedByReview {
            review_round_id, ..
        } => TaskReviewGate::IntegratedReview {
            review_round_id: review_round_id.clone(),
        },
        StudioIntegratedReviewGate::Required { reason } => {
            bail!("integrated review is still required: {reason}")
        }
    })
}

impl TaskTransitionInput {
    fn validation_blockers(&self) -> Vec<TransitionBlocker> {
        let mut blockers = Vec::new();
        match self.action {
            TransitionAction::SubmitPlan | TransitionAction::FinishDocumentEditing => {
                require_input(&mut blockers, "summary", self.summary.as_deref());
            }
            TransitionAction::BeginIntegratedReview => {}
            TransitionAction::CancelIntegratedReview => {
                require_input(
                    &mut blockers,
                    "reviewRoundId",
                    self.review_round_id.as_deref(),
                );
                require_input(&mut blockers, "reason", self.reason.as_deref());
            }
            TransitionAction::Complete => {
                if self.outcome.is_none() {
                    blockers.push(TransitionBlocker {
                        code: "missingInput",
                        message: "缺少必要输入 outcome".to_string(),
                    });
                }
                require_input(&mut blockers, "summary", self.summary.as_deref());
                if self.outcome == Some(CompletionOutcomeInput::Failed) {
                    require_input(&mut blockers, "evidence", self.evidence.as_deref());
                    require_input(&mut blockers, "cause", self.cause.as_deref());
                }
            }
            TransitionAction::ResolveIssue => {
                require_input(&mut blockers, "issueId", self.issue_id.as_deref());
                require_input(&mut blockers, "summary", self.summary.as_deref());
                require_input(
                    &mut blockers,
                    "resolutionEvidence",
                    self.resolution_evidence.as_deref(),
                );
            }
        }
        blockers
    }
}

fn require_input(blockers: &mut Vec<TransitionBlocker>, field: &'static str, value: Option<&str>) {
    if value.is_none_or(|value| value.trim().is_empty()) {
        blockers.push(TransitionBlocker {
            code: "missingInput",
            message: format!("缺少必要输入 {field}"),
        });
    }
}

impl TaskTransitionOutput {
    fn rejected(
        run: &TaskRun,
        action: TransitionAction,
        code: &'static str,
        message: impl Into<String>,
        reasons: Vec<TransitionBlocker>,
        available_paths: Vec<TransitionPath>,
    ) -> Self {
        let message = message.into();
        Self::Rejected {
            code,
            message: message.clone(),
            current_state: run.kind(),
            requested_action: action,
            requested_state: requested_state(action),
            task_run_id: run.id.clone(),
            revision: run.revision,
            generation: run.generation(),
            reasons,
            available_paths,
        }
    }

    fn failed(
        run: &TaskRun,
        action: TransitionAction,
        cause: impl Into<String>,
        available_paths: Vec<TransitionPath>,
        canonical_state_verified: bool,
    ) -> Self {
        Self::failed_with_context(
            run,
            operational_failure_phase(action),
            operational_failure_resource(action),
            cause,
            available_paths,
            canonical_state_verified,
        )
    }

    fn failed_during_result_assembly(run: &TaskRun, cause: impl Into<String>) -> Self {
        Self::failed_with_context(
            run,
            "assembleTransitionResult",
            None,
            cause,
            state_only_transition_paths(run.kind()),
            true,
        )
    }

    fn failed_with_context(
        run: &TaskRun,
        phase: &'static str,
        resource: Option<&'static str>,
        cause: impl Into<String>,
        available_paths: Vec<TransitionPath>,
        canonical_state_verified: bool,
    ) -> Self {
        let cause = cause.into();
        let (message, compensation) = if canonical_state_verified {
            (
                "业务事实已持久化，但事务后的外部操作失败；请依据当前状态选择后续动作",
                serde_json::json!({
                    "status": "canonicalStatePreserved",
                    "detail": "已重新读取并返回提交或补偿后的 TaskRun；不会把有副作用的失败降级为拒绝"
                }),
            )
        } else {
            (
                "无法确认失败动作是否已持久化业务事实；必须重新查询状态后再选择后续动作",
                serde_json::json!({
                    "status": "stateVerificationRequired",
                    "detail": "返回的是调用前最后已知 TaskRun；在 task_status 成功前不得重试状态动作"
                }),
            )
        };
        Self::Failed {
            code: "transitionOperationFailed",
            phase,
            recoverable: true,
            message: message.to_string(),
            task_run_id: run.id.clone(),
            work_unit_id: None,
            review_round_id: run
                .state
                .review_target()
                .map(|target| target.review_round_id.clone()),
            agent_id: None,
            resource,
            cause,
            compensation,
            current_state: run.kind(),
            revision: run.revision,
            generation: run.generation(),
            available_paths,
        }
    }
}

fn task_record_changed(before: &TaskRun, after: &TaskRun) -> bool {
    before.id == after.id
        && (before.revision != after.revision
            || before.generation() != after.generation()
            || before.state != after.state)
}

fn operational_failure_phase(action: TransitionAction) -> &'static str {
    match action {
        TransitionAction::SubmitPlan => "publishPlanConfirmation",
        TransitionAction::FinishDocumentEditing => "queuePlannerContinuation",
        TransitionAction::BeginIntegratedReview => "spawnIntegratedReviewer",
        TransitionAction::CancelIntegratedReview => "cancelIntegratedReviewer",
        TransitionAction::Complete => "finishTaskExecutions",
        TransitionAction::ResolveIssue => "publishIssueResolution",
    }
}

fn operational_failure_resource(action: TransitionAction) -> Option<&'static str> {
    match action {
        TransitionAction::SubmitPlan => Some("planConfirmation"),
        TransitionAction::FinishDocumentEditing => Some("plannerContinuation"),
        TransitionAction::BeginIntegratedReview | TransitionAction::CancelIntegratedReview => {
            Some("integratedReviewer")
        }
        TransitionAction::Complete => Some("taskExecutions"),
        TransitionAction::ResolveIssue => None,
    }
}

fn transition_path(
    action: TransitionAction,
    outcome: Option<CompletionOutcomeInput>,
    missing_facts: Vec<String>,
) -> TransitionPath {
    let immediately_available = missing_facts.is_empty();
    TransitionPath {
        action,
        target_state: requested_state(action),
        outcome,
        immediately_available,
        required_inputs: required_inputs(action),
        satisfied_facts: if immediately_available {
            vec!["当前业务门槛已满足".to_string()]
        } else {
            Vec::new()
        },
        missing_facts,
    }
}

fn state_only_transition_paths(state: TaskRunStateKind) -> Vec<TransitionPath> {
    available_actions(state)
        .into_iter()
        .flat_map(|action| {
            if action == TransitionAction::Complete {
                vec![
                    transition_path(
                        action,
                        Some(CompletionOutcomeInput::Succeeded),
                        vec!["状态事实诊断失败，请重新调用 task_status".to_string()],
                    ),
                    transition_path(action, Some(CompletionOutcomeInput::Failed), Vec::new()),
                ]
            } else {
                vec![transition_path(action, None, Vec::new())]
            }
        })
        .collect()
}

fn blockers_for_rejected_action(
    action: TransitionAction,
    outcome: Option<CompletionOutcomeInput>,
    paths: &[TransitionPath],
) -> Vec<TransitionBlocker> {
    let mut reasons = Vec::new();
    for path in paths
        .iter()
        .filter(|path| path.action == action)
        .filter(|path| outcome.is_none() || path.outcome.is_none() || path.outcome == outcome)
    {
        for message in &path.missing_facts {
            push_unique_blocker(
                &mut reasons,
                TransitionBlocker {
                    code: "missingFact",
                    message: message.clone(),
                },
            );
        }
    }
    reasons
}

fn push_unique_blocker(reasons: &mut Vec<TransitionBlocker>, blocker: TransitionBlocker) {
    if !reasons
        .iter()
        .any(|existing| existing.code == blocker.code && existing.message == blocker.message)
    {
        reasons.push(blocker);
    }
}

fn transition_result(output: TaskTransitionOutput) -> Result<ToolExecutionResult> {
    ToolExecutionResult::<serde_json::Value>::json(output).map_err(anyhow::Error::from)
}

fn finalize_transition_result(
    result: ToolExecutionResult,
    action: TransitionAction,
    ends_turn: bool,
    summary: Option<String>,
) -> Result<ToolExecutionResult> {
    if !ends_turn {
        return Ok(result);
    }
    match action {
        TransitionAction::SubmitPlan => Ok(result
            .with_completed_plan(
                summary.context("successful submitPlan transition requires summary")?,
            )
            .ending_turn()),
        TransitionAction::Complete => Ok(result.ending_turn_with_content(
            summary.context("successful complete transition requires summary")?,
        )),
        TransitionAction::FinishDocumentEditing
        | TransitionAction::BeginIntegratedReview
        | TransitionAction::CancelIntegratedReview
        | TransitionAction::ResolveIssue => Ok(result.ending_turn()),
    }
}

fn required<'a>(value: Option<&'a str>, message: &str) -> Result<&'a str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context(message.to_string())
}

pub(crate) fn available_actions(state: TaskRunStateKind) -> Vec<TransitionAction> {
    let mut actions = match state {
        TaskRunStateKind::Planning => vec![TransitionAction::SubmitPlan],
        TaskRunStateKind::PendingConfirmation => Vec::new(),
        TaskRunStateKind::EditingDocuments => vec![TransitionAction::FinishDocumentEditing],
        TaskRunStateKind::Working => vec![
            TransitionAction::BeginIntegratedReview,
            TransitionAction::Complete,
        ],
        TaskRunStateKind::Reviewing => vec![
            TransitionAction::BeginIntegratedReview,
            TransitionAction::CancelIntegratedReview,
            TransitionAction::Complete,
        ],
        TaskRunStateKind::Completed => Vec::new(),
    };
    if !state.is_terminal() {
        actions.push(TransitionAction::ResolveIssue);
        if !actions.contains(&TransitionAction::Complete) {
            actions.push(TransitionAction::Complete);
        }
    }
    actions
}

fn requested_state(action: TransitionAction) -> &'static str {
    match action {
        TransitionAction::SubmitPlan => "pendingConfirmation",
        TransitionAction::FinishDocumentEditing | TransitionAction::CancelIntegratedReview => {
            "working"
        }
        TransitionAction::BeginIntegratedReview => "reviewing",
        TransitionAction::Complete => "completed",
        TransitionAction::ResolveIssue => "unchanged",
    }
}

pub(crate) fn required_inputs(action: TransitionAction) -> Vec<&'static str> {
    match action {
        TransitionAction::SubmitPlan | TransitionAction::FinishDocumentEditing => vec!["summary"],
        TransitionAction::BeginIntegratedReview => Vec::new(),
        TransitionAction::CancelIntegratedReview => vec!["reviewRoundId", "reason"],
        TransitionAction::Complete => vec!["outcome", "summary"],
        TransitionAction::ResolveIssue => vec!["issueId", "summary", "resolutionEvidence"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_run() -> TaskRun {
        TaskRun {
            context: super::super::TaskContext {
                id: "task-1".to_string(),
                project_id: "project-1".to_string(),
                root_thread_id: "thread-1".to_string(),
                request: "implement the requested change".to_string(),
                plan: None,
                workspace_root: "/tmp/project".to_string(),
            },
            state: super::super::TaskRunState::new(),
            revision: 1,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn input(action: TransitionAction) -> TaskTransitionInput {
        TaskTransitionInput {
            action,
            expected_revision: 1,
            expected_generation: 0,
            summary: None,
            review_round_id: None,
            reason: None,
            outcome: None,
            evidence: None,
            cause: None,
            issue_id: None,
            resolution_evidence: None,
        }
    }

    #[test]
    fn input_validation_reports_every_missing_field_at_once() {
        let cancel = input(TransitionAction::CancelIntegratedReview);
        assert_eq!(
            cancel
                .validation_blockers()
                .into_iter()
                .map(|blocker| blocker.message)
                .collect::<Vec<_>>(),
            ["缺少必要输入 reviewRoundId", "缺少必要输入 reason"]
        );

        let mut failed = input(TransitionAction::Complete);
        failed.outcome = Some(CompletionOutcomeInput::Failed);
        assert_eq!(
            failed
                .validation_blockers()
                .into_iter()
                .map(|blocker| blocker.message)
                .collect::<Vec<_>>(),
            [
                "缺少必要输入 summary",
                "缺少必要输入 evidence",
                "缺少必要输入 cause",
            ]
        );
    }

    #[test]
    fn transition_paths_keep_success_and_failure_completion_distinct() {
        let paths = state_only_transition_paths(TaskRunStateKind::Working);
        let completion_paths = paths
            .iter()
            .filter(|path| path.action == TransitionAction::Complete)
            .collect::<Vec<_>>();
        assert_eq!(completion_paths.len(), 2);
        assert_eq!(
            completion_paths[0].outcome,
            Some(CompletionOutcomeInput::Succeeded)
        );
        assert!(!completion_paths[0].immediately_available);
        assert_eq!(
            completion_paths[1].outcome,
            Some(CompletionOutcomeInput::Failed)
        );
        assert!(completion_paths[1].immediately_available);
    }

    #[test]
    fn operational_failure_requires_a_durable_task_record_change() {
        let before = task_run();
        assert!(!task_record_changed(&before, &before));

        let mut revised = before.clone();
        revised.revision += 1;
        assert!(task_record_changed(&before, &revised));

        let mut stopped = before.clone();
        stopped.state = stopped
            .state
            .clone()
            .advance_generation()
            .expect("planning generation should advance");
        assert!(task_record_changed(&before, &stopped));

        let mut unrelated = revised;
        unrelated.context.id = "task-2".to_string();
        assert!(!task_record_changed(&before, &unrelated));
    }

    #[test]
    fn uncertain_transition_audit_never_serializes_as_rejected() {
        let run = task_run();
        let output = TaskTransitionOutput::failed(
            &run,
            TransitionAction::FinishDocumentEditing,
            "canonical read unavailable",
            state_only_transition_paths(run.kind()),
            false,
        );
        let value = serde_json::to_value(output).expect("failure output should serialize");
        assert_eq!(value["status"], "failed");
        assert_eq!(value["code"], "transitionOperationFailed");
        assert_eq!(value["revision"], 1);
        assert_eq!(value["generation"], 0);
        assert_eq!(value["compensation"]["status"], "stateVerificationRequired");
    }

    #[test]
    fn accepted_submit_plan_emits_completed_plan_and_ends_turn() {
        let output = finalize_transition_result(
            ToolExecutionResult::success("{}"),
            TransitionAction::SubmitPlan,
            true,
            Some("# 实施计划\n\n- 修复展示".to_string()),
        )
        .expect("submitPlan result should be finalized")
        .into_tool_output();

        assert!(output.ends_turn());
        assert!(output.runtime_events.iter().any(|event| matches!(
            event,
            pl_core::ToolRuntimeEvent::PlanCompleted { content }
                if content == "# 实施计划\n\n- 修复展示"
        )));
    }
}
