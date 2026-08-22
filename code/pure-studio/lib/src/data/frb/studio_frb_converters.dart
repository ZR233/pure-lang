part of 'studio_api.dart';

StudioBridgeEventPayload _productPayloadFromFrb(
  frb.BridgeProductEventPayload payload,
) {
  return switch (payload) {
    frb.BridgeProductEventPayload_ProjectDirectoryChanged(:final field0) =>
      ProjectDirectoryChangedPayload(_projectDirectoryFromFrb(field0)),
    frb.BridgeProductEventPayload_ThreadDirectoryChanged(:final field0) =>
      ThreadDirectoryChangedPayload(
        upserted: field0.upserted.map(_threadFromFrb).toList(),
        removed: field0.removed.toList(),
      ),
    frb.BridgeProductEventPayload_TaskDirectoryChanged(:final field0) =>
      TaskDirectoryChangedPayload(_taskDirectoryFromFrb(field0)),
    frb.BridgeProductEventPayload_AgentDirectoryChanged(:final field0) =>
      AgentDirectoryChangedPayload(_agentDirectoryFromFrb(field0)),
    frb.BridgeProductEventPayload_SettingsStateChanged(:final field0) =>
      SettingsStateChangedPayload(_settingsStateFromFrb(field0)),
    frb.BridgeProductEventPayload_RecoveryStateChanged(:final field0) =>
      RecoveryStateChangedPayload(_recoveryStateFromFrb(field0)),
    frb.BridgeProductEventPayload_McpStateChanged(:final field0) =>
      McpStateChangedPayload(_mcpStateFromFrb(field0)),
    frb.BridgeProductEventPayload_LspStateChanged(:final field0) =>
      LspStateChangedPayload(_lspStateFromFrb(field0)),
    frb.BridgeProductEventPayload_SkillsStateChanged(:final field0) =>
      SkillsStateChangedPayload(_skillsStateFromFrb(field0)),
    frb.BridgeProductEventPayload_ProviderUsageStateChanged(:final field0) =>
      ProviderUsageStateChangedPayload(_providerUsageStateFromFrb(field0)),
    frb.BridgeProductEventPayload_UpdaterStateChanged(:final field0) =>
      UpdaterStateChangedPayload(updaterStateFromFrb(field0)),
    frb.BridgeProductEventPayload_Stale(:final laggedEvents) => StalePayload(
      laggedEvents: laggedEvents.toInt(),
    ),
  };
}

TaskRuntimeView _taskRuntimeFromFrb(frb.BridgeTaskRuntimeDto task) {
  return TaskRuntimeView(
    runId: task.runId,
    state: _taskStateFromFrb(task.state),
    revision: task.revision.toInt(),
    integratedReviewGate: switch (task.integratedReviewGate) {
      frb.BridgeIntegratedReviewGateDto_Required(:final reason) =>
        IntegratedReviewGateView.required(reason: reason),
      frb.BridgeIntegratedReviewGateDto_SatisfiedByReview(
        :final reviewRoundId,
        :final reviewedHead,
      ) =>
        IntegratedReviewGateView.satisfiedByReview(
          reviewRoundId: reviewRoundId,
          reviewedHead: reviewedHead,
        ),
      frb.BridgeIntegratedReviewGateDto_NotRequiredNoDelivery() =>
        const IntegratedReviewGateView.notRequiredNoDelivery(),
      frb.BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent(
        :final workUnitId,
        :final completionRevision,
        :final mergeRecordId,
      ) =>
        IntegratedReviewGateView.notRequiredSingleExecutorEquivalent(
          workUnitId: workUnitId,
          completionRevision: completionRevision,
          mergeRecordId: mergeRecordId,
        ),
    },
    failures: task.failures.map(_taskFailureFromFrb).toList(),
    terminalFailure: task.terminalFailure == null
        ? null
        : _taskFailureFromFrb(task.terminalFailure!),
    workUnits: [
      for (final unit in task.workUnits)
        TaskWorkUnitView(
          id: unit.id,
          title: unit.title,
          state: _taskWorkUnitStateFromFrb(unit.state),
          worktreePath: unit.worktreePath,
          branch: unit.branch,
          agentId: unit.agentId,
          budgetSliceLimit: unit.budgetSliceLimit,
          executorProgressRevision: unit.executorProgressRevision,
          blueprintFingerprint: unit.blueprintFingerprint,
          objective: unit.objective,
          implementationStepCount: unit.implementationStepCount.toInt(),
          acceptanceCriterionCount: unit.acceptanceCriterionCount.toInt(),
          verificationCount: unit.verificationCount.toInt(),
        ),
    ],
    completions: [
      for (final completion in task.completions)
        TaskCompletionView(
          id: completion.id,
          workUnitId: completion.workUnitId,
          executorAgentId: completion.executorAgentId,
          revision: completion.revision,
          content: _taskCompletionContentFromFrb(completion.content),
          state: _taskCompletionStateFromFrb(completion.state),
          stateRevision: completion.stateRevision,
          baseCommit: completion.baseCommit,
          verificationSummary: completion.verificationSummary,
          worktreePath: completion.worktreePath,
          branch: completion.branch,
          createdAt: _dateFromUnix(completion.createdAt),
          updatedAt: _dateFromUnix(completion.updatedAt),
        ),
    ],
    merges: [
      for (final merge in task.merges)
        TaskMergeView(
          id: merge.id,
          workUnitId: merge.workUnitId,
          completionId: merge.completionId,
          completionRevision: merge.completionRevision,
          executorAgentId: merge.executorAgentId,
          expectedPreviousHead: merge.expectedPreviousHead,
          resultingHead: merge.resultingHead,
          deliveryHead: merge.deliveryHead,
          method: switch (merge.method) {
            frb.BridgeMergeMethod.merge => TaskMergeMethodView.merge,
            frb.BridgeMergeMethod.cherryPick => TaskMergeMethodView.cherryPick,
            frb.BridgeMergeMethod.squash => TaskMergeMethodView.squash,
            frb.BridgeMergeMethod.rebase => TaskMergeMethodView.rebase,
            frb.BridgeMergeMethod.manual => TaskMergeMethodView.manual,
          },
          summary: merge.summary,
          cleanup: _mergeCleanupFromFrb(merge.cleanup),
          createdAt: _dateFromUnix(merge.createdAt),
          updatedAt: _dateFromUnix(merge.updatedAt),
        ),
    ],
    reviews: [
      for (final review in task.reviews)
        TaskReviewView(
          id: review.id,
          round: review.round,
          scope: switch (review.scope) {
            frb.BridgeReviewScope.delivery => TaskReviewScopeView.delivery,
            frb.BridgeReviewScope.integrated => TaskReviewScopeView.integrated,
          },
          workUnitId: review.workUnitId,
          completionId: review.completionId,
          completionRevision: review.completionRevision,
          reviewedHead: review.reviewedHead,
          state: _taskReviewStateFromFrb(review.state),
          requestedByCallId: review.requestedByCallId,
          designReferences: [
            for (final reference in review.designReferences)
              TaskDesignReferenceView(
                path: reference.path,
                section: reference.section,
              ),
          ],
          findings: [
            for (final finding in review.findings)
              TaskReviewFindingView(
                severity: finding.severity,
                title: finding.title,
                body: finding.body,
                recommendation: finding.recommendation,
                path: finding.path,
                line: finding.line,
                designReferences: [
                  for (final reference in finding.designReferences)
                    TaskDesignReferenceView(
                      path: reference.path,
                      section: reference.section,
                    ),
                ],
              ),
          ],
          createdAt: _dateFromUnix(review.createdAt),
          updatedAt: _dateFromUnix(review.updatedAt),
        ),
    ],
  );
}

TaskCompletionContentView _taskCompletionContentFromFrb(
  frb.BridgeTaskCompletionContent content,
) {
  return switch (content) {
    frb.BridgeTaskCompletionContent_Delivery(:final field0) =>
      DeliveryTaskCompletionView(
        headCommit: field0.headCommit,
        changedFiles: field0.changedFiles,
      ),
    frb.BridgeTaskCompletionContent_NoDelivery() =>
      const NoDeliveryTaskCompletionView(),
  };
}

