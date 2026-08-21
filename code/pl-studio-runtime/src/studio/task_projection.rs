use anyhow::Result;

use crate::{
    StudioAwaitingExecution, StudioAwaitingWorkUnit, StudioBlockedRecovery,
    StudioBudgetLimitRuntime, StudioBudgetUsageRuntime, StudioExecutorContinuationState,
    StudioFailedReviewerState, StudioFinalizedDesign, StudioIntegratedReviewGate,
    StudioPendingReviewerState, StudioRunningExecution, StudioRunningWorkUnit,
    StudioTaskCompletionRuntime, StudioTaskDesignReferenceRuntime, StudioTaskFailureRuntime,
    StudioTaskMergeRuntime, StudioTaskReviewFindingRuntime, StudioTaskReviewRuntime,
    StudioTaskReviewState, StudioTaskReviewTarget, StudioTaskRuntime, StudioTaskState,
    StudioTaskStateData, StudioTaskStopRequest, StudioTaskWorkUnitProgress,
    StudioTaskWorkUnitRuntime, StudioTaskWorkUnitState, StudioTaskWorktreeDisposition,
};

use super::{
    StudioStore,
    task_coordinator::{
        AwaitingExecution, BlockedRecovery, DesignProgress, ExecutorContinuationState,
        FailedReviewerState, MergeRecord, PendingReviewerState, ReviewRoundRecord,
        ReviewRoundState, ReviewTarget, RunningExecution, TaskRun, TaskRunState,
        TaskWorktreeDisposition, WorkCompletionRecord, WorkUnitProgress, WorkUnitState,
    },
};

