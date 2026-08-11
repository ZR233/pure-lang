part of 'studio_api.dart';

StudioBridgeEventPayload _productPayloadFromFrb(
  frb.BridgeProductEventPayload payload,
) {
  return switch (payload) {
    frb.BridgeProductEventPayload_ThreadDirectoryChanged(
      :final projectId,
      :final threads,
    ) =>
      ThreadDirectoryChangedPayload(
        projectId: _emptyToNull(projectId),
        threads: threads.map(_threadFromFrb).toList(),
      ),
    frb.BridgeProductEventPayload_McpHealthChanged(:final health) =>
      McpHealthChangedPayload(
        activeMcpServers: health.activeMcpServers,
        servers: health.mcpServers.map(_mcpServerFromFrb).toList(),
      ),
    frb.BridgeProductEventPayload_LspHealthChanged(:final health) =>
      LspHealthChangedPayload(activeLspServers: health.activeLspServers),
    frb.BridgeProductEventPayload_TaskChanged(
      :final rootThreadId,
      :final task,
    ) =>
      TaskChangedPayload(
        rootThreadId: rootThreadId,
        task: task == null ? null : _taskRuntimeFromFrb(task),
      ),
    frb.BridgeProductEventPayload_AgentDirectoryChanged(
      :final rootThreadId,
      :final agent,
    ) =>
      AgentDirectoryChangedPayload(
        rootThreadId: rootThreadId,
        agent: _agentDirectoryEntryFromFrb(agent),
      ),
    frb.BridgeProductEventPayload_Stale(:final laggedEvents) => StalePayload(
      laggedEvents: laggedEvents.toInt(),
    ),
  };
}

