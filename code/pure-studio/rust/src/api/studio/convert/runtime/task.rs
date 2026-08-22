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
                content: bridge_completion_content(completion.content),
                state: bridge_completion_state(completion.state),
                state_revision: completion.state_revision,
                base_commit: completion.base_commit,
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
                method: match merge.method {
                    StudioMergeMethod::Merge => BridgeMergeMethod::Merge,
                    StudioMergeMethod::CherryPick => BridgeMergeMethod::CherryPick,
                    StudioMergeMethod::Squash => BridgeMergeMethod::Squash,
                    StudioMergeMethod::Rebase => BridgeMergeMethod::Rebase,
                    StudioMergeMethod::Manual => BridgeMergeMethod::Manual,
                },
                summary: merge.summary,
                cleanup: match merge.cleanup {
                    StudioMergeCleanupState::Pending => BridgeMergeCleanupState::Pending,
                    StudioMergeCleanupState::Deferred => BridgeMergeCleanupState::Deferred,
                    StudioMergeCleanupState::Attempting { operation_id, started_at } => {
                        BridgeMergeCleanupState::Attempting { operation_id, started_at }
                    }
                    StudioMergeCleanupState::Discarded { operation_id, completed_at } => {
                        BridgeMergeCleanupState::Discarded { operation_id, completed_at }
                    }
                    StudioMergeCleanupState::AlreadyAbsent { operation_id, completed_at } => {
                        BridgeMergeCleanupState::AlreadyAbsent { operation_id, completed_at }
                    }
                    StudioMergeCleanupState::Failed { operation_id, failed_at, detail } => {
                        BridgeMergeCleanupState::Failed { operation_id, failed_at, detail }
                    }
                },
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
                scope: match review.scope {
                    StudioReviewScope::Delivery => BridgeReviewScope::Delivery,
                    StudioReviewScope::Integrated => BridgeReviewScope::Integrated,
                },
                work_unit_id: review.work_unit_id,
                completion_id: review.completion_id,
                completion_revision: review.completion_revision,
                reviewed_head: review.reviewed_head,
                state: bridge_review_state(review.state),
                requested_by_call_id: review.requested_by_call_id,
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

fn bridge_completion_content(content: StudioTaskCompletionContent) -> BridgeTaskCompletionContent {
    match content {
        StudioTaskCompletionContent::Delivery(value) => {
            BridgeTaskCompletionContent::Delivery(BridgeTaskDeliveryCompletion {
                head_commit: value.head_commit,
                changed_files: value.changed_files,
            })
        }
        StudioTaskCompletionContent::NoDelivery(_) => {
            BridgeTaskCompletionContent::NoDelivery(BridgeTaskNoDeliveryCompletion {})
        }
    }
}

fn bridge_completion_state(state: StudioTaskCompletionState) -> BridgeTaskCompletionState {
    match state {
        StudioTaskCompletionState::ReadyForReview(_) => {
            BridgeTaskCompletionState::ReadyForReview(BridgeReadyForReviewCompletion {})
        }
        StudioTaskCompletionState::ChangesRequired(value) => {
            BridgeTaskCompletionState::ChangesRequired(BridgeReviewedCompletion {
                review_round_id: value.review_round_id,
                decided_at: value.decided_at,
            })
        }
        StudioTaskCompletionState::Approved(value) => {
            BridgeTaskCompletionState::Approved(BridgeReviewedCompletion {
                review_round_id: value.review_round_id,
                decided_at: value.decided_at,
            })
        }
    }
}