TaskCompletionStateView _taskCompletionStateFromFrb(
  frb.BridgeTaskCompletionState state,
) {
  return switch (state) {
    frb.BridgeTaskCompletionState_ReadyForReview() =>
      const ReadyForReviewTaskCompletionView(),
    frb.BridgeTaskCompletionState_ChangesRequired(:final field0) =>
      ChangesRequiredTaskCompletionView(
        reviewRoundId: field0.reviewRoundId,
        decidedAt: _dateFromUnix(field0.decidedAt),
      ),
    frb.BridgeTaskCompletionState_Approved(:final field0) =>
      ApprovedTaskCompletionView(
        reviewRoundId: field0.reviewRoundId,
        decidedAt: _dateFromUnix(field0.decidedAt),
      ),
  };
}

MergeCleanupStateView _mergeCleanupFromFrb(frb.BridgeMergeCleanupState state) =>
    switch (state) {
      frb.BridgeMergeCleanupState_Pending() => const PendingMergeCleanupView(),
      frb.BridgeMergeCleanupState_Deferred() =>
        const DeferredMergeCleanupView(),
      frb.BridgeMergeCleanupState_Attempting(
        :final operationId,
        :final startedAt,
      ) =>
        AttemptingMergeCleanupView(operationId, _dateFromUnix(startedAt)),
      frb.BridgeMergeCleanupState_Discarded(
        :final operationId,
        :final completedAt,
      ) =>
        DiscardedMergeCleanupView(operationId, _dateFromUnix(completedAt)),
      frb.BridgeMergeCleanupState_AlreadyAbsent(
        :final operationId,
        :final completedAt,
      ) =>
        AlreadyAbsentMergeCleanupView(operationId, _dateFromUnix(completedAt)),
      frb.BridgeMergeCleanupState_Failed(
        :final operationId,
        :final failedAt,
        :final detail,
      ) =>
        FailedMergeCleanupView(
          operationId: operationId,
          failedAt: _dateFromUnix(failedAt),
          detail: detail,
        ),
    };

TaskStateView _taskStateFromFrb(frb.BridgeTaskState state) {
  return switch (state) {
    frb.BridgeTaskState_DesignUpdating(:final field0) =>
      DesignUpdatingTaskStateView(generation: field0.generation.toInt()),
    frb.BridgeTaskState_Implementing(:final field0) =>
      ImplementingTaskStateView(
        generation: field0.generation.toInt(),
        design: TaskFinalizedDesignView(field0.design.summary),
      ),
    frb.BridgeTaskState_Merging(:final field0) => MergingTaskStateView(
      generation: field0.generation.toInt(),
      design: TaskFinalizedDesignView(field0.design.summary),
      statusMessage: field0.statusMessage,
    ),
    frb.BridgeTaskState_Reviewing(:final field0) => ReviewingTaskStateView(
      generation: field0.generation.toInt(),
      design: TaskFinalizedDesignView(field0.design.summary),
      target: _taskReviewTargetFromFrb(field0.target),
      statusMessage: field0.statusMessage,
    ),
    frb.BridgeTaskState_Reworking(:final field0) => ReworkingTaskStateView(
      generation: field0.generation.toInt(),
      design: TaskFinalizedDesignView(field0.design.summary),
      statusMessage: field0.statusMessage,
    ),
    frb.BridgeTaskState_Stopping(:final field0) => StoppingTaskStateView(
      generation: field0.generation.toInt(),
      design: _taskDesignProgressFromFrb(field0.design),
      request: _taskStopRequestFromFrb(field0.request),
      statusMessage: field0.statusMessage,
    ),
    frb.BridgeTaskState_Blocked(:final field0) => BlockedTaskStateView(
      generation: field0.generation.toInt(),
      design: _taskDesignProgressFromFrb(field0.design),
      message: field0.message,
      recovery: switch (field0.recovery) {
        frb.BridgeBlockedRecovery.retryMerge =>
          TaskBlockedRecoveryView.retryMerge,
        frb.BridgeBlockedRecovery.resumeRework =>
          TaskBlockedRecoveryView.resumeRework,
        frb.BridgeBlockedRecovery.manualOnly =>
          TaskBlockedRecoveryView.manualOnly,
      },
    ),
    frb.BridgeTaskState_Completed(:final field0) => CompletedTaskStateView(
      generation: field0.generation.toInt(),
      design: TaskFinalizedDesignView(field0.design.summary),
    ),
    frb.BridgeTaskState_Failed(:final field0) => FailedTaskStateView(
      generation: field0.generation.toInt(),
      design: _taskDesignProgressFromFrb(field0.design),
      message: field0.message,
      failureId: field0.failureId,
    ),
    frb.BridgeTaskState_Cancelled(:final field0) => CancelledTaskStateView(
      generation: field0.generation.toInt(),
      design: _taskDesignProgressFromFrb(field0.design),
      message: field0.message,
      request: field0.request == null
          ? null
          : _taskStopRequestFromFrb(field0.request!),
    ),
  };
}

TaskDesignProgressView _taskDesignProgressFromFrb(
  frb.BridgeDesignProgress progress,
) => switch (progress) {
  frb.BridgeDesignProgress_Updating() => const UpdatingTaskDesignView(),
  frb.BridgeDesignProgress_Finalized(:final field0) => TaskFinalizedDesignView(
    field0.summary,
  ),
};

TaskStopRequestView _taskStopRequestFromFrb(
  frb.BridgeTaskStopRequest request,
) => TaskStopRequestView(
  origin: switch (request.origin) {
    frb.BridgeTaskStopOrigin.userRequest => TaskStopOriginView.userRequest,
    frb.BridgeTaskStopOrigin.plannerDecision =>
      TaskStopOriginView.plannerDecision,
    frb.BridgeTaskStopOrigin.runtimeFailure =>
      TaskStopOriginView.runtimeFailure,
    frb.BridgeTaskStopOrigin.applicationShutdown =>
      TaskStopOriginView.applicationShutdown,
  },
  reason: request.reason,
  requestedAt: _dateFromUnix(request.requestedAt),
);

TaskReviewTargetView _taskReviewTargetFromFrb(
  frb.BridgeTaskReviewTarget target,
) => switch (target) {
  frb.BridgeTaskReviewTarget_Delivery(
    :final workUnitId,
    :final completionId,
    :final completionRevision,
    :final reviewedHead,
  ) =>
    DeliveryTaskReviewTargetView(
      workUnitId: workUnitId,
      completionId: completionId,
      completionRevision: completionRevision.toInt(),
      reviewedHead: reviewedHead,
    ),
  frb.BridgeTaskReviewTarget_Integration(:final reviewedHead) =>
    IntegrationTaskReviewTargetView(reviewedHead),
};