TaskRuntimeView _taskRuntimeFromFrb(frb.BridgeTaskRuntimeDto task) {
  return TaskRuntimeView(
    runId: task.runId,
    phase: task.phase,
    branch: task.branch,
    expectedHead: task.expectedHead,
    statusMessage: task.statusMessage,
    stopRequestedOrigin: task.stopRequestedOrigin,
    stopRequestedReason: task.stopRequestedReason,
    taskGeneration: task.taskGeneration.toInt(),
    failures: task.failures.map(_taskFailureFromFrb).toList(),
    terminalFailure: task.terminalFailure == null
        ? null
        : _taskFailureFromFrb(task.terminalFailure!),
    workUnits: [
      for (final unit in task.workUnits)
        TaskWorkUnitView(
          id: unit.id,
          title: unit.title,
          status: unit.status,
          worktreePath: unit.worktreePath,
          branch: unit.branch,
          agentId: unit.agentId,
          executionStatus: unit.executionStatus,
          executionError: unit.executionError,
          budgetLimit: unit.budgetLimit == null
              ? null
              : TaskBudgetLimitView(
                  kind: unit.budgetLimit!.kind,
                  usage: TaskBudgetUsageView(
                    modelSteps: unit.budgetLimit!.usage.modelSteps,
                    toolCalls: unit.budgetLimit!.usage.toolCalls,
                    waitCalls: unit.budgetLimit!.usage.waitCalls,
                    elapsedMs: unit.budgetLimit!.usage.elapsedMs,
                  ),
                ),
          budgetSliceCount: unit.budgetSliceCount,
          budgetSliceLimit: unit.budgetSliceLimit,
          continuationState: unit.continuationState,
          continuationSourceTurnId: unit.continuationSourceTurnId,
          continuationRevision: unit.continuationRevision,
          executorProgressRevision: unit.executorProgressRevision,
        ),
    ],
    completions: [
      for (final completion in task.completions)
        TaskCompletionView(
          id: completion.id,
          workUnitId: completion.workUnitId,
          executorAgentId: completion.executorAgentId,
          revision: completion.revision,
          kind: completion.kind,
          status: completion.status,
          baseCommit: completion.baseCommit,
          headCommit: completion.headCommit,
          changedFiles: completion.changedFiles,
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
          method: merge.method,
          summary: merge.summary,
          cleanupStatus: merge.cleanupStatus,
          cleanupDetail: merge.cleanupDetail,
          createdAt: _dateFromUnix(merge.createdAt),
          updatedAt: _dateFromUnix(merge.updatedAt),
        ),
    ],
    reviews: [
      for (final review in task.reviews)
        TaskReviewView(
          id: review.id,
          round: review.round,
          scope: review.scope,
          workUnitId: review.workUnitId,
          completionId: review.completionId,
          completionRevision: review.completionRevision,
          reviewedHead: review.reviewedHead,
          verdict: review.verdict,
          requestedByCallId: review.requestedByCallId,
          reviewerAgentId: review.reviewerAgentId,
          summary: review.summary,
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

TaskFailureView _taskFailureFromFrb(frb.BridgeTaskFailureDto failure) {
  return TaskFailureView(
    id: failure.id,
    sourceThreadId: failure.sourceThreadId,
    sourceTurnId: failure.sourceTurnId,
    sourceAgentId: failure.sourceAgentId,
    sourceRole: failure.sourceRole,
    workUnitId: failure.workUnitId,
    reviewRoundId: failure.reviewRoundId,
    disposition: failure.disposition,
    category: failure.category,
    providerKind: failure.providerKind,
    code: failure.code,
    httpStatus: failure.httpStatus,
    message: failure.message,
    retryable: failure.retryable,
    resolvedAt: failure.resolvedAt == null
        ? null
        : _dateFromUnix(failure.resolvedAt!),
    createdAt: _dateFromUnix(failure.createdAt),
  );
}

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
    status: agent.status,
    summary: agent.summary,
    depth: agent.depth,
    error: agent.error,
    reason: agent.reason,
    lifecycle: agent.lifecycle,
    activity: _agentActivityFromFrb(agent.activity),
    progress: _agentProgressFromFrb(agent.progress),
    updatedAt: _dateFromUnix(agent.updatedAt),
    summaryAgeSeconds: agent.summaryAgeSeconds.toInt(),
  );
}

StudioAgentActivity _agentActivityFromFrb(frb.BridgeAgentActivity activity) {
  return switch (activity) {
    frb.BridgeAgentActivity.idle => StudioAgentActivity.idle,
    frb.BridgeAgentActivity.queued => StudioAgentActivity.queued,
    frb.BridgeAgentActivity.activeRunning => StudioAgentActivity.activeRunning,
    frb.BridgeAgentActivity.activeWaitingTool =>
      StudioAgentActivity.activeWaitingTool,
    frb.BridgeAgentActivity.activeWaitingInteraction =>
      StudioAgentActivity.activeWaitingInteraction,
    frb.BridgeAgentActivity.cancelling => StudioAgentActivity.cancelling,
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
    endpoint: server.endpoint.isNotEmpty
        ? server.endpoint
        : (server.url ?? server.command ?? ''),
    enabled: server.enabled,
    status: server.statusKind.isEmpty
        ? server.availabilityKind
        : server.statusKind,
    sourceKind: server.sourceKind,
    mutationPolicy: server.mutationPolicy,
  );
}

String? _emptyToNull(String value) {
  return value.isEmpty ? null : value;
}

StudioState studioStateFromFrbSnapshot(frb.BridgeStudioSnapshotResponse value) {
  return _stateFromTypedSnapshot(
    projects: value.projects.map(_projectFromFrb).toList(),
    threads: value.threads.map(_threadFromFrb).toList(),
    selectedProjectId: value.selectedProjectId,
    selectedThreadId: value.selectedThreadId,
    recoveryIssues: value.recoveryIssues.map(_recoveryIssueFromFrb).toList(),
    selectedTask: value.selectedThreadTask == null
        ? null
        : _taskRuntimeFromFrb(value.selectedThreadTask!),
    settings: value.settings,
  );
}

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
    taskGeneration: preview.taskGeneration.toInt(),
    phase: preview.phase,
    expectedHead: preview.expectedHead,
    stopRequested: preview.stopRequested,
    branchLeaseId: preview.branchLeaseId,
    branchLeaseBranch: preview.branchLeaseBranch,
    branchLeaseGitCommonDir: preview.branchLeaseGitCommonDir,
    branchLeaseExpectedHead: preview.branchLeaseExpectedHead,
    recommendedThreadId: preview.recommendedThreadId,
    targets: preview.targets.map(_taskRecoveryTargetFromFrb).toList(),
    mainGitFingerprint: _taskGitFingerprintFromFrb(preview.mainGitFingerprint),
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
    turns: [
      for (final turn in target.turns)
        TaskRecoveryTurn(
          turnId: turn.turnId,
          status: turn.status,
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
    gitFingerprint: _taskGitFingerprintFromFrb(target.gitFingerprint),
  );
}

TaskGitFingerprint _taskGitFingerprintFromFrb(
  frb.BridgeTaskGitFingerprintDto fingerprint,
) {
  return TaskGitFingerprint(
    workspaceRoot: fingerprint.workspaceRoot,
    gitCommonDir: fingerprint.gitCommonDir,
    branch: fingerprint.branch,
    head: fingerprint.head,
    baseCommit: fingerprint.baseCommit,
    expectedHead: fingerprint.expectedHead,
    operation: fingerprint.operation,
    indexDiffHash: fingerprint.indexDiffHash,
    workingTreeDiffHash: fingerprint.workingTreeDiffHash,
    untrackedContentHash: fingerprint.untrackedContentHash,
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
    taskGeneration: BigInt.from(preview.taskGeneration),
    phase: preview.phase,
    expectedHead: preview.expectedHead,
    stopRequested: preview.stopRequested,
    branchLeaseId: preview.branchLeaseId,
    branchLeaseBranch: preview.branchLeaseBranch,
    branchLeaseGitCommonDir: preview.branchLeaseGitCommonDir,
    branchLeaseExpectedHead: preview.branchLeaseExpectedHead,
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
          turns: [
            for (final turn in target.turns)
              frb.BridgeTaskRecoveryTurnDto(
                turnId: turn.turnId,
                status: turn.status,
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
          gitFingerprint: _taskGitFingerprintToFrb(target.gitFingerprint),
        ),
    ],
    mainGitFingerprint: _taskGitFingerprintToFrb(preview.mainGitFingerprint),
    completionRevisionFingerprint: preview.completionRevisionFingerprint,
    reviewRevisionFingerprint: preview.reviewRevisionFingerprint,
    mergeRevisionFingerprint: preview.mergeRevisionFingerprint,
  );
}

frb.BridgeTaskGitFingerprintDto _taskGitFingerprintToFrb(
  TaskGitFingerprint fingerprint,
) {
  return frb.BridgeTaskGitFingerprintDto(
    workspaceRoot: fingerprint.workspaceRoot,
    gitCommonDir: fingerprint.gitCommonDir,
    branch: fingerprint.branch,
    head: fingerprint.head,
    baseCommit: fingerprint.baseCommit,
    expectedHead: fingerprint.expectedHead,
    operation: fingerprint.operation,
    indexDiffHash: fingerprint.indexDiffHash,
    workingTreeDiffHash: fingerprint.workingTreeDiffHash,
    untrackedContentHash: fingerprint.untrackedContentHash,
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
    gitFingerprint: _taskGitFingerprintFromFrb(result.gitFingerprint),
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
    updatedAt: _frbInt(usage.updatedAt),
    status: usage.status.isEmpty ? 'unknown' : usage.status,
    usageKind: usage.usageKind.isEmpty ? 'unknown' : usage.usageKind,
    message: usage.message,
    balance: usage.balance == null
        ? null
        : DeepSeekBalanceUsageView(
            isAvailable: usage.balance!.isAvailable,
            balances: usage.balance!.balances
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
    codingPlan: usage.codingPlan == null
        ? null
        : ZhipuCodingPlanUsageView(
            level: usage.codingPlan!.level,
            limits: usage.codingPlan!.limits
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
  );
}