fn bridge_task_state(state: StudioTaskState) -> BridgeTaskState {
    match state {
        StudioTaskState::DesignUpdating(state) => {
            BridgeTaskState::DesignUpdating(BridgeDesignUpdatingTaskState {
                generation: state.generation,
            })
        }
        StudioTaskState::Implementing(state) => {
            BridgeTaskState::Implementing(BridgeImplementingTaskState {
                generation: state.generation,
                design: bridge_finalized_design(state.design),
            })
        }
        StudioTaskState::Merging(state) => BridgeTaskState::Merging(BridgeMergingTaskState {
            generation: state.generation,
            status_message: state.status_message,
            design: bridge_finalized_design(state.design),
        }),
        StudioTaskState::Reviewing(state) => BridgeTaskState::Reviewing(BridgeReviewingTaskState {
            generation: state.generation,
            status_message: state.status_message,
            design: bridge_finalized_design(state.design),
            target: bridge_review_target(state.target),
        }),
        StudioTaskState::Reworking(state) => BridgeTaskState::Reworking(BridgeReworkingTaskState {
            generation: state.generation,
            status_message: state.status_message,
            design: bridge_finalized_design(state.design),
        }),
        StudioTaskState::Stopping(state) => BridgeTaskState::Stopping(BridgeStoppingTaskState {
            generation: state.generation,
            status_message: state.status_message,
            design: bridge_design_progress(state.design),
            request: bridge_stop_request(state.request),
        }),
        StudioTaskState::Blocked(state) => BridgeTaskState::Blocked(BridgeBlockedTaskState {
            generation: state.generation,
            message: state.message,
            design: bridge_design_progress(state.design),
            recovery: bridge_blocked_recovery(state.recovery),
        }),
        StudioTaskState::Completed(state) => BridgeTaskState::Completed(BridgeCompletedTaskState {
            generation: state.generation,
            design: bridge_finalized_design(state.design),
        }),
        StudioTaskState::Failed(state) => BridgeTaskState::Failed(BridgeFailedTaskState {
            generation: state.generation,
            message: state.message,
            design: bridge_design_progress(state.design),
            failure_id: state.failure_id,
        }),
        StudioTaskState::Cancelled(state) => BridgeTaskState::Cancelled(BridgeCancelledTaskState {
            generation: state.generation,
            message: state.message,
            design: bridge_design_progress(state.design),
            request: state.request.map(bridge_stop_request),
        }),
    }
}

fn bridge_finalized_design(design: StudioFinalizedDesign) -> BridgeFinalizedDesign {
    BridgeFinalizedDesign {
        summary: design.summary,
    }
}

fn bridge_design_progress(progress: StudioDesignProgress) -> BridgeDesignProgress {
    match progress {
        StudioDesignProgress::Updating(_) => {
            BridgeDesignProgress::Updating(BridgeUpdatingDesign {})
        }
        StudioDesignProgress::Finalized(design) => {
            BridgeDesignProgress::Finalized(bridge_finalized_design(design))
        }
    }
}

fn bridge_stop_request(request: StudioTaskStopRequest) -> BridgeTaskStopRequest {
    BridgeTaskStopRequest {
        origin: match request.origin {
            StudioTaskStopOrigin::UserRequest => BridgeTaskStopOrigin::UserRequest,
            StudioTaskStopOrigin::PlannerDecision => BridgeTaskStopOrigin::PlannerDecision,
            StudioTaskStopOrigin::RuntimeFailure => BridgeTaskStopOrigin::RuntimeFailure,
            StudioTaskStopOrigin::ApplicationShutdown => BridgeTaskStopOrigin::ApplicationShutdown,
        },
        reason: request.reason,
        requested_at: request.requested_at,
    }
}

fn bridge_review_target(target: StudioTaskReviewTarget) -> BridgeTaskReviewTarget {
    match target {
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
    }
}

fn bridge_blocked_recovery(recovery: StudioBlockedRecovery) -> BridgeBlockedRecovery {
    match recovery {
        StudioBlockedRecovery::RetryMerge => BridgeBlockedRecovery::RetryMerge,
        StudioBlockedRecovery::ResumeRework => BridgeBlockedRecovery::ResumeRework,
        StudioBlockedRecovery::ManualOnly => BridgeBlockedRecovery::ManualOnly,
    }
}