TaskWorkUnitStateView _taskWorkUnitStateFromFrb(
  frb.BridgeTaskWorkUnitState state,
) {
  return switch (state) {
    frb.BridgeTaskWorkUnitState_Pending() =>
      const PendingTaskWorkUnitStateView(),
    frb.BridgeTaskWorkUnitState_Running(:final field0) =>
      RunningTaskWorkUnitStateView(
        activity: switch (field0.activity) {
          frb.BridgeRunningWorkUnitActivity_Allocated() =>
            const AllocatedTaskRunningActivityView(),
          frb.BridgeRunningWorkUnitActivity_Active(:final turnId) =>
            ActiveTaskRunningActivityView(turnId),
        },
        continuation: _taskExecutorContinuationFromFrb(field0.continuation),
      ),
    frb.BridgeTaskWorkUnitState_WaitingReview(:final field0) =>
      WaitingReviewTaskWorkUnitStateView(switch (field0.phase) {
        frb.BridgeWaitingReviewPhase_AwaitingReport(
          :final outcome,
          :final continuation,
        ) =>
          AwaitingReportTaskWaitingReviewView(
            outcome: switch (outcome) {
              frb.BridgeExecutorTerminalOutcome_Completed(
                :final sourceTurnId,
                :final detail,
              ) =>
                CompletedTaskExecutorOutcomeView(
                  sourceTurnId: sourceTurnId,
                  detail: detail,
                ),
              frb.BridgeExecutorTerminalOutcome_Failed(
                :final sourceTurnId,
                :final detail,
              ) =>
                FailedTaskExecutorOutcomeView(
                  sourceTurnId: sourceTurnId,
                  detail: detail,
                ),
            },
            continuation: _taskExecutorContinuationFromFrb(continuation),
          ),
        frb.BridgeWaitingReviewPhase_Ready(
          :final completionId,
          :final completionRevision,
          :final verificationSummary,
        ) =>
          ReadyTaskWaitingReviewView(
            completionId: completionId,
            completionRevision: completionRevision,
            verificationSummary: verificationSummary,
          ),
        frb.BridgeWaitingReviewPhase_Reviewing(
          :final completionId,
          :final completionRevision,
          :final reviewRoundId,
          :final verificationSummary,
        ) =>
          ReviewingTaskWaitingReviewView(
            completionId: completionId,
            completionRevision: completionRevision,
            reviewRoundId: reviewRoundId,
            verificationSummary: verificationSummary,
          ),
      }),
    frb.BridgeTaskWorkUnitState_ReviewPassed(:final field0) =>
      ReviewPassedTaskWorkUnitStateView(
        completionId: field0.completionId,
        completionRevision: field0.completionRevision,
        reviewRoundId: field0.reviewRoundId,
        outcome: switch (field0.outcome) {
          frb.BridgeReviewPassedOutcome.delivery =>
            TaskReviewPassedOutcomeView.delivery,
          frb.BridgeReviewPassedOutcome.noDelivery =>
            TaskReviewPassedOutcomeView.noDelivery,
        },
        verificationSummary: field0.verificationSummary,
      ),
    frb.BridgeTaskWorkUnitState_ChangesRequired(:final field0) =>
      ChangesRequiredTaskWorkUnitStateView(
        completionId: field0.completionId,
        completionRevision: field0.completionRevision,
        reviewRoundId: field0.reviewRoundId,
        continuationRevision: field0.continuationRevision,
        sliceCount: field0.sliceCount,
      ),
    frb.BridgeTaskWorkUnitState_Paused(:final field0) =>
      PausedTaskWorkUnitStateView(
        reason: switch (field0.reason) {
          frb.BridgeWorkUnitPauseReason_Budget(:final limit) =>
            BudgetTaskWorkUnitPauseReasonView(_taskBudgetLimitFromFrb(limit)),
          frb.BridgeWorkUnitPauseReason_Operational(
            :final operationId,
            :final detail,
          ) =>
            OperationalTaskWorkUnitPauseReasonView(
              operationId: operationId,
              detail: detail,
            ),
        },
        continuation: _taskExecutorContinuationFromFrb(field0.continuation),
      ),
    frb.BridgeTaskWorkUnitState_Completed(:final field0) =>
      CompletedTaskWorkUnitStateView(switch (field0.outcome) {
        frb.BridgeWorkUnitCompletionOutcome_Merged(:final mergeRecordId) =>
          MergedTaskWorkUnitCompletionView(mergeRecordId),
        frb.BridgeWorkUnitCompletionOutcome_NoDelivery(:final completionId) =>
          NoDeliveryTaskWorkUnitCompletionView(completionId),
      }),
    frb.BridgeTaskWorkUnitState_Failed(:final field0) =>
      FailedTaskWorkUnitStateView(
        failure: switch (field0.failure) {
          frb.BridgeWorkUnitFailure_Spawn(:final failure) =>
            SpawnTaskWorkUnitFailureView(_taskSpawnFailureFromFrb(failure)),
          frb.BridgeWorkUnitFailure_Execution(
            :final operationId,
            :final detail,
          ) =>
            ExecutionTaskWorkUnitFailureView(
              operationId: operationId,
              detail: detail,
            ),
        },
        worktreeDisposition: switch (field0.worktreeDisposition) {
          frb.BridgeTaskWorktreeDisposition.protect =>
            TaskWorktreeDispositionView.protect,
          frb.BridgeTaskWorktreeDisposition.cleanupRequested =>
            TaskWorktreeDispositionView.cleanupRequested,
        },
      ),
    frb.BridgeTaskWorkUnitState_Cancelled(:final field0) =>
      CancelledTaskWorkUnitStateView(
        operationId: field0.operationId,
        reason: field0.reason,
        worktreeDisposition: switch (field0.worktreeDisposition) {
          frb.BridgeTaskWorktreeDisposition.protect =>
            TaskWorktreeDispositionView.protect,
          frb.BridgeTaskWorktreeDisposition.cleanupRequested =>
            TaskWorktreeDispositionView.cleanupRequested,
        },
      ),
  };
}

TaskExecutorContinuationView _taskExecutorContinuationFromFrb(
  frb.BridgeExecutorContinuationState state,
) {
  return switch (state) {
    frb.BridgeExecutorContinuationState_Idle(
      :final revision,
      :final sliceCount,
    ) =>
      IdleTaskExecutorContinuationView(
        revision: revision,
        sliceCount: sliceCount,
      ),
    frb.BridgeExecutorContinuationState_Compacting(
      :final revision,
      :final sourceTurnId,
      :final sliceCount,
    ) =>
      CompactingTaskExecutorContinuationView(
        revision: revision,
        sliceCount: sliceCount,
        turnId: sourceTurnId,
      ),
    frb.BridgeExecutorContinuationState_PendingStart(
      :final revision,
      :final sourceTurnId,
      :final sliceCount,
      :final limit,
    ) =>
      PendingStartTaskExecutorContinuationView(
        revision: revision,
        sliceCount: sliceCount,
        turnId: sourceTurnId,
        limit: _taskBudgetLimitFromFrb(limit),
      ),
    frb.BridgeExecutorContinuationState_PlannerWakePending(
      :final revision,
      :final sourceTurnId,
      :final sliceCount,
    ) =>
      PlannerWakePendingTaskExecutorContinuationView(
        revision: revision,
        sliceCount: sliceCount,
        turnId: sourceTurnId,
      ),
    frb.BridgeExecutorContinuationState_NeedsAttention(
      :final revision,
      :final sourceTurnId,
      :final sliceCount,
      :final detail,
    ) =>
      NeedsAttentionTaskExecutorContinuationView(
        revision: revision,
        sliceCount: sliceCount,
        turnId: sourceTurnId,
        attentionDetail: detail,
      ),
  };
}

TaskBudgetLimitView _taskBudgetLimitFromFrb(frb.BridgeBudgetLimitDto limit) {
  return TaskBudgetLimitView(
    kind: switch (limit.kind) {
      frb.BridgeBudgetLimitKind.modelStep => TaskBudgetLimitKindView.modelStep,
      frb.BridgeBudgetLimitKind.toolCall => TaskBudgetLimitKindView.toolCall,
      frb.BridgeBudgetLimitKind.wait => TaskBudgetLimitKindView.wait,
      frb.BridgeBudgetLimitKind.wallClock => TaskBudgetLimitKindView.wallClock,
      frb.BridgeBudgetLimitKind.agentCount =>
        TaskBudgetLimitKindView.agentCount,
      frb.BridgeBudgetLimitKind.agentDepth =>
        TaskBudgetLimitKindView.agentDepth,
      frb.BridgeBudgetLimitKind.finalization =>
        TaskBudgetLimitKindView.finalization,
    },
    usage: TaskBudgetUsageView(
      modelSteps: limit.usage.modelSteps,
      toolCalls: limit.usage.toolCalls,
      waitCalls: limit.usage.waitCalls,
      elapsedMs: limit.usage.elapsedMs,
    ),
  );
}