pub(crate) async fn load_task_runtime(
    store: &StudioStore,
    root_thread_id: &str,
) -> Result<Option<StudioTaskRuntime>> {
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
        let handoff = store
            .read_work_unit_handoff(&unit.id)
            .await
            .ok()
            .flatten()
            .map(|(_, handoff)| handoff);
        work_unit_runtimes.push(StudioTaskWorkUnitRuntime {
            id: unit.id.clone(),
            title: unit.title.clone(),
            state: studio_work_unit_state(&unit.state),
            worktree_path: unit.worktree_path.clone(),
            branch: unit.branch.clone(),
            agent_id: unit.executor_thread_id.clone(),
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
    let failures = store.list_task_failures(&run.id).await?;
    studio_task_runtime(
        run,
        work_unit_runtimes,
        completions,
        merges,
        reviews,
        failures,
        integrated_review_gate,
    )
    .map(Some)
}

fn studio_task_runtime(
    run: TaskRun,
    work_units: Vec<StudioTaskWorkUnitRuntime>,
    completions: Vec<WorkCompletionRecord>,
    merges: Vec<MergeRecord>,
    reviews: Vec<ReviewRoundRecord>,
    failures: Vec<super::task_coordinator::TaskFailureRecord>,
    integrated_review_gate: StudioIntegratedReviewGate,
) -> Result<StudioTaskRuntime> {
    let terminal_failure_id = run.terminal_failure_id();
    let all_failures = failures
        .into_iter()
        .map(|failure| StudioTaskFailureRuntime {
            id: failure.id,
            source_thread_id: failure.source_thread_id,
            source_turn_id: failure.source_turn_id,
            source_agent_id: failure.source_agent_id,
            source_role: failure.source_role,
            work_unit_id: failure.work_unit_id,
            review_round_id: failure.review_round_id,
            disposition: failure.disposition.as_str().to_string(),
            failure: failure.failure,
            resolved_at: failure.resolved_at,
            created_at: failure.created_at,
        })
        .collect::<Vec<_>>();
    let terminal_failure = terminal_failure_id
        .and_then(|id| all_failures.iter().find(|failure| failure.id == id))
        .cloned();
    let failures = all_failures
        .into_iter()
        .filter(|failure| failure.resolved_at.is_none())
        .collect();
    Ok(StudioTaskRuntime {
        run_id: run.id.clone(),
        state: studio_task_state(&run)?,
        branch: run.branch.clone(),
        expected_head: run.expected_head.clone(),
        revision: run.revision,
        integrated_review_gate,
        failures,
        terminal_failure,
        work_units,
        completions: completions
            .into_iter()
            .map(|completion| StudioTaskCompletionRuntime {
                id: completion.id,
                work_unit_id: completion.work_unit_id,
                executor_agent_id: completion.executor_agent_id,
                revision: completion.revision,
                kind: completion.kind.as_str().to_string(),
                status: completion.status.as_str().to_string(),
                base_commit: completion.base_commit,
                head_commit: completion.head_commit,
                changed_files: completion.changed_files,
                verification_summary: completion.verification_summary,
                worktree_path: completion.worktree_path,
                branch: completion.branch,
                created_at: completion.created_at,
                updated_at: completion.updated_at,
            })
            .collect(),
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
                method: merge.method.as_str().to_string(),
                summary: merge.summary,
                cleanup_status: merge.cleanup.status,
                cleanup_detail: merge.cleanup.detail,
                created_at: merge.created_at,
                updated_at: merge.updated_at,
            })
            .collect(),
        reviews: reviews
            .into_iter()
            .map(|review| StudioTaskReviewRuntime {
                id: review.id,
                round: review.round,
                scope: review.scope.as_str().to_string(),
                work_unit_id: review.work_unit_id,
                completion_id: review.completion_id,
                completion_revision: review.completion_revision,
                reviewed_head: review.reviewed_head,
                state: studio_review_state(&review.state),
                requested_by_call_id: review.requested_by_call_id,
                reviewer_agent_id: review.reviewer_thread_id,
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

fn studio_task_state(run: &TaskRun) -> Result<StudioTaskState> {
    let data = StudioTaskStateData {
        generation: run.generation(),
        status_message: run.status_message().map(str::to_string),
        finalized_design: match run.state.design() {
            DesignProgress::Updating => None,
            DesignProgress::Finalized(design) => Some(StudioFinalizedDesign {
                head: design.head.clone(),
                commit: design.commit.clone(),
                summary: design.summary.clone(),
                fingerprint: serde_json::to_string(&design.fingerprint)?,
            }),
        },
        stop_request: run
            .state
            .stop_request()
            .map(|request| StudioTaskStopRequest {
                origin: request.origin.as_str().to_string(),
                reason: request.reason.as_str().to_string(),
                requested_at: request.requested_at,
            }),
        review_target: match &run.state {
            TaskRunState::Reviewing(state) => Some(studio_review_target(state.target())),
            _ => None,
        },
        blocked_recovery: match &run.state {
            TaskRunState::Blocked(state) => Some(match state.recovery() {
                BlockedRecovery::RetryMerge => StudioBlockedRecovery::RetryMerge,
                BlockedRecovery::ResumeRework => StudioBlockedRecovery::ResumeRework,
                BlockedRecovery::ManualOnly => StudioBlockedRecovery::ManualOnly,
            }),
            _ => None,
        },
        failure_id: run.terminal_failure_id().map(str::to_string),
    };
    Ok(match &run.state {
        TaskRunState::DesignUpdating(_) => StudioTaskState::DesignUpdating(data),
        TaskRunState::Implementing(_) => StudioTaskState::Implementing(data),
        TaskRunState::Merging(_) => StudioTaskState::Merging(data),
        TaskRunState::Reviewing(_) => StudioTaskState::Reviewing(data),
        TaskRunState::Reworking(_) => StudioTaskState::Reworking(data),
        TaskRunState::Stopping(_) => StudioTaskState::Stopping(data),
        TaskRunState::Blocked(_) => StudioTaskState::Blocked(data),
        TaskRunState::Completed(_) => StudioTaskState::Completed(data),
        TaskRunState::Failed(_) => StudioTaskState::Failed(data),
        TaskRunState::Cancelled(_) => StudioTaskState::Cancelled(data),
    })
}

fn studio_review_target(target: &ReviewTarget) -> StudioTaskReviewTarget {
    match target {
        ReviewTarget::Delivery {
            work_unit_id,
            completion_id,
            completion_revision,
            reviewed_head,
        } => StudioTaskReviewTarget::Delivery {
            work_unit_id: work_unit_id.clone(),
            completion_id: completion_id.clone(),
            completion_revision: *completion_revision,
            reviewed_head: reviewed_head.clone(),
        },
        ReviewTarget::Integration { reviewed_head } => StudioTaskReviewTarget::Integration {
            reviewed_head: reviewed_head.clone(),
        },
    }
}

fn studio_work_unit_state(state: &WorkUnitState) -> StudioTaskWorkUnitState {
    match state {
        WorkUnitState::Pending(progress) => {
            StudioTaskWorkUnitState::Pending(studio_work_unit_progress(progress))
        }
        WorkUnitState::Running(state) => StudioTaskWorkUnitState::Running(StudioRunningWorkUnit {
            execution: match state.execution {
                RunningExecution::Running => StudioRunningExecution::Running,
                RunningExecution::BudgetLimited => StudioRunningExecution::BudgetLimited,
            },
            progress: studio_work_unit_progress(&state.progress),
        }),
        WorkUnitState::AwaitingCompletion(state) => {
            StudioTaskWorkUnitState::AwaitingCompletion(StudioAwaitingWorkUnit {
                execution: match state.execution {
                    AwaitingExecution::Completed => StudioAwaitingExecution::Completed,
                    AwaitingExecution::Failed => StudioAwaitingExecution::Failed,
                    AwaitingExecution::Cancelled => StudioAwaitingExecution::Cancelled,
                },
                progress: studio_work_unit_progress(&state.progress),
            })
        }
        WorkUnitState::ReadyForReview(progress) => {
            StudioTaskWorkUnitState::ReadyForReview(studio_work_unit_progress(progress))
        }
        WorkUnitState::Reviewing(progress) => {
            StudioTaskWorkUnitState::Reviewing(studio_work_unit_progress(progress))
        }
        WorkUnitState::ChangesRequested(progress) => {
            StudioTaskWorkUnitState::ChangesRequested(studio_work_unit_progress(progress))
        }
        WorkUnitState::Approved(progress) => {
            StudioTaskWorkUnitState::Approved(studio_work_unit_progress(progress))
        }
        WorkUnitState::Merged(progress) => {
            StudioTaskWorkUnitState::Merged(studio_work_unit_progress(progress))
        }
        WorkUnitState::NoDelivery(progress) => {
            StudioTaskWorkUnitState::NoDelivery(studio_work_unit_progress(progress))
        }
        WorkUnitState::NeedsAttention(progress) => {
            StudioTaskWorkUnitState::NeedsAttention(studio_work_unit_progress(progress))
        }
        WorkUnitState::Failed(progress) => {
            StudioTaskWorkUnitState::Failed(studio_work_unit_progress(progress))
        }
        WorkUnitState::Cancelled(progress) => {
            StudioTaskWorkUnitState::Cancelled(studio_work_unit_progress(progress))
        }
    }
}

fn studio_work_unit_progress(progress: &WorkUnitProgress) -> StudioTaskWorkUnitProgress {
    StudioTaskWorkUnitProgress {
        worktree_disposition: match progress.worktree_disposition {
            TaskWorktreeDisposition::Protect => StudioTaskWorktreeDisposition::Protect,
            TaskWorktreeDisposition::CleanupRequested => {
                StudioTaskWorktreeDisposition::CleanupRequested
            }
        },
        execution_summary: progress.execution_summary.clone(),
        execution_error: progress.execution_error.clone(),
        budget_limit: progress
            .budget_limit
            .as_ref()
            .map(|limit| StudioBudgetLimitRuntime {
                kind: limit.kind.as_str().to_string(),
                usage: StudioBudgetUsageRuntime {
                    model_steps: limit.usage.model_steps,
                    tool_calls: limit.usage.tool_calls,
                    wait_calls: limit.usage.wait_calls,
                    elapsed_ms: limit.usage.elapsed_ms,
                },
            }),
        budget_slice_count: progress.budget_slice_count,
        continuation_state: match progress.continuation_state {
            ExecutorContinuationState::None => StudioExecutorContinuationState::None,
            ExecutorContinuationState::PendingStart => {
                StudioExecutorContinuationState::PendingStart
            }
            ExecutorContinuationState::Compacting => StudioExecutorContinuationState::Compacting,
            ExecutorContinuationState::PlannerWakePending => {
                StudioExecutorContinuationState::PlannerWakePending
            }
            ExecutorContinuationState::NeedsAttention => {
                StudioExecutorContinuationState::NeedsAttention
            }
        },
        continuation_source_turn_id: progress.continuation_source_turn_id.clone(),
        continuation_revision: progress.continuation_revision,
    }
}

fn studio_review_state(state: &ReviewRoundState) -> StudioTaskReviewState {
    match state {
        ReviewRoundState::Pending(state) => StudioTaskReviewState::Pending {
            reviewer: match state.reviewer {
                PendingReviewerState::Queued => StudioPendingReviewerState::Queued,
                PendingReviewerState::Running => StudioPendingReviewerState::Running,
            },
        },
        ReviewRoundState::Pass(state) => StudioTaskReviewState::Pass {
            summary: state.summary.clone(),
        },
        ReviewRoundState::ChangesRequired(state) => StudioTaskReviewState::ChangesRequired {
            summary: state.summary.clone(),
        },
        ReviewRoundState::Blocked(state) => StudioTaskReviewState::Blocked {
            summary: state.summary.clone(),
        },
        ReviewRoundState::Failed(state) => StudioTaskReviewState::Failed {
            reviewer: match state.reviewer {
                FailedReviewerState::Failed => StudioFailedReviewerState::Failed,
                FailedReviewerState::Cancelled => StudioFailedReviewerState::Cancelled,
            },
            error: state.error.clone(),
            summary: state.summary.clone(),
        },
    }
}