fn bridge_work_unit_state(state: StudioTaskWorkUnitState) -> BridgeTaskWorkUnitState {
    match state {
        StudioTaskWorkUnitState::Pending(_) => {
            BridgeTaskWorkUnitState::Pending(BridgePendingWorkUnit {})
        }
        StudioTaskWorkUnitState::Running(state) => {
            BridgeTaskWorkUnitState::Running(BridgeRunningWorkUnit {
                activity: match state.activity {
                    StudioRunningWorkUnitActivity::Allocated => {
                        BridgeRunningWorkUnitActivity::Allocated
                    }
                    StudioRunningWorkUnitActivity::Active { turn_id } => {
                        BridgeRunningWorkUnitActivity::Active { turn_id }
                    }
                },
                continuation: bridge_executor_continuation(state.continuation),
            })
        }
        StudioTaskWorkUnitState::WaitingReview(state) => {
            BridgeTaskWorkUnitState::WaitingReview(BridgeWaitingReviewWorkUnit {
                phase: match state.phase {
                    StudioWaitingReviewPhase::AwaitingReport {
                        outcome,
                        continuation,
                    } => BridgeWaitingReviewPhase::AwaitingReport {
                        outcome: match outcome {
                            StudioExecutorTerminalOutcome::Completed {
                                source_turn_id,
                                detail,
                            } => BridgeExecutorTerminalOutcome::Completed {
                                source_turn_id,
                                detail,
                            },
                            StudioExecutorTerminalOutcome::Failed {
                                source_turn_id,
                                detail,
                            } => BridgeExecutorTerminalOutcome::Failed {
                                source_turn_id,
                                detail,
                            },
                        },
                        continuation: bridge_executor_continuation(continuation),
                    },
                    StudioWaitingReviewPhase::Ready {
                        completion_id,
                        completion_revision,
                        verification_summary,
                    } => BridgeWaitingReviewPhase::Ready {
                        completion_id,
                        completion_revision,
                        verification_summary,
                    },
                    StudioWaitingReviewPhase::Reviewing {
                        completion_id,
                        completion_revision,
                        review_round_id,
                        verification_summary,
                    } => BridgeWaitingReviewPhase::Reviewing {
                        completion_id,
                        completion_revision,
                        review_round_id,
                        verification_summary,
                    },
                },
            })
        }
        StudioTaskWorkUnitState::ReviewPassed(state) => {
            BridgeTaskWorkUnitState::ReviewPassed(BridgeReviewPassedWorkUnit {
                completion_id: state.completion_id,
                completion_revision: state.completion_revision,
                review_round_id: state.review_round_id,
                outcome: match state.outcome {
                    StudioReviewPassedOutcome::Delivery => BridgeReviewPassedOutcome::Delivery,
                    StudioReviewPassedOutcome::NoDelivery => BridgeReviewPassedOutcome::NoDelivery,
                },
                verification_summary: state.verification_summary,
            })
        }
        StudioTaskWorkUnitState::ChangesRequired(state) => {
            BridgeTaskWorkUnitState::ChangesRequired(BridgeChangesRequiredWorkUnit {
                completion_id: state.completion_id,
                completion_revision: state.completion_revision,
                review_round_id: state.review_round_id,
                continuation_revision: state.continuation_revision,
                slice_count: state.slice_count,
            })
        }
        StudioTaskWorkUnitState::Paused(state) => {
            BridgeTaskWorkUnitState::Paused(BridgePausedWorkUnit {
                reason: match state.reason {
                    StudioWorkUnitPauseReason::Budget { limit } => {
                        BridgeWorkUnitPauseReason::Budget {
                            limit: bridge_budget_limit(limit),
                        }
                    }
                    StudioWorkUnitPauseReason::Operational {
                        operation_id,
                        detail,
                    } => BridgeWorkUnitPauseReason::Operational {
                        operation_id,
                        detail,
                    },
                },
                continuation: bridge_executor_continuation(state.continuation),
            })
        }
        StudioTaskWorkUnitState::Completed(state) => {
            BridgeTaskWorkUnitState::Completed(BridgeCompletedWorkUnit {
                outcome: match state.outcome {
                    StudioWorkUnitCompletionOutcome::Merged { merge_record_id } => {
                        BridgeWorkUnitCompletionOutcome::Merged { merge_record_id }
                    }
                    StudioWorkUnitCompletionOutcome::NoDelivery { completion_id } => {
                        BridgeWorkUnitCompletionOutcome::NoDelivery { completion_id }
                    }
                },
            })
        }
        StudioTaskWorkUnitState::Failed(state) => {
            BridgeTaskWorkUnitState::Failed(BridgeFailedWorkUnit {
                failure: match state.failure {
                    StudioWorkUnitFailure::Spawn { failure } => BridgeWorkUnitFailure::Spawn {
                        failure: Box::new(bridge_spawn_failure(*failure)),
                    },
                    StudioWorkUnitFailure::Execution {
                        operation_id,
                        detail,
                    } => BridgeWorkUnitFailure::Execution {
                        operation_id,
                        detail,
                    },
                },
                worktree_disposition: match state.worktree_disposition {
                    StudioTaskWorktreeDisposition::Protect => {
                        BridgeTaskWorktreeDisposition::Protect
                    }
                    StudioTaskWorktreeDisposition::CleanupRequested => {
                        BridgeTaskWorktreeDisposition::CleanupRequested
                    }
                },
            })
        }
        StudioTaskWorkUnitState::Cancelled(state) => {
            BridgeTaskWorkUnitState::Cancelled(BridgeCancelledWorkUnit {
                operation_id: state.operation_id,
                reason: state.reason,
                worktree_disposition: match state.worktree_disposition {
                    StudioTaskWorktreeDisposition::Protect => {
                        BridgeTaskWorktreeDisposition::Protect
                    }
                    StudioTaskWorktreeDisposition::CleanupRequested => {
                        BridgeTaskWorktreeDisposition::CleanupRequested
                    }
                },
            })
        }
    }
}