TaskSpawnFailureView _taskSpawnFailureFromFrb(
  frb.BridgeTaskSpawnFailure failure,
) {
  return TaskSpawnFailureView(
    code: switch (failure.code) {
      frb.BridgeTaskSpawnFailureCode.allocation =>
        TaskSpawnFailureCodeView.allocation,
      frb.BridgeTaskSpawnFailureCode.worktreeCreate =>
        TaskSpawnFailureCodeView.worktreeCreate,
      frb.BridgeTaskSpawnFailureCode.childThreadCreate =>
        TaskSpawnFailureCodeView.childThreadCreate,
      frb.BridgeTaskSpawnFailureCode.agentRegistration =>
        TaskSpawnFailureCodeView.agentRegistration,
      frb.BridgeTaskSpawnFailureCode.activation =>
        TaskSpawnFailureCodeView.activation,
    },
    phase: switch (failure.phase) {
      frb.BridgeTaskSpawnFailurePhase.allocation =>
        TaskSpawnFailurePhaseView.allocation,
      frb.BridgeTaskSpawnFailurePhase.worktreeCreate =>
        TaskSpawnFailurePhaseView.worktreeCreate,
      frb.BridgeTaskSpawnFailurePhase.childThreadCreate =>
        TaskSpawnFailurePhaseView.childThreadCreate,
      frb.BridgeTaskSpawnFailurePhase.agentRegistration =>
        TaskSpawnFailurePhaseView.agentRegistration,
      frb.BridgeTaskSpawnFailurePhase.activation =>
        TaskSpawnFailurePhaseView.activation,
    },
    recoverable: failure.recoverable,
    message: failure.message,
    taskRunId: failure.taskRunId,
    workUnitId: failure.workUnitId,
    agentId: failure.agentId,
    resource: failure.resource == null
        ? null
        : TaskSpawnResourceView(
            repoRoot: failure.resource!.repoRoot,
            path: failure.resource!.path,
            branch: failure.resource!.branch,
            baseRef: failure.resource!.baseRef,
          ),
    cause: TaskWorktreeFailureCauseView(
      kind: switch (failure.cause.kind) {
        frb.BridgeWorktreeFailureCauseKind.invalidRepoRoot =>
          TaskWorktreeFailureCauseKindView.invalidRepoRoot,
        frb.BridgeWorktreeFailureCauseKind.unsafeBranch =>
          TaskWorktreeFailureCauseKindView.unsafeBranch,
        frb.BridgeWorktreeFailureCauseKind.gitLaunchFailed =>
          TaskWorktreeFailureCauseKindView.gitLaunchFailed,
        frb.BridgeWorktreeFailureCauseKind.gitTimedOut =>
          TaskWorktreeFailureCauseKindView.gitTimedOut,
        frb.BridgeWorktreeFailureCauseKind.gitExited =>
          TaskWorktreeFailureCauseKindView.gitExited,
        frb.BridgeWorktreeFailureCauseKind.gitStatusUnknown =>
          TaskWorktreeFailureCauseKindView.gitStatusUnknown,
        frb.BridgeWorktreeFailureCauseKind.io =>
          TaskWorktreeFailureCauseKindView.io,
        frb.BridgeWorktreeFailureCauseKind.disabled =>
          TaskWorktreeFailureCauseKindView.disabled,
        frb.BridgeWorktreeFailureCauseKind.operationAndCleanupFailed =>
          TaskWorktreeFailureCauseKindView.operationAndCleanupFailed,
      },
      message: failure.cause.message,
      args: failure.cause.args,
      exitCode: failure.cause.exitCode,
      stderr: failure.cause.stderr,
    ),
    compensation: TaskSpawnCompensationView(
      allocation: _spawnCompensationFromFrb(failure.compensation.allocation),
      worktree: _spawnCompensationFromFrb(failure.compensation.worktree),
      childThread: _spawnCompensationFromFrb(failure.compensation.childThread),
    ),
    nextAction: switch (failure.nextAction) {
      frb.BridgeTaskSpawnNextAction.retryTaskSpawnExecutor =>
        TaskSpawnNextActionView.retryTaskSpawnExecutor,
      frb.BridgeTaskSpawnNextAction.recoverWorktreeResources =>
        TaskSpawnNextActionView.recoverWorktreeResources,
    },
  );
}

TaskSpawnCompensationStateView _spawnCompensationFromFrb(
  frb.BridgeTaskSpawnCompensationState state,
) => switch (state) {
  frb.BridgeTaskSpawnCompensationState.notCreated =>
    TaskSpawnCompensationStateView.notCreated,
  frb.BridgeTaskSpawnCompensationState.markedFailed =>
    TaskSpawnCompensationStateView.markedFailed,
  frb.BridgeTaskSpawnCompensationState.removed =>
    TaskSpawnCompensationStateView.removed,
  frb.BridgeTaskSpawnCompensationState.faulted =>
    TaskSpawnCompensationStateView.faulted,
  frb.BridgeTaskSpawnCompensationState.cleanupFailed =>
    TaskSpawnCompensationStateView.cleanupFailed,
  frb.BridgeTaskSpawnCompensationState.unknown =>
    TaskSpawnCompensationStateView.unknown,
};

TaskReviewStateView _taskReviewStateFromFrb(frb.BridgeTaskReviewState state) {
  return switch (state) {
    frb.BridgeTaskReviewState_PendingDispatch() =>
      const PendingTaskReviewDispatchView(),
    frb.BridgeTaskReviewState_Dispatched(:final reviewerAgentId) =>
      DispatchedTaskReviewView(reviewerAgentId),
    frb.BridgeTaskReviewState_Running(:final reviewerAgentId) =>
      RunningTaskReviewView(reviewerAgentId),
    frb.BridgeTaskReviewState_Passed(:final reviewerAgentId, :final summary) =>
      PassedTaskReviewView(reviewerAgentId, summary),
    frb.BridgeTaskReviewState_ChangesRequired(
      :final reviewerAgentId,
      :final summary,
    ) =>
      ChangesRequiredTaskReviewView(reviewerAgentId, summary),
    frb.BridgeTaskReviewState_Blocked(:final reviewerAgentId, :final summary) =>
      BlockedTaskReviewView(reviewerAgentId, summary),
    frb.BridgeTaskReviewState_Failed(
      :final reviewerAgentId,
      :final summary,
      :final error,
    ) =>
      FailedTaskReviewView(
        reviewerAgentId: reviewerAgentId,
        failure: error,
        failureSummary: summary,
      ),
    frb.BridgeTaskReviewState_Cancelled(
      :final reviewerAgentId,
      :final summary,
      :final reason,
    ) =>
      CancelledTaskReviewView(
        reviewerAgentId: reviewerAgentId,
        reason: reason,
        cancellationSummary: summary,
      ),
  };
}

TaskFailureView _taskFailureFromFrb(frb.BridgeTaskFailureDto failure) {
  final state = switch (failure.state) {
    frb.BridgeTaskFailureState_OpenRecoverable(:final failure) =>
      OpenRecoverableTaskFailureView(_taskFailureDetailFromFrb(failure)),
    frb.BridgeTaskFailureState_OpenFatal(:final failure) =>
      OpenFatalTaskFailureView(_taskFailureDetailFromFrb(failure)),
    frb.BridgeTaskFailureState_Resolved(:final failure, :final resolvedAt) =>
      ResolvedTaskFailureView(
        _taskFailureDetailFromFrb(failure),
        _dateFromUnix(resolvedAt),
      ),
  };
  return TaskFailureView(
    id: failure.id,
    sourceThreadId: failure.sourceThreadId,
    sourceTurnId: failure.sourceTurnId,
    sourceAgentId: failure.sourceAgentId,
    sourceRole: failure.sourceRole,
    workUnitId: failure.workUnitId,
    reviewRoundId: failure.reviewRoundId,
    state: state,
    createdAt: _dateFromUnix(failure.createdAt),
  );
}

TaskFailureDetailView _taskFailureDetailFromFrb(
  frb.BridgeTaskFailureDetail failure,
) => TaskFailureDetailView(
  category: failure.category,
  providerKind: failure.providerKind,
  code: failure.code,
  httpStatus: failure.httpStatus,
  message: failure.message,
  retryable: failure.retryable,
);

StudioAgentView _agentDirectoryEntryFromFrb(
  frb.BridgeAgentDirectoryEntryDto agent,
) {
  return StudioAgentView(
    id: agent.id,
    threadId: agent.threadId,
    rootThreadId: agent.rootThreadId,
    path: agent.path,
    parentPath: agent.parentPath,
    role: agent.role,
    task: agent.task,
    summary: agent.summary,
    depth: agent.depth,
    state: _agentStateFromFrb(agent.state),
    progress: _agentProgressFromFrb(agent.progress),
    updatedAt: _dateFromUnix(agent.updatedAt),
    summaryAgeSeconds: agent.summaryAgeSeconds.toInt(),
  );
}

StudioAgentState _agentStateFromFrb(frb.BridgeAgentState state) {
  return switch (state) {
    frb.BridgeAgentState_Idle() => const IdleStudioAgent(),
    frb.BridgeAgentState_Queued(:final field0) => QueuedStudioAgent(
      field0.turnId,
    ),
    frb.BridgeAgentState_Running(:final field0) => RunningStudioAgent(
      field0.turnId,
    ),
    frb.BridgeAgentState_WaitingTool(:final field0) => WaitingToolStudioAgent(
      field0.turnId,
    ),
    frb.BridgeAgentState_WaitingInteraction(:final field0) =>
      WaitingInteractionStudioAgent(field0.turnId, field0.interactionId),
    frb.BridgeAgentState_Cancelling(:final field0) => CancellingStudioAgent(
      field0.turnId,
    ),
    frb.BridgeAgentState_Closing() => const ClosingStudioAgent(),
    frb.BridgeAgentState_Closed() => const ClosedStudioAgent(),
    frb.BridgeAgentState_Faulted(:final field0) => FaultedStudioAgent(
      failure: AgentStateError(
        code: field0.error.code,
        message: field0.error.message,
        retryable: field0.error.retryable,
      ),
      diagnosticTurnId: field0.diagnosticTurnId,
    ),
  };
}

