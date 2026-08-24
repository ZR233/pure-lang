use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::{
    StudioBudgetLimitKind, StudioBudgetLimitRuntime, StudioBudgetUsageRuntime,
    StudioCancelledWorkUnit, StudioChangesRequiredWorkUnit, StudioCompletedTaskState,
    StudioCompletedWorkUnit, StudioEditingDocumentsTaskState, StudioExecutorContinuationState,
    StudioExecutorTerminalOutcome, StudioFailedWorkUnit, StudioIntegratedReviewGate,
    StudioIntegratedReviewTarget, StudioMergeCleanupState, StudioMergeMethod, StudioPausedWorkUnit,
    StudioPendingConfirmationTaskState, StudioPendingWorkUnit, StudioPlanningTaskState,
    StudioReadyForReviewCompletion, StudioReviewPassedOutcome, StudioReviewPassedWorkUnit,
    StudioReviewScope, StudioReviewedCompletion, StudioReviewingTaskState, StudioRunningWorkUnit,
    StudioRunningWorkUnitActivity, StudioTaskCompletionContent, StudioTaskCompletionRuntime,
    StudioTaskCompletionState, StudioTaskDeliveryCompletion, StudioTaskDesignReferenceRuntime,
    StudioTaskFailureKind, StudioTaskIssueRuntime, StudioTaskIssueState, StudioTaskMergeRuntime,
    StudioTaskNoDeliveryCompletion, StudioTaskOutcome, StudioTaskReviewFindingRuntime,
    StudioTaskReviewGate, StudioTaskReviewRuntime, StudioTaskReviewState, StudioTaskRuntime,
    StudioTaskSpawnCompensation, StudioTaskSpawnCompensationState, StudioTaskSpawnFailure,
    StudioTaskSpawnFailureCode, StudioTaskSpawnFailurePhase, StudioTaskSpawnNextAction,
    StudioTaskSpawnResource, StudioTaskState, StudioTaskWorkUnitRuntime, StudioTaskWorkUnitState,
    StudioTaskWorktreeDisposition, StudioWaitingReviewPhase, StudioWaitingReviewWorkUnit,
    StudioWorkUnitCompletionOutcome, StudioWorkUnitFailure, StudioWorkUnitPauseReason,
    StudioWorkingTaskState, StudioWorktreeFailureCause, StudioWorktreeFailureCauseKind,
};

use super::{
    StudioStore,
    task_coordinator::{
        ExecutorContinuationState, MergeCleanupState, MergeRecord, ReviewPassedOutcome,
        ReviewRoundRecord, ReviewRoundState, RunningActivity, TaskIssueState, TaskOutcome,
        TaskReviewGate, TaskRun, TaskRunState, WaitingReviewPhase, WorkCompletionKind,
        WorkCompletionRecord, WorkCompletionStatus, WorkUnitCompletionOutcome, WorkUnitFailure,
        WorkUnitPauseReason, WorkUnitState, WorkUnitStateKind,
    },
};

/// 从 SQLite 冷基线一次性恢复出的完整 Task 聚合。
///
/// 该类型只在冷加载边界构造；活动查询由 `TaskRuntime` 克隆内存中的同一聚合，
/// 不得再分别查询 TaskRun、WorkUnit、Completion、Merge、Review 和 Issue 表。
#[derive(Debug, Clone)]
pub(crate) struct LoadedTaskAggregate {
    pub(crate) run: TaskRun,
    pub(crate) work_units: Vec<super::task_coordinator::WorkUnit>,
    pub(crate) completions: Vec<WorkCompletionRecord>,
    pub(crate) merges: Vec<MergeRecord>,
    pub(crate) reviews: Vec<ReviewRoundRecord>,
    pub(crate) issues: Vec<super::task_coordinator::TaskIssueRecord>,
    pub(crate) runtime: StudioTaskRuntime,
}