fn bridge_executor_continuation(
    state: StudioExecutorContinuationState,
) -> BridgeExecutorContinuationState {
    match state {
        StudioExecutorContinuationState::Idle {
            revision,
            slice_count,
        } => BridgeExecutorContinuationState::Idle {
            revision,
            slice_count,
        },
        StudioExecutorContinuationState::Compacting {
            revision,
            source_turn_id,
            slice_count,
        } => BridgeExecutorContinuationState::Compacting {
            revision,
            source_turn_id,
            slice_count,
        },
        StudioExecutorContinuationState::PendingStart {
            revision,
            source_turn_id,
            slice_count,
            limit,
        } => BridgeExecutorContinuationState::PendingStart {
            revision,
            source_turn_id,
            slice_count,
            limit: bridge_budget_limit(limit),
        },
        StudioExecutorContinuationState::PlannerWakePending {
            revision,
            source_turn_id,
            slice_count,
        } => BridgeExecutorContinuationState::PlannerWakePending {
            revision,
            source_turn_id,
            slice_count,
        },
        StudioExecutorContinuationState::NeedsAttention {
            revision,
            source_turn_id,
            slice_count,
            detail,
        } => BridgeExecutorContinuationState::NeedsAttention {
            revision,
            source_turn_id,
            slice_count,
            detail,
        },
    }
}

fn bridge_budget_limit(limit: StudioBudgetLimitRuntime) -> BridgeBudgetLimitDto {
    BridgeBudgetLimitDto {
        kind: match limit.kind {
            StudioBudgetLimitKind::ModelStep => BridgeBudgetLimitKind::ModelStep,
            StudioBudgetLimitKind::ToolCall => BridgeBudgetLimitKind::ToolCall,
            StudioBudgetLimitKind::Wait => BridgeBudgetLimitKind::Wait,
            StudioBudgetLimitKind::WallClock => BridgeBudgetLimitKind::WallClock,
            StudioBudgetLimitKind::AgentCount => BridgeBudgetLimitKind::AgentCount,
            StudioBudgetLimitKind::AgentDepth => BridgeBudgetLimitKind::AgentDepth,
            StudioBudgetLimitKind::Finalization => BridgeBudgetLimitKind::Finalization,
        },
        usage: BridgeBudgetUsageDto {
            model_steps: limit.usage.model_steps,
            tool_calls: limit.usage.tool_calls,
            wait_calls: limit.usage.wait_calls,
            elapsed_ms: limit.usage.elapsed_ms,
        },
    }
}