AgentProgressView? _agentProgressFromFrb(frb.BridgeAgentProgressDto? progress) {
  if (progress == null) {
    return null;
  }
  return AgentProgressView(
    stage: progress.stage,
    summary: progress.summary,
    nextStep: progress.nextStep,
    revision: progress.revision.toInt(),
    updatedAt: _dateFromUnix(progress.updatedAt),
  );
}

StudioProject _projectFromFrb(frb.ProjectDto project) {
  return StudioProject(id: project.id, name: project.name, path: project.path);
}

McpServerSettingsView _mcpServerFromFrb(frb.BridgeMcpServerDto server) {
  return McpServerSettingsView(
    id: server.id,
    transport: server.transport,
    endpoint: server.endpoint,
    state: switch (server.state) {
      frb.BridgeMcpServerState_Disabled(:final message) => McpDisabledState(
        message: message,
      ),
      frb.BridgeMcpServerState_MissingCredential(:final message) =>
        McpMissingCredentialState(message: message),
      frb.BridgeMcpServerState_Checking(:final message) => McpCheckingState(
        message: message,
      ),
      frb.BridgeMcpServerState_Available(:final checkedAt, :final toolCount) =>
        McpAvailableState(
          checkedAt: checkedAt.toInt(),
          toolCount: toolCount.toInt(),
        ),
      frb.BridgeMcpServerState_Unavailable(:final checkedAt, :final error) =>
        McpUnavailableState(
          checkedAt: checkedAt.toInt(),
          code: error.code,
          message: error.message,
          retryable: error.retryable,
        ),
    },
    sourceKind: server.sourceKind,
    mutationPolicy: server.mutationPolicy,
  );
}

StudioState studioStateFromFrbSnapshot(frb.BridgeStudioStateSnapshot value) {
  final projectDirectory = _projectDirectoryFromFrb(value.projectDirectory);
  final threadPage = _threadDirectoryPageFromFrb(value.threadDirectory);
  return StudioState(
    projectDirectory: projectDirectory,
    threadDirectory: ThreadDirectoryWindow(
      threads: threadPage.threads,
      nextCursor: threadPage.nextCursor,
      hasMore: threadPage.nextCursor != null,
    ),
    taskDirectory: _taskDirectoryFromFrb(value.taskDirectory),
    agentDirectory: _agentDirectoryFromFrb(value.agentDirectory),
    settingsState: _settingsStateFromFrb(value.settings),
    recoveryState: _recoveryStateFromFrb(value.recovery),
    mcpState: _mcpStateFromFrb(value.mcp),
    lspState: _lspStateFromFrb(value.lsp),
    skillsByProject: {
      for (final snapshot in value.skillsByProject)
        snapshot.projectId: _skillsStateFromFrb(snapshot),
    },
    providerUsageState: _providerUsageStateFromFrb(value.providerUsage),
    updaterState: updaterStateFromFrb(value.updater),
    selectedProjectId: null,
    selectedThreadId: null,
  );
}

ProjectDirectoryState _projectDirectoryFromFrb(
  frb.BridgeProjectDirectoryState snapshot,
) {
  List<StudioProject> convert(frb.BridgeProjectDirectoryData data) =>
      data.projects.map(_projectFromFrb).toList();
  return ProjectDirectoryState.fromState(
    state: switch (snapshot) {
      frb.BridgeProjectDirectoryState_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeProjectDirectoryState_Loading(:final field0) =>
        _loadingResource(field0),
      frb.BridgeProjectDirectoryState_Ready(:final resource, :final value) =>
        _readyResource(resource, convert(value)),
      frb.BridgeProjectDirectoryState_Refreshing(
        :final resource,
        :final value,
      ) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeProjectDirectoryState_Stale(:final resource, :final value) =>
        _staleResource(resource, convert(value)),
      frb.BridgeProjectDirectoryState_Degraded(:final resource, :final value) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeProjectDirectoryState_Failed(:final field0) => _failedResource(
        field0,
      ),
      frb.BridgeProjectDirectoryState_Stopped(:final field0) =>
        _stoppedResource(field0),
    },
  );
}

ThreadDirectoryPage _threadDirectoryPageFromFrb(
  frb.BridgeThreadDirectoryPage snapshot,
) {
  ThreadDirectoryPage convert(frb.BridgeThreadDirectoryPageData data) =>
      ThreadDirectoryPage(
        threads: data.threads.map(_threadFromFrb).toList(),
        nextCursor: data.nextCursor,
      );
  final resource = switch (snapshot) {
    frb.BridgeThreadDirectoryPage_Uninitialized(:final field0) =>
      _uninitializedResource<ThreadDirectoryPage>(field0),
    frb.BridgeThreadDirectoryPage_Loading(:final field0) =>
      _loadingResource<ThreadDirectoryPage>(field0),
    frb.BridgeThreadDirectoryPage_Ready(:final resource, :final value) =>
      _readyResource(resource, convert(value)),
    frb.BridgeThreadDirectoryPage_Refreshing(:final resource, :final value) =>
      _refreshingResource(resource, convert(value)),
    frb.BridgeThreadDirectoryPage_Stale(:final resource, :final value) =>
      _staleResource(resource, convert(value)),
    frb.BridgeThreadDirectoryPage_Degraded(:final resource, :final value) =>
      _degradedResource(resource, convert(value)),
    frb.BridgeThreadDirectoryPage_Failed(:final field0) =>
      _failedResource<ThreadDirectoryPage>(field0),
    frb.BridgeThreadDirectoryPage_Stopped(:final field0) =>
      _stoppedResource<ThreadDirectoryPage>(field0),
  };
  return resource.value ?? const ThreadDirectoryPage(threads: []);
}

TaskDirectoryState _taskDirectoryFromFrb(
  frb.BridgeTaskDirectoryState snapshot,
) {
  List<TaskDirectoryEntryView> convert(frb.BridgeTaskDirectoryData data) => [
    for (final entry in data.tasks)
      TaskDirectoryEntryView(
        rootThreadId: entry.rootThreadId,
        task: _taskRuntimeFromFrb(entry.task),
      ),
  ];
  return TaskDirectoryState.fromState(
    state: switch (snapshot) {
      frb.BridgeTaskDirectoryState_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeTaskDirectoryState_Loading(:final field0) => _loadingResource(
        field0,
      ),
      frb.BridgeTaskDirectoryState_Ready(:final resource, :final value) =>
        _readyResource(resource, convert(value)),
      frb.BridgeTaskDirectoryState_Refreshing(:final resource, :final value) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeTaskDirectoryState_Stale(:final resource, :final value) =>
        _staleResource(resource, convert(value)),
      frb.BridgeTaskDirectoryState_Degraded(:final resource, :final value) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeTaskDirectoryState_Failed(:final field0) => _failedResource(
        field0,
      ),
      frb.BridgeTaskDirectoryState_Stopped(:final field0) => _stoppedResource(
        field0,
      ),
    },
  );
}

AgentDirectoryState _agentDirectoryFromFrb(
  frb.BridgeAgentDirectoryState snapshot,
) {
  List<StudioAgentView> convert(frb.BridgeAgentDirectoryData data) =>
      data.agents.map(_agentDirectoryEntryFromFrb).toList();
  return AgentDirectoryState.fromState(
    state: switch (snapshot) {
      frb.BridgeAgentDirectoryState_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeAgentDirectoryState_Loading(:final field0) => _loadingResource(
        field0,
      ),
      frb.BridgeAgentDirectoryState_Ready(:final resource, :final value) =>
        _readyResource(resource, convert(value)),
      frb.BridgeAgentDirectoryState_Refreshing(:final resource, :final value) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeAgentDirectoryState_Stale(:final resource, :final value) =>
        _staleResource(resource, convert(value)),
      frb.BridgeAgentDirectoryState_Degraded(:final resource, :final value) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeAgentDirectoryState_Failed(:final field0) => _failedResource(
        field0,
      ),
      frb.BridgeAgentDirectoryState_Stopped(:final field0) => _stoppedResource(
        field0,
      ),
    },
  );
}

RecoveryStateSnapshot _recoveryStateFromFrb(
  frb.BridgeRecoveryStateSnapshot snapshot,
) {
  List<StudioRecoveryIssue> convert(frb.BridgeRecoveryStateData data) =>
      data.issues.map(_recoveryIssueFromFrb).toList();
  return RecoveryStateSnapshot.fromState(
    state: switch (snapshot) {
      frb.BridgeRecoveryStateSnapshot_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeRecoveryStateSnapshot_Loading(:final field0) =>
        _loadingResource(field0),
      frb.BridgeRecoveryStateSnapshot_Ready(:final resource, :final value) =>
        _readyResource(resource, convert(value)),
      frb.BridgeRecoveryStateSnapshot_Refreshing(
        :final resource,
        :final value,
      ) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeRecoveryStateSnapshot_Stale(:final resource, :final value) =>
        _staleResource(resource, convert(value)),
      frb.BridgeRecoveryStateSnapshot_Degraded(:final resource, :final value) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeRecoveryStateSnapshot_Failed(:final field0) => _failedResource(
        field0,
      ),
      frb.BridgeRecoveryStateSnapshot_Stopped(:final field0) =>
        _stoppedResource(field0),
    },
  );
}

