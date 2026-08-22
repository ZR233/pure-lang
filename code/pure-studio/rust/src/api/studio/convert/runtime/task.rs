//! Task runtime 聚合与状态机桥接。

use crate::api::studio::types::*;
use pl_studio_runtime::*;

pub(crate) fn bridge_task_runtime(
    task: pl_studio_runtime::StudioTaskRuntime,
) -> BridgeTaskRuntimeDto {
    BridgeTaskRuntimeDto {
        run_id: task.run_id,
        state: bridge_task_state(task.state),
        revision: task.revision,
        integrated_review_gate: match task.integrated_review_gate {
            pl_studio_runtime::StudioIntegratedReviewGate::Required { reason } => {
                BridgeIntegratedReviewGateDto::Required { reason }
            }
            pl_studio_runtime::StudioIntegratedReviewGate::SatisfiedByReview {
                review_round_id,
                reviewed_head,
            } => BridgeIntegratedReviewGateDto::SatisfiedByReview {
                review_round_id,
                reviewed_head,
            },
            pl_studio_runtime::StudioIntegratedReviewGate::NotRequiredNoDelivery => {
                BridgeIntegratedReviewGateDto::NotRequiredNoDelivery
            }
            pl_studio_runtime::StudioIntegratedReviewGate::NotRequiredSingleExecutorEquivalent {
                work_unit_id,
                completion_revision,
                merge_record_id,
            } => BridgeIntegratedReviewGateDto::NotRequiredSingleExecutorEquivalent {
                work_unit_id,
                completion_revision,
                merge_record_id,
            },
        },
        failures: task.failures.into_iter().map(bridge_task_failure).collect(),
        terminal_failure: task.terminal_failure.map(bridge_task_failure),
        work_units: task
            .work_units
            .into_iter()
            .map(|unit| BridgeTaskWorkUnitDto {
                id: unit.id,
                title: unit.title,
                state: bridge_work_unit_state(unit.state),
                worktree_path: unit.worktree_path,
                branch: unit.branch,
                agent_id: unit.agent_id,
                budget_slice_limit: unit.budget_slice_limit,
                executor_progress_revision: unit.executor_progress_revision,
                blueprint_fingerprint: unit.blueprint_fingerprint,
                objective: unit.objective,
                implementation_step_count: unit.implementation_step_count,
                acceptance_criterion_count: unit.acceptance_criterion_count,
                verification_count: unit.verification_count,
            })
            .collect(),
        completions: task
            .completions
            .into_iter()
            .map(|completion| BridgeTaskCompletionDto {
                id: completion.id,
                work_unit_id: completion.work_unit_id,
                executor_agent_id: completion.executor_agent_id,
                revision: completion.revision,
                kind: completion.kind,
                status: completion.status,
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
        merges: task
            .merges
            .into_iter()
            .map(|merge| BridgeTaskMergeDto {
                id: merge.id,
                work_unit_id: merge.work_unit_id,
                completion_id: merge.completion_id,
                completion_revision: merge.completion_revision,
                executor_agent_id: merge.executor_agent_id,
                expected_previous_head: merge.expected_previous_head,
                resulting_head: merge.resulting_head,
                delivery_head: merge.delivery_head,
                method: merge.method,
                summary: merge.summary,
                cleanup_status: merge.cleanup_status,
                cleanup_detail: merge.cleanup_detail,
                created_at: merge.created_at,
                updated_at: merge.updated_at,
            })
            .collect(),
        reviews: task
            .reviews
            .into_iter()
            .map(|review| BridgeTaskReviewDto {
                id: review.id,
                round: review.round,
                scope: review.scope,
                work_unit_id: review.work_unit_id,
                completion_id: review.completion_id,
                completion_revision: review.completion_revision,
                reviewed_head: review.reviewed_head,
                state: bridge_review_state(review.state),
                requested_by_call_id: review.requested_by_call_id,
                reviewer_agent_id: review.reviewer_agent_id,
                design_references: review
                    .design_references
                    .into_iter()
                    .map(|reference| BridgeTaskDesignReferenceDto {
                        path: reference.path,
                        section: reference.section,
                    })
                    .collect(),
                findings: review
                    .findings
                    .into_iter()
                    .map(|finding| BridgeTaskReviewFindingDto {
                        severity: finding.severity,
                        title: finding.title,
                        body: finding.body,
                        recommendation: finding.recommendation,
                        path: finding.path,
                        line: finding.line,
                        design_references: finding
                            .design_references
                            .into_iter()
                            .map(|reference| BridgeTaskDesignReferenceDto {
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
    }
}

fn bridge_task_state(state: StudioTaskState) -> BridgeTaskState {
    match state {
        StudioTaskState::DesignUpdating(data) => {
            BridgeTaskState::DesignUpdating(bridge_task_state_data(data))
        }
        StudioTaskState::Implementing(data) => {
            BridgeTaskState::Implementing(bridge_task_state_data(data))
        }
        StudioTaskState::Merging(data) => BridgeTaskState::Merging(bridge_task_state_data(data)),
        StudioTaskState::Reviewing(data) => {
            BridgeTaskState::Reviewing(bridge_task_state_data(data))
        }
        StudioTaskState::Reworking(data) => {
            BridgeTaskState::Reworking(bridge_task_state_data(data))
        }
        StudioTaskState::Stopping(data) => BridgeTaskState::Stopping(bridge_task_state_data(data)),
        StudioTaskState::Blocked(data) => BridgeTaskState::Blocked(bridge_task_state_data(data)),
        StudioTaskState::Completed(data) => {
            BridgeTaskState::Completed(bridge_task_state_data(data))
        }
        StudioTaskState::Failed(data) => BridgeTaskState::Failed(bridge_task_state_data(data)),
        StudioTaskState::Cancelled(data) => {
            BridgeTaskState::Cancelled(bridge_task_state_data(data))
        }
    }
}

fn bridge_task_state_data(data: StudioTaskStateData) -> BridgeTaskStateData {
    BridgeTaskStateData {
        generation: data.generation,
        status_message: data.status_message,
        finalized_design: data.finalized_design.map(|design| BridgeFinalizedDesign {
            summary: design.summary,
        }),
        stop_request: data.stop_request.map(|request| BridgeTaskStopRequest {
            origin: request.origin,
            reason: request.reason,
            requested_at: request.requested_at,
        }),
        review_target: data.review_target.map(|target| match target {
            StudioTaskReviewTarget::Delivery {
                work_unit_id,
                completion_id,
                completion_revision,
                reviewed_head,
            } => BridgeTaskReviewTarget::Delivery {
                work_unit_id,
                completion_id,
                completion_revision,
                reviewed_head,
            },
            StudioTaskReviewTarget::Integration { reviewed_head } => {
                BridgeTaskReviewTarget::Integration { reviewed_head }
            }
        }),
        blocked_recovery: data.blocked_recovery.map(|recovery| match recovery {
            StudioBlockedRecovery::RetryMerge => BridgeBlockedRecovery::RetryMerge,
            StudioBlockedRecovery::ResumeRework => BridgeBlockedRecovery::ResumeRework,
            StudioBlockedRecovery::ManualOnly => BridgeBlockedRecovery::ManualOnly,
        }),
        failure_id: data.failure_id,
    }
}

fn bridge_work_unit_state(state: StudioTaskWorkUnitState) -> BridgeTaskWorkUnitState {
    match state {
        StudioTaskWorkUnitState::Pending(progress) => {
            BridgeTaskWorkUnitState::Pending(bridge_work_unit_progress(progress))
        }
        StudioTaskWorkUnitState::Running(state) => {
            BridgeTaskWorkUnitState::Running(BridgeRunningWorkUnit {
                execution: match state.execution {
                    StudioRunningExecution::Running => BridgeRunningExecution::Running,
                    StudioRunningExecution::BudgetLimited => BridgeRunningExecution::BudgetLimited,
                },
                progress: bridge_work_unit_progress(state.progress),
            })
        }
        StudioTaskWorkUnitState::AwaitingCompletion(state) => {
            BridgeTaskWorkUnitState::AwaitingCompletion(BridgeAwaitingWorkUnit {
                execution: match state.execution {
                    StudioAwaitingExecution::Completed => BridgeAwaitingExecution::Completed,
                    StudioAwaitingExecution::Failed => BridgeAwaitingExecution::Failed,
                    StudioAwaitingExecution::Cancelled => BridgeAwaitingExecution::Cancelled,
                },
                progress: bridge_work_unit_progress(state.progress),
            })
        }
        StudioTaskWorkUnitState::ReadyForReview(progress) => {
            BridgeTaskWorkUnitState::ReadyForReview(bridge_work_unit_progress(progress))
        }
        StudioTaskWorkUnitState::Reviewing(progress) => {
            BridgeTaskWorkUnitState::Reviewing(bridge_work_unit_progress(progress))
        }
        StudioTaskWorkUnitState::ChangesRequested(progress) => {
            BridgeTaskWorkUnitState::ChangesRequested(bridge_work_unit_progress(progress))
        }
        StudioTaskWorkUnitState::Approved(progress) => {
            BridgeTaskWorkUnitState::Approved(bridge_work_unit_progress(progress))
        }
        StudioTaskWorkUnitState::Merged(progress) => {
            BridgeTaskWorkUnitState::Merged(bridge_work_unit_progress(progress))
        }
        StudioTaskWorkUnitState::NoDelivery(progress) => {
            BridgeTaskWorkUnitState::NoDelivery(bridge_work_unit_progress(progress))
        }
        StudioTaskWorkUnitState::NeedsAttention(progress) => {
            BridgeTaskWorkUnitState::NeedsAttention(bridge_work_unit_progress(progress))
        }
        StudioTaskWorkUnitState::Failed(progress) => {
            BridgeTaskWorkUnitState::Failed(bridge_work_unit_progress(progress))
        }
        StudioTaskWorkUnitState::Cancelled(progress) => {
            BridgeTaskWorkUnitState::Cancelled(bridge_work_unit_progress(progress))
        }
    }
}

fn bridge_work_unit_progress(progress: StudioTaskWorkUnitProgress) -> BridgeTaskWorkUnitProgress {
    BridgeTaskWorkUnitProgress {
        worktree_disposition: match progress.worktree_disposition {
            StudioTaskWorktreeDisposition::Protect => BridgeTaskWorktreeDisposition::Protect,
            StudioTaskWorktreeDisposition::CleanupRequested => {
                BridgeTaskWorktreeDisposition::CleanupRequested
            }
        },
        execution_summary: progress.execution_summary,
        execution_error: progress.execution_error,
        budget_limit: progress.budget_limit.map(|limit| BridgeBudgetLimitDto {
            kind: limit.kind,
            usage: BridgeBudgetUsageDto {
                model_steps: limit.usage.model_steps,
                tool_calls: limit.usage.tool_calls,
                wait_calls: limit.usage.wait_calls,
                elapsed_ms: limit.usage.elapsed_ms,
            },
        }),
        budget_slice_count: progress.budget_slice_count,
        continuation_state: match progress.continuation_state {
            StudioExecutorContinuationState::None => BridgeExecutorContinuationState::None,
            StudioExecutorContinuationState::PendingStart => {
                BridgeExecutorContinuationState::PendingStart
            }
            StudioExecutorContinuationState::Compacting => {
                BridgeExecutorContinuationState::Compacting
            }
            StudioExecutorContinuationState::PlannerWakePending => {
                BridgeExecutorContinuationState::PlannerWakePending
            }
            StudioExecutorContinuationState::NeedsAttention => {
                BridgeExecutorContinuationState::NeedsAttention
            }
        },
        continuation_source_turn_id: progress.continuation_source_turn_id,
        continuation_revision: progress.continuation_revision,
    }
}

fn bridge_review_state(state: StudioTaskReviewState) -> BridgeTaskReviewState {
    match state {
        StudioTaskReviewState::Pending { reviewer } => BridgeTaskReviewState::Pending {
            reviewer: match reviewer {
                StudioPendingReviewerState::Queued => BridgePendingReviewerState::Queued,
                StudioPendingReviewerState::Running => BridgePendingReviewerState::Running,
            },
        },
        StudioTaskReviewState::Pass { summary } => BridgeTaskReviewState::Pass { summary },
        StudioTaskReviewState::ChangesRequired { summary } => {
            BridgeTaskReviewState::ChangesRequired { summary }
        }
        StudioTaskReviewState::Blocked { summary } => BridgeTaskReviewState::Blocked { summary },
        StudioTaskReviewState::Failed {
            reviewer,
            error,
            summary,
        } => BridgeTaskReviewState::Failed {
            reviewer: match reviewer {
                StudioFailedReviewerState::Failed => BridgeFailedReviewerState::Failed,
                StudioFailedReviewerState::Cancelled => BridgeFailedReviewerState::Cancelled,
            },
            error,
            summary,
        },
    }
}

fn bridge_task_failure(
    failure: pl_studio_runtime::StudioTaskFailureRuntime,
) -> BridgeTaskFailureDto {
    BridgeTaskFailureDto {
        id: failure.id,
        source_thread_id: failure.source_thread_id,
        source_turn_id: failure.source_turn_id,
        source_agent_id: failure.source_agent_id,
        source_role: failure.source_role,
        work_unit_id: failure.work_unit_id,
        review_round_id: failure.review_round_id,
        disposition: failure.disposition,
        category: format!("{:?}", failure.failure.category).to_ascii_lowercase(),
        provider_kind: failure
            .failure
            .provider_kind
            .map(|kind| format!("{kind:?}").to_ascii_lowercase()),
        code: failure.failure.code,
        http_status: failure.failure.http_status,
        message: failure.failure.message,
        retryable: failure.failure.retry.is_retryable(),
        resolved_at: failure.resolved_at,
        created_at: failure.created_at,
    }
}