fn bridge_spawn_failure(failure: StudioTaskSpawnFailure) -> BridgeTaskSpawnFailure {
    BridgeTaskSpawnFailure {
        code: match failure.code {
            StudioTaskSpawnFailureCode::Allocation => BridgeTaskSpawnFailureCode::Allocation,
            StudioTaskSpawnFailureCode::WorktreeCreate => {
                BridgeTaskSpawnFailureCode::WorktreeCreate
            }
            StudioTaskSpawnFailureCode::ChildThreadCreate => {
                BridgeTaskSpawnFailureCode::ChildThreadCreate
            }
            StudioTaskSpawnFailureCode::AgentRegistration => {
                BridgeTaskSpawnFailureCode::AgentRegistration
            }
            StudioTaskSpawnFailureCode::Activation => BridgeTaskSpawnFailureCode::Activation,
        },
        phase: match failure.phase {
            StudioTaskSpawnFailurePhase::Allocation => BridgeTaskSpawnFailurePhase::Allocation,
            StudioTaskSpawnFailurePhase::WorktreeCreate => {
                BridgeTaskSpawnFailurePhase::WorktreeCreate
            }
            StudioTaskSpawnFailurePhase::ChildThreadCreate => {
                BridgeTaskSpawnFailurePhase::ChildThreadCreate
            }
            StudioTaskSpawnFailurePhase::AgentRegistration => {
                BridgeTaskSpawnFailurePhase::AgentRegistration
            }
            StudioTaskSpawnFailurePhase::Activation => BridgeTaskSpawnFailurePhase::Activation,
        },
        recoverable: failure.recoverable,
        message: failure.message,
        task_run_id: failure.task_run_id,
        work_unit_id: failure.work_unit_id,
        agent_id: failure.agent_id,
        resource: failure.resource.map(|resource| BridgeTaskSpawnResource {
            repo_root: resource.repo_root,
            path: resource.path,
            branch: resource.branch,
            base_ref: resource.base_ref,
        }),
        cause: BridgeWorktreeFailureCause {
            kind: match failure.cause.kind {
                StudioWorktreeFailureCauseKind::InvalidRepoRoot => {
                    BridgeWorktreeFailureCauseKind::InvalidRepoRoot
                }
                StudioWorktreeFailureCauseKind::UnsafeBranch => {
                    BridgeWorktreeFailureCauseKind::UnsafeBranch
                }
                StudioWorktreeFailureCauseKind::GitLaunchFailed => {
                    BridgeWorktreeFailureCauseKind::GitLaunchFailed
                }
                StudioWorktreeFailureCauseKind::GitTimedOut => {
                    BridgeWorktreeFailureCauseKind::GitTimedOut
                }
                StudioWorktreeFailureCauseKind::GitExited => {
                    BridgeWorktreeFailureCauseKind::GitExited
                }
                StudioWorktreeFailureCauseKind::GitStatusUnknown => {
                    BridgeWorktreeFailureCauseKind::GitStatusUnknown
                }
                StudioWorktreeFailureCauseKind::Io => BridgeWorktreeFailureCauseKind::Io,
                StudioWorktreeFailureCauseKind::Disabled => {
                    BridgeWorktreeFailureCauseKind::Disabled
                }
                StudioWorktreeFailureCauseKind::OperationAndCleanupFailed => {
                    BridgeWorktreeFailureCauseKind::OperationAndCleanupFailed
                }
            },
            message: failure.cause.message,
            args: failure.cause.args,
            exit_code: failure.cause.exit_code,
            stderr: failure.cause.stderr,
        },
        compensation: BridgeTaskSpawnCompensation {
            allocation: bridge_spawn_compensation(failure.compensation.allocation),
            worktree: bridge_spawn_compensation(failure.compensation.worktree),
            child_thread: bridge_spawn_compensation(failure.compensation.child_thread),
        },
        next_action: match failure.next_action {
            StudioTaskSpawnNextAction::RetryTaskSpawnExecutor => {
                BridgeTaskSpawnNextAction::RetryTaskSpawnExecutor
            }
            StudioTaskSpawnNextAction::RecoverWorktreeResources => {
                BridgeTaskSpawnNextAction::RecoverWorktreeResources
            }
        },
    }
}