McpStateSnapshot _mcpStateFromFrb(frb.BridgeMcpStateSnapshot snapshot) {
  McpStateData convert(frb.BridgeMcpStateData data) => McpStateData(
    desiredConfigFingerprint: data.desiredConfigFingerprint,
    appliedConfigFingerprint: data.appliedConfigFingerprint,
    activeServers: data.health.activeMcpServers,
    servers: data.health.mcpServers.map(_mcpServerFromFrb).toList(),
  );
  return McpStateSnapshot.fromState(
    state: switch (snapshot) {
      frb.BridgeMcpStateSnapshot_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeMcpStateSnapshot_Loading(:final field0) => _loadingResource(
        field0,
      ),
      frb.BridgeMcpStateSnapshot_Ready(:final resource, :final value) =>
        _readyResource(resource, convert(value)),
      frb.BridgeMcpStateSnapshot_Refreshing(:final resource, :final value) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeMcpStateSnapshot_Stale(:final resource, :final value) =>
        _staleResource(resource, convert(value)),
      frb.BridgeMcpStateSnapshot_Degraded(:final resource, :final value) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeMcpStateSnapshot_Failed(:final field0) => _failedResource(
        field0,
      ),
      frb.BridgeMcpStateSnapshot_Stopped(:final field0) => _stoppedResource(
        field0,
      ),
    },
  );
}

LspStateSnapshot _lspStateFromFrb(frb.BridgeLspStateSnapshot snapshot) {
  LspStateData convert(frb.BridgeLspStateData data) => LspStateData(
    activeServers: data.health.activeLspServers,
    servers: [
      for (final server in data.health.lspServers)
        LspServerStateView(
          id: server.id,
          displayName: server.displayName,
          state: switch (server.state) {
            frb.BridgeLspServerState_Checking(:final message) =>
              LspCheckingState(message: message),
            frb.BridgeLspServerState_Available(
              :final checkedAt,
              :final diagnosticCount,
              :final activity,
            ) =>
              LspAvailableState(
                checkedAt: checkedAt.toInt(),
                diagnosticCount: diagnosticCount.toInt(),
                activity: switch (activity) {
                  frb.BridgeLspActivity_Idle() => const LspIdleActivity(),
                  frb.BridgeLspActivity_Busy(
                    :final title,
                    :final message,
                    :final percentage,
                  ) =>
                    LspBusyActivity(
                      title: title,
                      message: message,
                      percentage: percentage,
                    ),
                  frb.BridgeLspActivity_Indexing(
                    :final title,
                    :final message,
                    :final percentage,
                  ) =>
                    LspIndexingActivity(
                      title: title,
                      message: message,
                      percentage: percentage,
                    ),
                },
              ),
            frb.BridgeLspServerState_Unavailable(
              :final checkedAt,
              :final error,
            ) =>
              LspUnavailableState(
                checkedAt: checkedAt.toInt(),
                code: error.code,
                message: error.message,
                retryable: error.retryable,
              ),
            frb.BridgeLspServerState_Disabled(:final message) =>
              LspDisabledState(message: message),
          },
        ),
    ],
  );
  return LspStateSnapshot.fromState(
    state: switch (snapshot) {
      frb.BridgeLspStateSnapshot_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeLspStateSnapshot_Loading(:final field0) => _loadingResource(
        field0,
      ),
      frb.BridgeLspStateSnapshot_Ready(:final resource, :final value) =>
        _readyResource(resource, convert(value)),
      frb.BridgeLspStateSnapshot_Refreshing(:final resource, :final value) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeLspStateSnapshot_Stale(:final resource, :final value) =>
        _staleResource(resource, convert(value)),
      frb.BridgeLspStateSnapshot_Degraded(:final resource, :final value) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeLspStateSnapshot_Failed(:final field0) => _failedResource(
        field0,
      ),
      frb.BridgeLspStateSnapshot_Stopped(:final field0) => _stoppedResource(
        field0,
      ),
    },
  );
}

SkillsStateSnapshot _skillsStateFromFrb(
  frb.BridgeSkillsStateSnapshot snapshot,
) {
  SkillsStateData convert(frb.BridgeSkillsStateData data) => SkillsStateData(
    configFingerprint: data.configFingerprint,
    catalogRevision: data.catalogRevision.toInt(),
    skills: data.skills.map((skill) => skill.name).toList(),
    warnings: data.warnings,
  );
  return SkillsStateSnapshot.fromState(
    projectId: snapshot.projectId,
    state: switch (snapshot.state) {
      frb.BridgeSkillsResourceState_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeSkillsResourceState_Loading(:final field0) => _loadingResource(
        field0,
      ),
      frb.BridgeSkillsResourceState_Ready(:final resource, :final value) =>
        _readyResource(resource, convert(value)),
      frb.BridgeSkillsResourceState_Refreshing(:final resource, :final value) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeSkillsResourceState_Stale(:final resource, :final value) =>
        _staleResource(resource, convert(value)),
      frb.BridgeSkillsResourceState_Degraded(:final resource, :final value) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeSkillsResourceState_Failed(:final field0) => _failedResource(
        field0,
      ),
      frb.BridgeSkillsResourceState_Stopped(:final field0) => _stoppedResource(
        field0,
      ),
    },
  );
}

ProviderUsageStateSnapshot _providerUsageStateFromFrb(
  frb.BridgeProviderUsageStateSnapshot snapshot,
) {
  ProviderUsageStateData convert(frb.BridgeProviderUsageStateData data) =>
      ProviderUsageStateData(
        configFingerprint: data.configFingerprint,
        usages: data.usages.map(_providerUsageFromFrb).toList(),
      );
  return ProviderUsageStateSnapshot.fromState(
    state: switch (snapshot) {
      frb.BridgeProviderUsageStateSnapshot_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeProviderUsageStateSnapshot_Loading(:final field0) =>
        _loadingResource(field0),
      frb.BridgeProviderUsageStateSnapshot_Ready(
        :final resource,
        :final value,
      ) =>
        _readyResource(resource, convert(value)),
      frb.BridgeProviderUsageStateSnapshot_Refreshing(
        :final resource,
        :final value,
      ) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeProviderUsageStateSnapshot_Stale(
        :final resource,
        :final value,
      ) =>
        _staleResource(resource, convert(value)),
      frb.BridgeProviderUsageStateSnapshot_Degraded(
        :final resource,
        :final value,
      ) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeProviderUsageStateSnapshot_Failed(:final field0) =>
        _failedResource(field0),
      frb.BridgeProviderUsageStateSnapshot_Stopped(:final field0) =>
        _stoppedResource(field0),
    },
  );
}

UpdaterStateSnapshot updaterStateFromFrb(
  frb.BridgeUpdaterStateSnapshot snapshot,
) => switch (snapshot) {
  frb.BridgeUpdaterStateSnapshot_Disabled(:final field0) =>
    DisabledUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      updatedAt: _dateFromUnix(field0.updatedAt),
    ),
  frb.BridgeUpdaterStateSnapshot_Idle(:final field0) =>
    IdleUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      updatedAt: _dateFromUnix(field0.updatedAt),
    ),
  frb.BridgeUpdaterStateSnapshot_Checking(:final field0) =>
    CheckingUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      operationId: field0.operationId,
      startedAt: _dateFromUnix(field0.startedAt),
    ),
  frb.BridgeUpdaterStateSnapshot_UpToDate(:final field0) =>
    UpToDateUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      checkedAt: _dateFromUnix(field0.checkedAt),
    ),
  frb.BridgeUpdaterStateSnapshot_Available(:final field0) =>
    AvailableUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      checkedAt: _dateFromUnix(field0.checkedAt),
      update: _updateInfoFromFrb(field0.update),
    ),
  frb.BridgeUpdaterStateSnapshot_Downloading(:final field0) =>
    DownloadingUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      updatedAt: _dateFromUnix(field0.updatedAt),
      update: _updateInfoFromFrb(field0.update),
      downloaded: field0.downloaded.toInt(),
      total: field0.total.toInt(),
    ),
  frb.BridgeUpdaterStateSnapshot_Verifying(:final field0) =>
    VerifyingUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      updatedAt: _dateFromUnix(field0.updatedAt),
      update: _updateInfoFromFrb(field0.update),
      downloaded: field0.downloaded.toInt(),
      total: field0.total.toInt(),
    ),
  frb.BridgeUpdaterStateSnapshot_InstallerLaunched(:final field0) =>
    InstallerLaunchedUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      launchedAt: _dateFromUnix(field0.launchedAt),
      update: _updateInfoFromFrb(field0.update),
    ),
  frb.BridgeUpdaterStateSnapshot_CheckFailed(:final field0) =>
    CheckFailedUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      failedAt: _dateFromUnix(field0.failedAt),
      error: _updaterErrorFromFrb(field0.error),
    ),
  frb.BridgeUpdaterStateSnapshot_InstallFailed(:final field0) =>
    InstallFailedUpdaterStateSnapshot(
      revision: field0.revision.toInt(),
      failedAt: _dateFromUnix(field0.failedAt),
      update: _updateInfoFromFrb(field0.update),
      error: _updaterErrorFromFrb(field0.error),
    ),
};

