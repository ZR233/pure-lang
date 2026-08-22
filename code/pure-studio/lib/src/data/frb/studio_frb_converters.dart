part of 'studio_api.dart';

StudioBridgeEventPayload _productPayloadFromFrb(
  frb.BridgeProductEventPayload payload,
) {
  return switch (payload) {
    frb.BridgeProductEventPayload_ProjectDirectoryChanged(:final field0) =>
      ProjectDirectoryChangedPayload(
        ProjectDirectoryState(
          meta: _observedMetaFromFrb(field0.meta),
          values: field0.projects.map(_projectFromFrb).toList(),
        ),
      ),
    frb.BridgeProductEventPayload_ThreadDirectoryChanged(:final field0) =>
      ThreadDirectoryChangedPayload(
        upserted: field0.upserted.map(_threadFromFrb).toList(),
        removed: field0.removed.toList(),
      ),
    frb.BridgeProductEventPayload_TaskDirectoryChanged(:final field0) =>
      TaskDirectoryChangedPayload(
        TaskDirectoryState(
          meta: _observedMetaFromFrb(field0.meta),
          values: [
            for (final entry in field0.tasks)
              TaskDirectoryEntryView(
                rootThreadId: entry.rootThreadId,
                task: _taskRuntimeFromFrb(entry.task),
              ),
          ],
        ),
      ),
    frb.BridgeProductEventPayload_AgentDirectoryChanged(:final field0) =>
      AgentDirectoryChangedPayload(
        AgentDirectoryState(
          meta: _observedMetaFromFrb(field0.meta),
          values: field0.agents.map(_agentDirectoryEntryFromFrb).toList(),
        ),
      ),
    frb.BridgeProductEventPayload_SettingsStateChanged(:final field0) =>
      SettingsStateChangedPayload(_settingsStateFromFrb(field0)),
    frb.BridgeProductEventPayload_RecoveryStateChanged(:final field0) =>
      RecoveryStateChangedPayload(
        RecoveryStateSnapshot(
          meta: _observedMetaFromFrb(field0.meta),
          values: field0.issues.map(_recoveryIssueFromFrb).toList(),
        ),
      ),
    frb.BridgeProductEventPayload_McpStateChanged(:final field0) =>
      McpStateChangedPayload(_mcpStateFromFrb(field0)),
    frb.BridgeProductEventPayload_LspStateChanged(:final field0) =>
      LspStateChangedPayload(_lspStateFromFrb(field0)),
    frb.BridgeProductEventPayload_SkillsStateChanged(:final field0) =>
      SkillsStateChangedPayload(_skillsStateFromFrb(field0)),
    frb.BridgeProductEventPayload_ProviderUsageStateChanged(:final field0) =>
      ProviderUsageStateChangedPayload(_providerUsageStateFromFrb(field0)),
    frb.BridgeProductEventPayload_UpdaterStateChanged(:final field0) =>
      UpdaterStateChangedPayload(_updaterStateFromFrb(field0)),
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
          state: _taskReviewStateFromFrb(review.state),
          requestedByCallId: review.requestedByCallId,
          reviewerAgentId: review.reviewerAgentId,
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

TaskStateView _taskStateFromFrb(frb.BridgeTaskState state) {
  return switch (state) {
    frb.BridgeTaskState_DesignUpdating(:final field0) =>
      DesignUpdatingTaskStateView(_taskStateDataFromFrb(field0)),
    frb.BridgeTaskState_Implementing(:final field0) =>
      ImplementingTaskStateView(_taskStateDataFromFrb(field0)),
    frb.BridgeTaskState_Merging(:final field0) => MergingTaskStateView(
      _taskStateDataFromFrb(field0),
    ),
    frb.BridgeTaskState_Reviewing(:final field0) => ReviewingTaskStateView(
      _taskStateDataFromFrb(field0),
    ),
    frb.BridgeTaskState_Reworking(:final field0) => ReworkingTaskStateView(
      _taskStateDataFromFrb(field0),
    ),
    frb.BridgeTaskState_Stopping(:final field0) => StoppingTaskStateView(
      _taskStateDataFromFrb(field0),
    ),
    frb.BridgeTaskState_Blocked(:final field0) => BlockedTaskStateView(
      _taskStateDataFromFrb(field0),
    ),
    frb.BridgeTaskState_Completed(:final field0) => CompletedTaskStateView(
      _taskStateDataFromFrb(field0),
    ),
    frb.BridgeTaskState_Failed(:final field0) => FailedTaskStateView(
      _taskStateDataFromFrb(field0),
    ),
    frb.BridgeTaskState_Cancelled(:final field0) => CancelledTaskStateView(
      _taskStateDataFromFrb(field0),
    ),
  };
}

TaskStateDataView _taskStateDataFromFrb(frb.BridgeTaskStateData data) {
  return TaskStateDataView(
    generation: data.generation.toInt(),
    statusMessage: data.statusMessage,
    stopRequestedOrigin: data.stopRequest?.origin,
    stopRequestedReason: data.stopRequest?.reason,
  );
}

TaskWorkUnitStateView _taskWorkUnitStateFromFrb(
  frb.BridgeTaskWorkUnitState state,
) {
  return switch (state) {
    frb.BridgeTaskWorkUnitState_Pending(:final field0) => _taskWorkUnitState(
      TaskWorkUnitStateKind.pending,
      TaskWorkUnitExecution.queued,
      field0,
    ),
    frb.BridgeTaskWorkUnitState_Running(:final field0) => _taskWorkUnitState(
      TaskWorkUnitStateKind.running,
      switch (field0.execution) {
        frb.BridgeRunningExecution.running => TaskWorkUnitExecution.running,
        frb.BridgeRunningExecution.budgetLimited =>
          TaskWorkUnitExecution.budgetLimited,
      },
      field0.progress,
    ),
    frb.BridgeTaskWorkUnitState_AwaitingCompletion(:final field0) =>
      _taskWorkUnitState(
        TaskWorkUnitStateKind.awaitingCompletion,
        switch (field0.execution) {
          frb.BridgeAwaitingExecution.completed =>
            TaskWorkUnitExecution.completed,
          frb.BridgeAwaitingExecution.failed => TaskWorkUnitExecution.failed,
          frb.BridgeAwaitingExecution.cancelled =>
            TaskWorkUnitExecution.cancelled,
        },
        field0.progress,
      ),
    frb.BridgeTaskWorkUnitState_ReadyForReview(:final field0) =>
      _taskWorkUnitState(
        TaskWorkUnitStateKind.readyForReview,
        TaskWorkUnitExecution.completed,
        field0,
      ),
    frb.BridgeTaskWorkUnitState_Reviewing(:final field0) => _taskWorkUnitState(
      TaskWorkUnitStateKind.reviewing,
      TaskWorkUnitExecution.completed,
      field0,
    ),
    frb.BridgeTaskWorkUnitState_ChangesRequested(:final field0) =>
      _taskWorkUnitState(
        TaskWorkUnitStateKind.changesRequested,
        TaskWorkUnitExecution.completed,
        field0,
      ),
    frb.BridgeTaskWorkUnitState_Approved(:final field0) => _taskWorkUnitState(
      TaskWorkUnitStateKind.approved,
      TaskWorkUnitExecution.completed,
      field0,
    ),
    frb.BridgeTaskWorkUnitState_Merged(:final field0) => _taskWorkUnitState(
      TaskWorkUnitStateKind.merged,
      TaskWorkUnitExecution.completed,
      field0,
    ),
    frb.BridgeTaskWorkUnitState_NoDelivery(:final field0) => _taskWorkUnitState(
      TaskWorkUnitStateKind.noDelivery,
      TaskWorkUnitExecution.completed,
      field0,
    ),
    frb.BridgeTaskWorkUnitState_NeedsAttention(:final field0) =>
      _taskWorkUnitState(
        TaskWorkUnitStateKind.needsAttention,
        TaskWorkUnitExecution.budgetLimited,
        field0,
      ),
    frb.BridgeTaskWorkUnitState_Failed(:final field0) => _taskWorkUnitState(
      TaskWorkUnitStateKind.failed,
      TaskWorkUnitExecution.failed,
      field0,
    ),
    frb.BridgeTaskWorkUnitState_Cancelled(:final field0) => _taskWorkUnitState(
      TaskWorkUnitStateKind.cancelled,
      TaskWorkUnitExecution.cancelled,
      field0,
    ),
  };
}

TaskWorkUnitStateView _taskWorkUnitState(
  TaskWorkUnitStateKind kind,
  TaskWorkUnitExecution execution,
  frb.BridgeTaskWorkUnitProgress progress,
) {
  return TaskWorkUnitStateView(
    kind: kind,
    execution: execution,
    progress: TaskWorkUnitProgressView(
      executionError: progress.executionError,
      budgetLimit: progress.budgetLimit == null
          ? null
          : TaskBudgetLimitView(
              kind: progress.budgetLimit!.kind,
              usage: TaskBudgetUsageView(
                modelSteps: progress.budgetLimit!.usage.modelSteps,
                toolCalls: progress.budgetLimit!.usage.toolCalls,
                waitCalls: progress.budgetLimit!.usage.waitCalls,
                elapsedMs: progress.budgetLimit!.usage.elapsedMs,
              ),
            ),
      budgetSliceCount: progress.budgetSliceCount,
      continuationState: switch (progress.continuationState) {
        frb.BridgeExecutorContinuationState.none =>
          TaskExecutorContinuationState.none,
        frb.BridgeExecutorContinuationState.pendingStart =>
          TaskExecutorContinuationState.pendingStart,
        frb.BridgeExecutorContinuationState.compacting =>
          TaskExecutorContinuationState.compacting,
        frb.BridgeExecutorContinuationState.plannerWakePending =>
          TaskExecutorContinuationState.plannerWakePending,
        frb.BridgeExecutorContinuationState.needsAttention =>
          TaskExecutorContinuationState.needsAttention,
      },
      continuationSourceTurnId: progress.continuationSourceTurnId,
      continuationRevision: progress.continuationRevision,
    ),
  );
}

TaskReviewStateView _taskReviewStateFromFrb(frb.BridgeTaskReviewState state) {
  return switch (state) {
    frb.BridgeTaskReviewState_Pending() => const TaskReviewStateView(
      kind: TaskReviewStateKind.pending,
    ),
    frb.BridgeTaskReviewState_Pass(:final summary) => TaskReviewStateView(
      kind: TaskReviewStateKind.pass,
      summary: summary,
    ),
    frb.BridgeTaskReviewState_ChangesRequired(:final summary) =>
      TaskReviewStateView(
        kind: TaskReviewStateKind.changesRequired,
        summary: summary,
      ),
    frb.BridgeTaskReviewState_Blocked(:final summary) => TaskReviewStateView(
      kind: TaskReviewStateKind.blocked,
      summary: summary,
    ),
    frb.BridgeTaskReviewState_Failed(:final summary, :final error) =>
      TaskReviewStateView(
        kind: TaskReviewStateKind.failed,
        summary: summary,
        error: error,
      ),
  };
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
    endpoint: server.endpoint,
    enabled: server.enabled,
    status: server.statusKind.isEmpty
        ? (server.enabled ? 'enabled' : 'disabled')
        : server.statusKind,
    availabilityKind: server.availabilityKind,
    availabilityMessage: server.availabilityMessage,
    lastCheckedAt: server.lastCheckedAt == null
        ? null
        : _dateFromUnix(server.lastCheckedAt!),
    toolCount: server.toolCount?.toInt(),
    sourceKind: server.sourceKind,
    mutationPolicy: server.mutationPolicy,
  );
}

StudioState studioStateFromFrbSnapshot(frb.BridgeStudioStateSnapshot value) {
  final projects = value.projectDirectory.projects
      .map(_projectFromFrb)
      .toList();
  final threads = value.threadDirectory.threads.map(_threadFromFrb).toList();
  return StudioState(
    projectDirectory: ProjectDirectoryState(
      meta: _observedMetaFromFrb(value.projectDirectory.meta),
      values: projects,
    ),
    threadDirectory: ThreadDirectoryWindow(
      threads: threads,
      nextCursor: value.threadDirectory.nextCursor,
      hasMore: value.threadDirectory.nextCursor != null,
    ),
    taskDirectory: TaskDirectoryState(
      meta: _observedMetaFromFrb(value.taskDirectory.meta),
      values: [
        for (final entry in value.taskDirectory.tasks)
          TaskDirectoryEntryView(
            rootThreadId: entry.rootThreadId,
            task: _taskRuntimeFromFrb(entry.task),
          ),
      ],
    ),
    agentDirectory: AgentDirectoryState(
      meta: _observedMetaFromFrb(value.agentDirectory.meta),
      values: value.agentDirectory.agents
          .map(_agentDirectoryEntryFromFrb)
          .toList(),
    ),
    settingsState: _settingsStateFromFrb(value.settings),
    recoveryState: RecoveryStateSnapshot(
      meta: _observedMetaFromFrb(value.recovery.meta),
      values: value.recovery.issues.map(_recoveryIssueFromFrb).toList(),
    ),
    mcpState: _mcpStateFromFrb(value.mcp),
    lspState: _lspStateFromFrb(value.lsp),
    skillsByProject: {
      for (final snapshot in value.skillsByProject)
        snapshot.projectId: _skillsStateFromFrb(snapshot),
    },
    providerUsageState: _providerUsageStateFromFrb(value.providerUsage),
    updaterState: _updaterStateFromFrb(value.updater),
    selectedProjectId: null,
    selectedThreadId: null,
  );
}

McpStateSnapshot _mcpStateFromFrb(frb.BridgeMcpStateSnapshot snapshot) {
  return McpStateSnapshot(
    meta: _observedMetaFromFrb(snapshot.meta),
    desiredConfigFingerprint: snapshot.desiredConfigFingerprint,
    appliedConfigFingerprint: snapshot.appliedConfigFingerprint,
    activeServers: snapshot.health.activeMcpServers,
    servers: snapshot.health.mcpServers.map(_mcpServerFromFrb).toList(),
  );
}

LspStateSnapshot _lspStateFromFrb(frb.BridgeLspStateSnapshot snapshot) {
  return LspStateSnapshot(
    meta: _observedMetaFromFrb(snapshot.meta),
    activeServers: snapshot.health.activeLspServers,
    servers: [
      for (final server in snapshot.health.lspServers)
        LspServerStateView(
          id: server.id,
          displayName: server.displayName,
          availability: server.availabilityKind,
          message: server.availabilityMessage,
          lastCheckedAt: server.lastCheckedAt == null
              ? null
              : _dateFromUnix(server.lastCheckedAt!),
          lastError: server.lastError,
          diagnosticCount: server.diagnosticCount.toInt(),
          activityKind: server.activityKind,
          activityTitle: server.activityTitle,
          activityMessage: server.activityMessage,
          activityPercentage: server.activityPercentage,
        ),
    ],
  );
}

SkillsStateSnapshot _skillsStateFromFrb(
  frb.BridgeSkillsStateSnapshot snapshot,
) {
  return SkillsStateSnapshot(
    meta: _observedMetaFromFrb(snapshot.meta),
    projectId: snapshot.projectId,
    configFingerprint: snapshot.configFingerprint,
    catalogRevision: snapshot.catalogRevision.toInt(),
    skills: snapshot.skills.map((skill) => skill.name).toList(),
    warnings: snapshot.warnings,
  );
}

ProviderUsageStateSnapshot _providerUsageStateFromFrb(
  frb.BridgeProviderUsageStateSnapshot snapshot,
) {
  return ProviderUsageStateSnapshot(
    meta: _observedMetaFromFrb(snapshot.meta),
    configFingerprint: snapshot.configFingerprint,
    usages: snapshot.usages.map(_providerUsageFromFrb).toList(),
  );
}

UpdaterStateSnapshot _updaterStateFromFrb(
  frb.BridgeUpdaterStateSnapshot snapshot,
) {
  final update = snapshot.update;
  return UpdaterStateSnapshot(
    meta: _observedMetaFromFrb(snapshot.meta),
    version: update?.version,
    publishedAt: update == null ? null : _dateFromUnix(update.publishedAt),
    notesUrl: update?.notesUrl,
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