fn bridge_spawn_compensation(
    state: StudioTaskSpawnCompensationState,
) -> BridgeTaskSpawnCompensationState {
    match state {
        StudioTaskSpawnCompensationState::NotCreated => {
            BridgeTaskSpawnCompensationState::NotCreated
        }
        StudioTaskSpawnCompensationState::MarkedFailed => {
            BridgeTaskSpawnCompensationState::MarkedFailed
        }
        StudioTaskSpawnCompensationState::Removed => BridgeTaskSpawnCompensationState::Removed,
        StudioTaskSpawnCompensationState::Faulted => BridgeTaskSpawnCompensationState::Faulted,
        StudioTaskSpawnCompensationState::CleanupFailed => {
            BridgeTaskSpawnCompensationState::CleanupFailed
        }
        StudioTaskSpawnCompensationState::Unknown => BridgeTaskSpawnCompensationState::Unknown,
    }
}

fn bridge_review_state(state: StudioTaskReviewState) -> BridgeTaskReviewState {
    match state {
        StudioTaskReviewState::PendingDispatch => BridgeTaskReviewState::PendingDispatch,
        StudioTaskReviewState::Dispatched { reviewer_agent_id } => {
            BridgeTaskReviewState::Dispatched { reviewer_agent_id }
        }
        StudioTaskReviewState::Running { reviewer_agent_id } => {
            BridgeTaskReviewState::Running { reviewer_agent_id }
        }
        StudioTaskReviewState::Passed {
            reviewer_agent_id,
            summary,
        } => BridgeTaskReviewState::Passed {
            reviewer_agent_id,
            summary,
        },
        StudioTaskReviewState::ChangesRequired {
            reviewer_agent_id,
            summary,
        } => BridgeTaskReviewState::ChangesRequired {
            reviewer_agent_id,
            summary,
        },
        StudioTaskReviewState::Blocked {
            reviewer_agent_id,
            summary,
        } => BridgeTaskReviewState::Blocked {
            reviewer_agent_id,
            summary,
        },
        StudioTaskReviewState::Failed {
            reviewer_agent_id,
            error,
            summary,
        } => BridgeTaskReviewState::Failed {
            reviewer_agent_id,
            error,
            summary,
        },
        StudioTaskReviewState::Cancelled {
            reviewer_agent_id,
            reason,
            summary,
        } => BridgeTaskReviewState::Cancelled {
            reviewer_agent_id,
            reason,
            summary,
        },
    }
}

fn bridge_task_failure(
    failure: pl_studio_runtime::StudioTaskFailureRuntime,
) -> BridgeTaskFailureDto {
    let state = match failure.state {
        StudioTaskFailureState::OpenRecoverable { failure } => {
            BridgeTaskFailureState::OpenRecoverable {
                failure: bridge_task_failure_detail(failure),
            }
        }
        StudioTaskFailureState::OpenFatal { failure } => BridgeTaskFailureState::OpenFatal {
            failure: bridge_task_failure_detail(failure),
        },
        StudioTaskFailureState::Resolved {
            failure,
            resolved_at,
        } => BridgeTaskFailureState::Resolved {
            failure: bridge_task_failure_detail(failure),
            resolved_at,
        },
    };
    BridgeTaskFailureDto {
        id: failure.id,
        source_thread_id: failure.source_thread_id,
        source_turn_id: failure.source_turn_id,
        source_agent_id: failure.source_agent_id,
        source_role: failure.source_role,
        work_unit_id: failure.work_unit_id,
        review_round_id: failure.review_round_id,
        state,
        created_at: failure.created_at,
    }
}

fn bridge_task_failure_detail(failure: pl_protocol::TurnFailure) -> BridgeTaskFailureDetail {
    BridgeTaskFailureDetail {
        category: format!("{:?}", failure.category).to_ascii_lowercase(),
        provider_kind: failure
            .provider_kind
            .map(|kind| format!("{kind:?}").to_ascii_lowercase()),
        code: failure.code,
        http_status: failure.http_status,
        message: failure.message,
        retryable: failure.retry.is_retryable(),
    }
}