StudioUpdateInfoView _updateInfoFromFrb(
  frb.BridgeVerifiedUpdateSummary update,
) => StudioUpdateInfoView(
  version: update.version,
  publishedAt: _dateFromUnix(update.publishedAt),
  notesUrl: update.notesUrl,
);

UpdaterErrorView _updaterErrorFromFrb(frb.BridgeStateError error) =>
    UpdaterErrorView(
      code: error.code,
      message: error.message,
      retryable: error.retryable,
    );

StudioRecoveryIssue _recoveryIssueFromFrb(
  frb.BridgeStudioRecoveryIssueDto issue,
) {
  return StudioRecoveryIssue(
    id: issue.id,
    scope: switch (issue.scope) {
      frb.BridgeRecoveryIssueScope.application =>
        RecoveryIssueScope.application,
      frb.BridgeRecoveryIssueScope.project => RecoveryIssueScope.project,
      frb.BridgeRecoveryIssueScope.thread => RecoveryIssueScope.thread,
    },
    category: switch (issue.category) {
      frb.BridgeRecoveryIssueCategory.processLease =>
        RecoveryIssueCategory.processLease,
      frb.BridgeRecoveryIssueCategory.agentState =>
        RecoveryIssueCategory.agentState,
      frb.BridgeRecoveryIssueCategory.worktree =>
        RecoveryIssueCategory.worktree,
      frb.BridgeRecoveryIssueCategory.repository =>
        RecoveryIssueCategory.repository,
      frb.BridgeRecoveryIssueCategory.merge => RecoveryIssueCategory.merge,
      frb.BridgeRecoveryIssueCategory.conflict =>
        RecoveryIssueCategory.conflict,
    },
    availableActions: [
      for (final action in issue.availableActions)
        switch (action) {
          frb.BridgeRecoveryIssueAction.retry => RecoveryIssueAction.retry,
          frb.BridgeRecoveryIssueAction.cleanupThread =>
            RecoveryIssueAction.cleanupThread,
          frb.BridgeRecoveryIssueAction.removeProject =>
            RecoveryIssueAction.removeProject,
        },
    ],
    projectId: issue.projectId,
    threadId: issue.threadId,
    taskRunId: issue.taskRunId,
    detail: issue.detail,
  );
}

RecoveryCleanupPreview _recoveryCleanupPreviewFromFrb(
  frb.BridgeRecoveryCleanupPreviewDto preview,
) {
  return RecoveryCleanupPreview(
    issueId: preview.issueId,
    expectedRevision: preview.expectedRevision,
    scope: switch (preview.scope) {
      frb.BridgeRecoveryIssueScope.application =>
        RecoveryIssueScope.application,
      frb.BridgeRecoveryIssueScope.project => RecoveryIssueScope.project,
      frb.BridgeRecoveryIssueScope.thread => RecoveryIssueScope.thread,
    },
    projectId: preview.projectId,
    threadId: preview.threadId,
    detail: preview.detail,
    resources: [
      for (final resource in preview.resources)
        RecoveryCleanupResource(
          workUnitId: resource.workUnitId,
          path: resource.path,
          branch: resource.branch,
          presence: switch (resource.presence) {
            frb.BridgeRecoveryResourcePresence.absent =>
              RecoveryResourcePresence.absent,
            frb.BridgeRecoveryResourcePresence.complete =>
              RecoveryResourcePresence.complete,
            frb.BridgeRecoveryResourcePresence.partial =>
              RecoveryResourcePresence.partial,
          },
          registrationExists: resource.registrationExists,
          pathExists: resource.pathExists,
          branchExists: resource.branchExists,
          branchHead: resource.branchHead,
          dirty: resource.dirty,
          aheadBy: resource.aheadBy.toInt(),
          changedFileCount: resource.changedFileCount.toInt(),
        ),
    ],
  );
}

TaskRecoveryPreview _taskRecoveryPreviewFromFrb(
  frb.BridgeTaskRecoveryPreviewDto preview,
) {
  return TaskRecoveryPreview(
    previewToken: preview.previewToken,
    rootThreadId: preview.rootThreadId,
    runId: preview.runId,
    revision: preview.revision.toInt(),
    taskGeneration: preview.taskGeneration.toInt(),
    state: switch (preview.state) {
      frb.BridgeTaskRecoveryState.designUpdating =>
        TaskStateKind.designUpdating,
      frb.BridgeTaskRecoveryState.implementing => TaskStateKind.implementing,
      frb.BridgeTaskRecoveryState.merging => TaskStateKind.merging,
      frb.BridgeTaskRecoveryState.reviewing => TaskStateKind.reviewing,
      frb.BridgeTaskRecoveryState.reworking => TaskStateKind.reworking,
      frb.BridgeTaskRecoveryState.stopping => TaskStateKind.stopping,
      frb.BridgeTaskRecoveryState.blocked => TaskStateKind.blocked,
      frb.BridgeTaskRecoveryState.completed => TaskStateKind.completed,
      frb.BridgeTaskRecoveryState.failed => TaskStateKind.failed,
      frb.BridgeTaskRecoveryState.cancelled => TaskStateKind.cancelled,
    },
    stopRequested: preview.stopRequested,
    projectLeaseId: preview.projectLeaseId,
    recommendedThreadId: preview.recommendedThreadId,
    targets: preview.targets.map(_taskRecoveryTargetFromFrb).toList(),
    completionRevisionFingerprint: preview.completionRevisionFingerprint,
    reviewRevisionFingerprint: preview.reviewRevisionFingerprint,
    mergeRevisionFingerprint: preview.mergeRevisionFingerprint,
  );
}

TaskRecoveryTarget _taskRecoveryTargetFromFrb(
  frb.BridgeTaskRecoveryTargetDto target,
) {
  return TaskRecoveryTarget(
    threadId: target.threadId,
    kind: switch (target.kind) {
      frb.BridgeTaskRecoveryTargetKind.planner =>
        TaskRecoveryTargetKind.planner,
      frb.BridgeTaskRecoveryTargetKind.executor =>
        TaskRecoveryTargetKind.executor,
    },
    workUnitId: target.workUnitId,
    attempt: target.attempt,
    continuationRevision: target.continuationRevision?.toInt(),
    expectedRuntimeRevision: target.expectedRuntimeRevision.toInt(),
    expectedThreadRevision: target.expectedThreadRevision.toInt(),
    branch: target.branch,
    worktreePath: target.worktreePath,
    baseCommit: target.baseCommit,
    turns: [
      for (final turn in target.turns)
        TaskRecoveryTurn(
          turnId: turn.turnId,
          state: switch (turn.state) {
            frb.BridgeTaskRecoveryTurnState.completed =>
              TaskRecoveryTurnState.completed,
            frb.BridgeTaskRecoveryTurnState.cancelled =>
              TaskRecoveryTurnState.cancelled,
            frb.BridgeTaskRecoveryTurnState.failed =>
              TaskRecoveryTurnState.failed,
            frb.BridgeTaskRecoveryTurnState.budgetLimited =>
              TaskRecoveryTurnState.budgetLimited,
          },
          updatedAt: _dateFromUnix(turn.updatedAt),
          itemCount: turn.itemCount.toInt(),
          inputCount: turn.inputCount.toInt(),
          toolCount: turn.toolCount.toInt(),
          toolSummaries: turn.toolSummaries,
        ),
    ],
    defaultTurnIds: target.defaultTurnIds,
    availableModes: target.availableModes
        .map(_conversationRecoveryModeFromFrb)
        .toList(),
  );
}