impl LoadedTaskAggregate {
    /// 为新 TaskRun 构造不依赖 SQLite 的空子事实聚合。
    pub(crate) fn new(run: TaskRun) -> Result<Self> {
        let runtime = studio_task_runtime(
            run.clone(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            StudioIntegratedReviewGate::NotRequiredNoDelivery,
        )?;
        Ok(Self {
            run,
            work_units: Vec::new(),
            completions: Vec::new(),
            merges: Vec::new(),
            reviews: Vec::new(),
            issues: Vec::new(),
            runtime,
        })
    }

    /// TaskRun 单事实转换后刷新热目录投影；不访问 SQLite。
    pub(crate) fn refresh_run_projection(&mut self) -> Result<()> {
        self.runtime.run_id.clone_from(&self.run.id);
        self.runtime.state = studio_task_state(&self.run)?;
        self.runtime.revision = self.run.revision;
        self.runtime.generation = self.run.generation();
        Ok(())
    }

    /// 从内存领域事实重建完整热投影；保留 handoff 与执行进度等 Thread owner 元数据。
    pub(crate) fn refresh_projection(&mut self) -> Result<()> {
        let previous = self
            .runtime
            .work_units
            .iter()
            .cloned()
            .map(|runtime| (runtime.id.clone(), runtime))
            .collect::<HashMap<_, _>>();
        let work_units = self
            .work_units
            .iter()
            .map(|unit| {
                let previous = previous.get(&unit.id);
                Ok(StudioTaskWorkUnitRuntime {
                    id: unit.id.clone(),
                    title: unit.title.clone(),
                    state: studio_work_unit_state(&unit.state)?,
                    worktree_path: unit.worktree_path.clone(),
                    branch: unit.branch.clone(),
                    agent_id: unit.executor_thread_id.clone(),
                    attempt: unit.attempt,
                    supersedes_work_unit_id: unit.supersedes_work_unit_id.clone(),
                    budget_slice_limit: crate::studio::task_coordinator::MAX_EXECUTOR_BUDGET_SLICES,
                    executor_progress_revision: previous
                        .map_or(0, |runtime| runtime.executor_progress_revision),
                    blueprint_fingerprint: previous
                        .and_then(|runtime| runtime.blueprint_fingerprint.clone()),
                    objective: previous.and_then(|runtime| runtime.objective.clone()),
                    implementation_step_count: previous
                        .map_or(0, |runtime| runtime.implementation_step_count),
                    acceptance_criterion_count: previous
                        .map_or(0, |runtime| runtime.acceptance_criterion_count),
                    verification_count: previous.map_or(0, |runtime| runtime.verification_count),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let gate = super::task_coordinator::review::integrated_review_gate_now(
            &self.run,
            &self.work_units,
            &self.completions,
            &self.merges,
            &self.reviews,
        );
        self.runtime = studio_task_runtime(
            self.run.clone(),
            work_units,
            self.completions.clone(),
            self.merges.clone(),
            self.reviews.clone(),
            self.issues.clone(),
            gate,
        )?;
        Ok(())
    }
}

pub(crate) async fn load_task_aggregate(
    store: &StudioStore,
    root_thread_id: &str,
) -> Result<Option<LoadedTaskAggregate>> {
    let Some(run) = store
        .find_latest_task_run_for_root_thread(root_thread_id)
        .await?
    else {
        return Ok(None);
    };
    let work_units = store.list_work_units(&run.id).await?;
    let mut work_unit_runtimes = Vec::with_capacity(work_units.len());
    for unit in &work_units {
        let executor_progress_revision = if let Some(executor_thread_id) = &unit.executor_thread_id
        {
            store
                .read_thread_runtime_revision(executor_thread_id)
                .await?
        } else {
            0
        };
        let handoff = if unit.kind() == WorkUnitStateKind::Pending {
            None
        } else {
            store
                .read_work_unit_handoff(&unit.id)
                .await
                .with_context(|| format!("failed to load Task handoff for work unit {}", unit.id))?
                .map(|(_, handoff)| handoff)
        };
        work_unit_runtimes.push(StudioTaskWorkUnitRuntime {
            id: unit.id.clone(),
            title: unit.title.clone(),
            state: studio_work_unit_state(&unit.state)?,
            worktree_path: unit.worktree_path.clone(),
            branch: unit.branch.clone(),
            agent_id: unit.executor_thread_id.clone(),
            attempt: unit.attempt,
            supersedes_work_unit_id: unit.supersedes_work_unit_id.clone(),
            budget_slice_limit: crate::studio::task_coordinator::MAX_EXECUTOR_BUDGET_SLICES,
            executor_progress_revision,
            blueprint_fingerprint: handoff
                .as_ref()
                .map(|handoff| handoff.blueprint_fingerprint.clone()),
            objective: handoff
                .as_ref()
                .map(|handoff| handoff.blueprint.objective.clone()),
            implementation_step_count: handoff
                .as_ref()
                .map_or(0, |handoff| handoff.blueprint.implementation_steps.len()),
            acceptance_criterion_count: handoff
                .as_ref()
                .map_or(0, |handoff| handoff.blueprint.acceptance_criteria.len()),
            verification_count: handoff
                .as_ref()
                .map_or(0, |handoff| handoff.blueprint.verification_count()),
        });
    }
    let completions = store.list_work_completions(&run.id).await?;
    let merges = store.list_merge_records(&run.id).await?;
    let reviews = store.list_review_rounds(&run.id).await?;
    let integrated_review_gate = super::task_coordinator::review::integrated_review_gate(
        &run,
        &work_units,
        &completions,
        &merges,
        &reviews,
    )
    .await;
    let issues = store.list_task_issues(&run.id).await?;
    let runtime = studio_task_runtime(
        run.clone(),
        work_unit_runtimes,
        completions.clone(),
        merges.clone(),
        reviews.clone(),
        issues.clone(),
        integrated_review_gate,
    )?;
    Ok(Some(LoadedTaskAggregate {
        run,
        work_units,
        completions,
        merges,
        reviews,
        issues,
        runtime,
    }))
}

fn studio_task_runtime(
    run: TaskRun,
    work_units: Vec<StudioTaskWorkUnitRuntime>,
    completions: Vec<WorkCompletionRecord>,
    merges: Vec<MergeRecord>,
    reviews: Vec<ReviewRoundRecord>,
    issues: Vec<super::task_coordinator::TaskIssueRecord>,
    integrated_review_gate: StudioIntegratedReviewGate,
) -> Result<StudioTaskRuntime> {
    let issues = issues
        .into_iter()
        .map(|issue| {
            let state = studio_task_issue_state(&issue);
            StudioTaskIssueRuntime {
                id: issue.id,
                source_thread_id: issue.source_thread_id,
                source_turn_id: issue.source_turn_id,
                source_agent_id: issue.source_agent_id,
                source_role: issue.source_role,
                work_unit_id: issue.work_unit_id,
                review_round_id: issue.review_round_id,
                state,
                created_at: issue.created_at,
            }
        })
        .collect::<Vec<_>>();
    Ok(StudioTaskRuntime {
        run_id: run.id.clone(),
        state: studio_task_state(&run)?,
        revision: run.revision,
        generation: run.generation(),
        integrated_review_gate,
        issues,
        work_units,
        completions: completions
            .into_iter()
            .map(completion_runtime)
            .collect::<Result<Vec<_>>>()?,
        merges: merges
            .into_iter()
            .map(|merge| StudioTaskMergeRuntime {
                id: merge.id,
                work_unit_id: merge.work_unit_id,
                completion_id: merge.completion_id,
                completion_revision: merge.completion_revision,
                executor_agent_id: merge.executor_agent_id,
                expected_previous_head: merge.expected_previous_head,
                resulting_head: merge.resulting_head,
                delivery_head: merge.delivery_head,
                method: match merge.method {
                    super::task_coordinator::MergeMethod::Merge => StudioMergeMethod::Merge,
                    super::task_coordinator::MergeMethod::CherryPick => {
                        StudioMergeMethod::CherryPick
                    }
                    super::task_coordinator::MergeMethod::Squash => StudioMergeMethod::Squash,
                    super::task_coordinator::MergeMethod::Rebase => StudioMergeMethod::Rebase,
                    super::task_coordinator::MergeMethod::Manual => StudioMergeMethod::Manual,
                },
                summary: merge.summary,
                cleanup: studio_merge_cleanup_state(&merge.cleanup),
                created_at: merge.created_at,
                updated_at: merge.updated_at,
            })
            .collect(),
        reviews: reviews
            .into_iter()
            .map(|review| StudioTaskReviewRuntime {
                id: review.id,
                round: review.round,
                scope: match review.scope {
                    super::task_coordinator::ReviewScope::Delivery => StudioReviewScope::Delivery,
                    super::task_coordinator::ReviewScope::Integrated => {
                        StudioReviewScope::Integrated
                    }
                },
                work_unit_id: review.work_unit_id,
                completion_id: review.completion_id,
                completion_revision: review.completion_revision,
                reviewed_head: review.reviewed_head,
                state: studio_review_state(&review.state),
                requested_by_call_id: review.requested_by_call_id,
                design_references: review
                    .design_references
                    .into_iter()
                    .map(|reference| StudioTaskDesignReferenceRuntime {
                        path: reference.path,
                        section: reference.section,
                    })
                    .collect(),
                findings: review
                    .findings
                    .into_iter()
                    .map(|finding| StudioTaskReviewFindingRuntime {
                        severity: finding.severity,
                        title: finding.title,
                        body: finding.body,
                        recommendation: finding.recommendation,
                        path: finding.path,
                        line: finding.line,
                        design_references: finding
                            .design_references
                            .into_iter()
                            .map(|reference| StudioTaskDesignReferenceRuntime {
                                path: reference.path,
                                section: reference.section,
                            })
                            .collect(),
                    })
                    .collect(),
                created_at: review.created_at,
                updated_at: review.updated_at,
            })
            .collect(),
    })
}

fn completion_runtime(completion: WorkCompletionRecord) -> Result<StudioTaskCompletionRuntime> {
    let content = match completion.kind() {
        WorkCompletionKind::Delivery => {
            StudioTaskCompletionContent::Delivery(StudioTaskDeliveryCompletion {
                head_commit: completion
                    .head_commit()
                    .map(str::to_string)
                    .context("delivery completion has no head commit")?,
                changed_files: completion.changed_files().to_vec(),
            })
        }
        WorkCompletionKind::NoDelivery => {
            StudioTaskCompletionContent::NoDelivery(StudioTaskNoDeliveryCompletion {})
        }
    };
    let reviewed = || -> Result<StudioReviewedCompletion> {
        Ok(StudioReviewedCompletion {
            review_round_id: completion
                .state
                .review_round_id()
                .map(str::to_string)
                .context("reviewed completion has no review round")?,
            decided_at: completion
                .state
                .decided_at()
                .context("reviewed completion has no decision timestamp")?,
        })
    };
    let state = match completion.status() {
        WorkCompletionStatus::ReadyForReview => {
            StudioTaskCompletionState::ReadyForReview(StudioReadyForReviewCompletion {})
        }
        WorkCompletionStatus::ChangesRequired => {
            StudioTaskCompletionState::ChangesRequired(reviewed()?)
        }
        WorkCompletionStatus::Approved => StudioTaskCompletionState::Approved(reviewed()?),
    };
    Ok(StudioTaskCompletionRuntime {
        id: completion.id,
        work_unit_id: completion.work_unit_id,
        executor_agent_id: completion.executor_agent_id,
        revision: completion.revision,
        content,
        state,
        state_revision: completion.state_revision,
        base_commit: completion.base_commit,
        verification_summary: completion.verification_summary,
        worktree_path: completion.worktree_path,
        branch: completion.branch,
        created_at: completion.created_at,
        updated_at: completion.updated_at,
    })
}

fn studio_merge_cleanup_state(state: &MergeCleanupState) -> StudioMergeCleanupState {
    match state {
        MergeCleanupState::Pending(_) => StudioMergeCleanupState::Pending,
        MergeCleanupState::Deferred(_) => StudioMergeCleanupState::Deferred,
        MergeCleanupState::Attempting(state) => StudioMergeCleanupState::Attempting {
            operation_id: state.operation_id().to_string(),
            started_at: state.started_at(),
        },
        MergeCleanupState::Discarded(state) => StudioMergeCleanupState::Discarded {
            operation_id: state.operation_id().to_string(),
            completed_at: state.completed_at(),
        },
        MergeCleanupState::AlreadyAbsent(state) => StudioMergeCleanupState::AlreadyAbsent {
            operation_id: state.operation_id().to_string(),
            completed_at: state.completed_at(),
        },
        MergeCleanupState::Failed(state) => StudioMergeCleanupState::Failed {
            operation_id: state.operation_id().to_string(),
            failed_at: state.failed_at(),
            detail: state.detail().to_string(),
        },
    }
}

fn studio_task_issue_state(
    failure: &super::task_coordinator::TaskIssueRecord,
) -> StudioTaskIssueState {
    match &failure.state {
        TaskIssueState::OpenRecoverable(_) => StudioTaskIssueState::OpenRecoverable {
            failure: failure.failure().clone(),
        },
        TaskIssueState::OpenFatal(_) => StudioTaskIssueState::OpenFatal {
            failure: failure.failure().clone(),
        },
        TaskIssueState::Resolved(state) => StudioTaskIssueState::Resolved {
            failure: failure.failure().clone(),
            resolved_at: state.resolved_at(),
        },
    }
}

fn studio_task_state(run: &TaskRun) -> Result<StudioTaskState> {
    Ok(match &run.state {
        TaskRunState::Planning(_) => StudioTaskState::Planning(StudioPlanningTaskState {
            request: run.context.request.clone(),
        }),
        TaskRunState::PendingConfirmation(_) => {
            StudioTaskState::PendingConfirmation(StudioPendingConfirmationTaskState {
                plan_revision: run
                    .state
                    .plan_revision()
                    .context("pending confirmation has no plan revision")?,
            })
        }
        TaskRunState::EditingDocuments(_) => {
            StudioTaskState::EditingDocuments(StudioEditingDocumentsTaskState {
                plan_revision: run
                    .state
                    .plan_revision()
                    .context("document editing has no plan revision")?,
            })
        }
        TaskRunState::Working(_) => StudioTaskState::Working(StudioWorkingTaskState {
            document_edit_summary: run
                .state
                .document_edit_summary()
                .context("working task has no document edit summary")?
                .to_string(),
        }),
        TaskRunState::Reviewing(_) => {
            let target = run
                .state
                .review_target()
                .context("reviewing task has no frozen target")?;
            StudioTaskState::Reviewing(StudioReviewingTaskState {
                target: StudioIntegratedReviewTarget {
                    review_round_id: target.review_round_id.clone(),
                    reviewed_head: target.reviewed_head.clone(),
                    changed_files: target.changed_files.clone(),
                },
            })
        }
        TaskRunState::Completed(_) => StudioTaskState::Completed(StudioCompletedTaskState {
            outcome: studio_task_outcome(
                run.state
                    .outcome()
                    .context("completed task has no outcome")?,
            ),
        }),
    })
}

fn studio_task_outcome(outcome: &TaskOutcome) -> StudioTaskOutcome {
    match outcome {
        TaskOutcome::Succeeded {
            summary,
            completed_at,
            review_gate,
        } => StudioTaskOutcome::Succeeded {
            summary: summary.clone(),
            completed_at: *completed_at,
            review_gate: match review_gate {
                TaskReviewGate::NotRequiredNoDelivery => {
                    StudioTaskReviewGate::NotRequiredNoDelivery
                }
                TaskReviewGate::NotRequiredSingleExecutor { work_unit_id } => {
                    StudioTaskReviewGate::NotRequiredSingleExecutor {
                        work_unit_id: work_unit_id.clone(),
                    }
                }
                TaskReviewGate::IntegratedReview { review_round_id } => {
                    StudioTaskReviewGate::IntegratedReview {
                        review_round_id: review_round_id.clone(),
                    }
                }
            },
        },
        TaskOutcome::Failed {
            kind,
            summary,
            evidence,
            cause,
            completed_at,
        } => StudioTaskOutcome::Failed {
            kind: match kind {
                super::task_coordinator::TaskFailureKind::UnableToProceed => {
                    StudioTaskFailureKind::UnableToProceed
                }
                super::task_coordinator::TaskFailureKind::Fatal => StudioTaskFailureKind::Fatal,
            },
            summary: summary.clone(),
            evidence: evidence.clone(),
            cause: cause.clone(),
            completed_at: *completed_at,
        },
    }
}

fn studio_work_unit_state(state: &WorkUnitState) -> Result<StudioTaskWorkUnitState> {
    Ok(match state {
        WorkUnitState::Pending(_) => StudioTaskWorkUnitState::Pending(StudioPendingWorkUnit {}),
        WorkUnitState::Running(value) => StudioTaskWorkUnitState::Running(StudioRunningWorkUnit {
            activity: match value.activity() {
                RunningActivity::Allocated => StudioRunningWorkUnitActivity::Allocated,
                RunningActivity::Active { turn_id } => StudioRunningWorkUnitActivity::Active {
                    turn_id: turn_id.clone(),
                },
            },
            continuation: studio_executor_continuation(value.continuation())?,
        }),
        WorkUnitState::WaitingReview(value) => {
            let phase = match value.phase() {
                WaitingReviewPhase::AwaitingReport(value) => {
                    StudioWaitingReviewPhase::AwaitingReport {
                        outcome: match value.outcome() {
                            super::task_coordinator::ExecutorTerminalOutcome::Completed {
                                source_turn_id,
                                detail,
                            } => StudioExecutorTerminalOutcome::Completed {
                                source_turn_id: source_turn_id.clone(),
                                detail: detail.clone(),
                            },
                            super::task_coordinator::ExecutorTerminalOutcome::Failed {
                                source_turn_id,
                                detail,
                            } => StudioExecutorTerminalOutcome::Failed {
                                source_turn_id: source_turn_id.clone(),
                                detail: detail.clone(),
                            },
                        },
                        continuation: studio_executor_continuation(value.continuation())?,
                    }
                }
                WaitingReviewPhase::Ready(value) => StudioWaitingReviewPhase::Ready {
                    completion_id: value.completion_id().to_string(),
                    completion_revision: value.completion_revision(),
                    verification_summary: value.verification_summary().to_string(),
                },
                WaitingReviewPhase::Reviewing(value) => StudioWaitingReviewPhase::Reviewing {
                    completion_id: value.completion_id().to_string(),
                    completion_revision: value.completion_revision(),
                    review_round_id: value.review_round_id().to_string(),
                    verification_summary: value.verification_summary().to_string(),
                },
            };
            StudioTaskWorkUnitState::WaitingReview(StudioWaitingReviewWorkUnit { phase })
        }
        WorkUnitState::ReviewPassed(value) => {
            StudioTaskWorkUnitState::ReviewPassed(StudioReviewPassedWorkUnit {
                completion_id: value.completion_id().to_string(),
                completion_revision: value.completion_revision(),
                review_round_id: value.review_round_id().to_string(),
                outcome: match value.outcome() {
                    ReviewPassedOutcome::Delivery => StudioReviewPassedOutcome::Delivery,
                    ReviewPassedOutcome::NoDelivery => StudioReviewPassedOutcome::NoDelivery,
                },
                verification_summary: value.verification_summary().to_string(),
            })
        }
        WorkUnitState::ChangesRequired(value) => {
            StudioTaskWorkUnitState::ChangesRequired(StudioChangesRequiredWorkUnit {
                completion_id: value.completion_id().to_string(),
                completion_revision: value.completion_revision(),
                review_round_id: value.review_round_id().to_string(),
                continuation_revision: value.continuation_revision(),
                slice_count: value.slice_count(),
            })
        }
        WorkUnitState::Paused(value) => StudioTaskWorkUnitState::Paused(StudioPausedWorkUnit {
            reason: match value.reason() {
                WorkUnitPauseReason::Budget { limit } => StudioWorkUnitPauseReason::Budget {
                    limit: studio_budget_limit(limit),
                },
                WorkUnitPauseReason::Operational {
                    operation_id,
                    detail,
                } => StudioWorkUnitPauseReason::Operational {
                    operation_id: operation_id.clone(),
                    detail: detail.clone(),
                },
            },
            continuation: studio_executor_continuation(value.continuation())?,
        }),
        WorkUnitState::Completed(value) => {
            StudioTaskWorkUnitState::Completed(StudioCompletedWorkUnit {
                outcome: match value.outcome() {
                    WorkUnitCompletionOutcome::Merged { merge_record_id } => {
                        StudioWorkUnitCompletionOutcome::Merged {
                            merge_record_id: merge_record_id.clone(),
                        }
                    }
                    WorkUnitCompletionOutcome::NoDelivery { completion_id } => {
                        StudioWorkUnitCompletionOutcome::NoDelivery {
                            completion_id: completion_id.clone(),
                        }
                    }
                },
            })
        }
        WorkUnitState::Failed(value) => StudioTaskWorkUnitState::Failed(StudioFailedWorkUnit {
            failure: match value.failure() {
                WorkUnitFailure::Spawn(failure) => StudioWorkUnitFailure::Spawn {
                    failure: Box::new(studio_spawn_failure(failure)),
                },
                WorkUnitFailure::Execution {
                    operation_id,
                    detail,
                } => StudioWorkUnitFailure::Execution {
                    operation_id: operation_id.clone(),
                    detail: detail.clone(),
                },
            },
            worktree_disposition: match value.worktree_disposition() {
                super::task_coordinator::TaskWorktreeDisposition::Protect => {
                    StudioTaskWorktreeDisposition::Protect
                }
                super::task_coordinator::TaskWorktreeDisposition::CleanupRequested => {
                    StudioTaskWorktreeDisposition::CleanupRequested
                }
            },
        }),
        WorkUnitState::Cancelled(value) => {
            StudioTaskWorkUnitState::Cancelled(StudioCancelledWorkUnit {
                operation_id: value.operation_id().to_string(),
                reason: value.reason().to_string(),
                worktree_disposition: match value.worktree_disposition() {
                    super::task_coordinator::TaskWorktreeDisposition::Protect => {
                        StudioTaskWorktreeDisposition::Protect
                    }
                    super::task_coordinator::TaskWorktreeDisposition::CleanupRequested => {
                        StudioTaskWorktreeDisposition::CleanupRequested
                    }
                },
            })
        }
    })
}

fn studio_executor_continuation(
    state: &ExecutorContinuationState,
) -> Result<StudioExecutorContinuationState> {
    let revision = state.revision();
    let slice_count = state.slice_count();
    let source_turn_id = || {
        state
            .source_turn_id()
            .map(str::to_string)
            .context("active executor continuation has no source Turn")
    };
    Ok(match state {
        ExecutorContinuationState::Idle(_) => StudioExecutorContinuationState::Idle {
            revision,
            slice_count,
        },
        ExecutorContinuationState::Compacting(_) => StudioExecutorContinuationState::Compacting {
            revision,
            source_turn_id: source_turn_id()?,
            slice_count,
        },
        ExecutorContinuationState::PendingStart(_) => {
            StudioExecutorContinuationState::PendingStart {
                revision,
                source_turn_id: source_turn_id()?,
                slice_count,
                limit: studio_budget_limit(
                    state
                        .budget_limit()
                        .context("pending executor continuation has no budget snapshot")?,
                ),
            }
        }
        ExecutorContinuationState::PlannerWakePending(_) => {
            StudioExecutorContinuationState::PlannerWakePending {
                revision,
                source_turn_id: source_turn_id()?,
                slice_count,
            }
        }
        ExecutorContinuationState::NeedsAttention(_) => {
            StudioExecutorContinuationState::NeedsAttention {
                revision,
                source_turn_id: source_turn_id()?,
                slice_count,
                detail: state
                    .detail()
                    .context("attention continuation has no detail")?
                    .to_string(),
            }
        }
    })
}

fn studio_budget_limit(limit: &pl_protocol::BudgetLimitSnapshot) -> StudioBudgetLimitRuntime {
    StudioBudgetLimitRuntime {
        kind: match limit.kind {
            pl_protocol::BudgetLimitKind::ModelStep => StudioBudgetLimitKind::ModelStep,
            pl_protocol::BudgetLimitKind::ToolCall => StudioBudgetLimitKind::ToolCall,
            pl_protocol::BudgetLimitKind::Wait => StudioBudgetLimitKind::Wait,
            pl_protocol::BudgetLimitKind::WallClock => StudioBudgetLimitKind::WallClock,
            pl_protocol::BudgetLimitKind::AgentCount => StudioBudgetLimitKind::AgentCount,
            pl_protocol::BudgetLimitKind::AgentDepth => StudioBudgetLimitKind::AgentDepth,
            pl_protocol::BudgetLimitKind::Finalization => StudioBudgetLimitKind::Finalization,
        },
        usage: StudioBudgetUsageRuntime {
            model_steps: limit.usage.model_steps,
            tool_calls: limit.usage.tool_calls,
            wait_calls: limit.usage.wait_calls,
            elapsed_ms: limit.usage.elapsed_ms,
        },
    }
}

fn studio_spawn_failure(
    failure: &super::task_coordinator::TaskSpawnFailure,
) -> StudioTaskSpawnFailure {
    StudioTaskSpawnFailure {
        code: match failure.code {
            super::task_coordinator::TaskSpawnFailureCode::Allocation => {
                StudioTaskSpawnFailureCode::Allocation
            }
            super::task_coordinator::TaskSpawnFailureCode::WorktreeCreate => {
                StudioTaskSpawnFailureCode::WorktreeCreate
            }
            super::task_coordinator::TaskSpawnFailureCode::ChildThreadCreate => {
                StudioTaskSpawnFailureCode::ChildThreadCreate
            }
            super::task_coordinator::TaskSpawnFailureCode::AgentRegistration => {
                StudioTaskSpawnFailureCode::AgentRegistration
            }
            super::task_coordinator::TaskSpawnFailureCode::Activation => {
                StudioTaskSpawnFailureCode::Activation
            }
        },
        phase: match failure.phase {
            super::task_coordinator::TaskSpawnFailurePhase::Allocation => {
                StudioTaskSpawnFailurePhase::Allocation
            }
            super::task_coordinator::TaskSpawnFailurePhase::WorktreeCreate => {
                StudioTaskSpawnFailurePhase::WorktreeCreate
            }
            super::task_coordinator::TaskSpawnFailurePhase::ChildThreadCreate => {
                StudioTaskSpawnFailurePhase::ChildThreadCreate
            }
            super::task_coordinator::TaskSpawnFailurePhase::AgentRegistration => {
                StudioTaskSpawnFailurePhase::AgentRegistration
            }
            super::task_coordinator::TaskSpawnFailurePhase::Activation => {
                StudioTaskSpawnFailurePhase::Activation
            }
        },
        recoverable: failure.recoverable,
        message: failure.message.clone(),
        task_run_id: failure.task_run_id.clone(),
        work_unit_id: failure.work_unit_id.clone(),
        agent_id: failure.agent_id.clone(),
        resource: failure
            .resource
            .as_ref()
            .map(|resource| StudioTaskSpawnResource {
                repo_root: resource.repo_root.clone(),
                path: resource.path.clone(),
                branch: resource.branch.clone(),
                base_ref: resource.base_ref.clone(),
            }),
        cause: StudioWorktreeFailureCause {
            kind: match failure.cause.kind {
                crate::agent::worktree::WorktreeFailureCauseKind::InvalidRepoRoot => {
                    StudioWorktreeFailureCauseKind::InvalidRepoRoot
                }
                crate::agent::worktree::WorktreeFailureCauseKind::UnsafeBranch => {
                    StudioWorktreeFailureCauseKind::UnsafeBranch
                }
                crate::agent::worktree::WorktreeFailureCauseKind::GitLaunchFailed => {
                    StudioWorktreeFailureCauseKind::GitLaunchFailed
                }
                crate::agent::worktree::WorktreeFailureCauseKind::GitTimedOut => {
                    StudioWorktreeFailureCauseKind::GitTimedOut
                }
                crate::agent::worktree::WorktreeFailureCauseKind::GitExited => {
                    StudioWorktreeFailureCauseKind::GitExited
                }
                crate::agent::worktree::WorktreeFailureCauseKind::GitStatusUnknown => {
                    StudioWorktreeFailureCauseKind::GitStatusUnknown
                }
                crate::agent::worktree::WorktreeFailureCauseKind::Io => {
                    StudioWorktreeFailureCauseKind::Io
                }
                crate::agent::worktree::WorktreeFailureCauseKind::Disabled => {
                    StudioWorktreeFailureCauseKind::Disabled
                }
                crate::agent::worktree::WorktreeFailureCauseKind::OperationAndCleanupFailed => {
                    StudioWorktreeFailureCauseKind::OperationAndCleanupFailed
                }
            },
            message: failure.cause.message.clone(),
            args: failure.cause.args.clone(),
            exit_code: failure.cause.exit_code,
            stderr: failure.cause.stderr.clone(),
        },
        compensation: StudioTaskSpawnCompensation {
            allocation: studio_spawn_compensation(failure.compensation.allocation),
            worktree: studio_spawn_compensation(failure.compensation.worktree),
            child_thread: studio_spawn_compensation(failure.compensation.child_thread),
        },
        next_action: match failure.next_action {
            super::task_coordinator::TaskSpawnNextAction::RetryTaskSpawnExecutor => {
                StudioTaskSpawnNextAction::RetryTaskSpawnExecutor
            }
            super::task_coordinator::TaskSpawnNextAction::RecoverWorktreeResources => {
                StudioTaskSpawnNextAction::RecoverWorktreeResources
            }
        },
    }
}

fn studio_spawn_compensation(
    state: super::task_coordinator::TaskSpawnCompensationState,
) -> StudioTaskSpawnCompensationState {
    match state {
        super::task_coordinator::TaskSpawnCompensationState::NotCreated => {
            StudioTaskSpawnCompensationState::NotCreated
        }
        super::task_coordinator::TaskSpawnCompensationState::MarkedFailed => {
            StudioTaskSpawnCompensationState::MarkedFailed
        }
        super::task_coordinator::TaskSpawnCompensationState::Removed => {
            StudioTaskSpawnCompensationState::Removed
        }
        super::task_coordinator::TaskSpawnCompensationState::Faulted => {
            StudioTaskSpawnCompensationState::Faulted
        }
        super::task_coordinator::TaskSpawnCompensationState::CleanupFailed => {
            StudioTaskSpawnCompensationState::CleanupFailed
        }
        super::task_coordinator::TaskSpawnCompensationState::Unknown => {
            StudioTaskSpawnCompensationState::Unknown
        }
    }
}

fn studio_review_state(state: &ReviewRoundState) -> StudioTaskReviewState {
    match state {
        ReviewRoundState::PendingDispatch(_) => StudioTaskReviewState::PendingDispatch,
        ReviewRoundState::Dispatched(state) => StudioTaskReviewState::Dispatched {
            reviewer_agent_id: state.reviewer_thread_id().to_string(),
        },
        ReviewRoundState::Running(state) => StudioTaskReviewState::Running {
            reviewer_agent_id: state.reviewer_thread_id().to_string(),
        },
        ReviewRoundState::Passed(state) => StudioTaskReviewState::Passed {
            reviewer_agent_id: state.reviewer_thread_id().to_string(),
            summary: state.summary().to_string(),
        },
        ReviewRoundState::ChangesRequired(state) => StudioTaskReviewState::ChangesRequired {
            reviewer_agent_id: state.reviewer_thread_id().to_string(),
            summary: state.summary().to_string(),
        },
        ReviewRoundState::Blocked(state) => StudioTaskReviewState::Blocked {
            reviewer_agent_id: state.reviewer_thread_id().to_string(),
            summary: state.summary().to_string(),
        },
        ReviewRoundState::Failed(state) => StudioTaskReviewState::Failed {
            reviewer_agent_id: state.reviewer_thread_id().map(str::to_string),
            error: state.error().to_string(),
            summary: state.summary().to_string(),
        },
        ReviewRoundState::Cancelled(state) => StudioTaskReviewState::Cancelled {
            reviewer_agent_id: state.reviewer_thread_id().map(str::to_string),
            reason: state.reason().to_string(),
            summary: state.summary().to_string(),
        },
    }
}