frb.BridgeTaskRecoveryRequestDto _taskRecoveryRequestToFrb(
  TaskRecoveryRequest request,
) {
  return frb.BridgeTaskRecoveryRequestDto(
    recoveryId: request.recoveryId,
    rootThreadId: request.rootThreadId,
    targetThreadId: request.targetThreadId,
    mode: _conversationRecoveryModeToFrb(request.mode),
    turnIds: request.turnIds,
    preview: _taskRecoveryPreviewToFrb(request.preview),
  );
}

frb.BridgeTaskRecoveryPreviewDto _taskRecoveryPreviewToFrb(
  TaskRecoveryPreview preview,
) {
  return frb.BridgeTaskRecoveryPreviewDto(
    previewToken: preview.previewToken,
    rootThreadId: preview.rootThreadId,
    runId: preview.runId,
    revision: BigInt.from(preview.revision),
    taskGeneration: BigInt.from(preview.taskGeneration),
    state: switch (preview.state) {
      TaskStateKind.designUpdating =>
        frb.BridgeTaskRecoveryState.designUpdating,
      TaskStateKind.implementing => frb.BridgeTaskRecoveryState.implementing,
      TaskStateKind.merging => frb.BridgeTaskRecoveryState.merging,
      TaskStateKind.reviewing => frb.BridgeTaskRecoveryState.reviewing,
      TaskStateKind.reworking => frb.BridgeTaskRecoveryState.reworking,
      TaskStateKind.stopping => frb.BridgeTaskRecoveryState.stopping,
      TaskStateKind.blocked => frb.BridgeTaskRecoveryState.blocked,
      TaskStateKind.completed => frb.BridgeTaskRecoveryState.completed,
      TaskStateKind.failed => frb.BridgeTaskRecoveryState.failed,
      TaskStateKind.cancelled => frb.BridgeTaskRecoveryState.cancelled,
    },
    stopRequested: preview.stopRequested,
    projectLeaseId: preview.projectLeaseId,
    recommendedThreadId: preview.recommendedThreadId,
    targets: [
      for (final target in preview.targets)
        frb.BridgeTaskRecoveryTargetDto(
          threadId: target.threadId,
          kind: switch (target.kind) {
            TaskRecoveryTargetKind.planner =>
              frb.BridgeTaskRecoveryTargetKind.planner,
            TaskRecoveryTargetKind.executor =>
              frb.BridgeTaskRecoveryTargetKind.executor,
          },
          workUnitId: target.workUnitId,
          attempt: target.attempt,
          continuationRevision: target.continuationRevision == null
              ? null
              : BigInt.from(target.continuationRevision!),
          expectedRuntimeRevision: BigInt.from(target.expectedRuntimeRevision),
          expectedThreadRevision: BigInt.from(target.expectedThreadRevision),
          branch: target.branch,
          worktreePath: target.worktreePath,
          baseCommit: target.baseCommit,
          turns: [
            for (final turn in target.turns)
              frb.BridgeTaskRecoveryTurnDto(
                turnId: turn.turnId,
                state: switch (turn.state) {
                  TaskRecoveryTurnState.completed =>
                    frb.BridgeTaskRecoveryTurnState.completed,
                  TaskRecoveryTurnState.cancelled =>
                    frb.BridgeTaskRecoveryTurnState.cancelled,
                  TaskRecoveryTurnState.failed =>
                    frb.BridgeTaskRecoveryTurnState.failed,
                  TaskRecoveryTurnState.budgetLimited =>
                    frb.BridgeTaskRecoveryTurnState.budgetLimited,
                },
                updatedAt: turn.updatedAt.millisecondsSinceEpoch ~/ 1000,
                itemCount: BigInt.from(turn.itemCount),
                inputCount: BigInt.from(turn.inputCount),
                toolCount: BigInt.from(turn.toolCount),
                toolSummaries: turn.toolSummaries,
              ),
          ],
          defaultTurnIds: target.defaultTurnIds,
          availableModes: target.availableModes
              .map(_conversationRecoveryModeToFrb)
              .toList(),
        ),
    ],
    completionRevisionFingerprint: preview.completionRevisionFingerprint,
    reviewRevisionFingerprint: preview.reviewRevisionFingerprint,
    mergeRevisionFingerprint: preview.mergeRevisionFingerprint,
  );
}

TaskRecoveryResult _taskRecoveryResultFromFrb(
  frb.BridgeTaskRecoveryResultDto result,
) {
  return TaskRecoveryResult(
    recoveryId: result.recoveryId,
    runId: result.runId,
    workUnitId: result.workUnitId,
    rootThreadId: result.rootThreadId,
    targetThreadId: result.targetThreadId,
    mode: _conversationRecoveryModeFromFrb(result.mode),
    recoveryRevision: result.recoveryRevision.toInt(),
    runtimeRevision: result.runtimeRevision.toInt(),
    threadRevision: result.threadRevision.toInt(),
    beforeTranscriptHash: result.beforeTranscriptHash,
    afterTranscriptHash: result.afterTranscriptHash,
    removedItemCount: result.removedItemCount.toInt(),
    removedInputCount: result.removedInputCount.toInt(),
    stopCleared: result.stopCleared,
    resumeTurnId: result.resumeTurnId,
  );
}

ConversationRecoveryMode _conversationRecoveryModeFromFrb(
  frb.BridgeConversationRecoveryMode mode,
) {
  return switch (mode) {
    frb.BridgeConversationRecoveryMode.rewindTail =>
      ConversationRecoveryMode.rewindTail,
    frb.BridgeConversationRecoveryMode.rebuildThread =>
      ConversationRecoveryMode.rebuildThread,
  };
}

frb.BridgeConversationRecoveryMode _conversationRecoveryModeToFrb(
  ConversationRecoveryMode mode,
) {
  return switch (mode) {
    ConversationRecoveryMode.rewindTail =>
      frb.BridgeConversationRecoveryMode.rewindTail,
    ConversationRecoveryMode.rebuildThread =>
      frb.BridgeConversationRecoveryMode.rebuildThread,
  };
}

ProviderUsageView _providerUsageFromFrb(frb.ProviderUsageDto usage) {
  return ProviderUsageView(
    providerId: usage.providerId,
    revision: usage.revision.toInt(),
    updatedAt: _frbInt(usage.updatedAt),
    state: switch (usage.state) {
      frb.BridgeProviderUsageState_Unsupported() =>
        const UnsupportedProviderUsageView(),
      frb.BridgeProviderUsageState_MissingCredential(:final message) =>
        MissingCredentialProviderUsageView(message: message),
      frb.BridgeProviderUsageState_Failed(:final error) =>
        FailedProviderUsageView(
          code: error.code,
          message: error.message,
          retryable: error.retryable,
        ),
      frb.BridgeProviderUsageState_Ready(:final data) => ReadyProviderUsageView(
        data: switch (data) {
          frb.BridgeProviderUsageData_DeepSeekBalance(:final field0) =>
            DeepSeekBalanceProviderUsageView(
              balance: DeepSeekBalanceUsageView(
                isAvailable: field0.isAvailable,
                balances: field0.balances
                    .map(
                      (item) => DeepSeekBalanceInfoView(
                        currency: item.currency,
                        totalBalance: item.totalBalance,
                        grantedBalance: item.grantedBalance,
                        toppedUpBalance: item.toppedUpBalance,
                      ),
                    )
                    .where((item) => item.currency.isNotEmpty)
                    .toList(),
              ),
            ),
          frb.BridgeProviderUsageData_ZhipuCodingPlan(:final field0) =>
            ZhipuCodingPlanProviderUsageView(
              codingPlan: ZhipuCodingPlanUsageView(
                level: field0.level,
                limits: field0.limits
                    .map(
                      (item) => ZhipuQuotaLimitView(
                        window: item.window.isEmpty ? 'other' : item.window,
                        label: item.label,
                        percentage: item.percentage,
                        currentValue: item.currentValue,
                        total: item.total,
                        remaining: item.remaining,
                        nextResetAt: _frbNullableInt(item.nextResetAt),
                        usageDetails: item.usageDetails
                            .map(
                              (detail) => ZhipuToolUsageDetailView(
                                name: detail.name,
                                currentValue: detail.currentValue,
                                total: detail.total,
                                percentage: detail.percentage,
                              ),
                            )
                            .toList(),
                      ),
                    )
                    .toList(),
              ),
            ),
        },
      ),
    },
  );
}
