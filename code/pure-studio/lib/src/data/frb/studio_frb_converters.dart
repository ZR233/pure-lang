part of 'studio_api.dart';

StudioBridgeEventPayload _productPayloadFromFrb(
  frb.BridgeProductEventPayload payload,
) {
  return switch (payload) {
    frb.BridgeProductEventPayload_SessionListChanged(
      :final projectId,
      :final sessions,
    ) =>
      SessionListChangedPayload(
        projectId: _emptyToNull(projectId),
        sessions: sessions.map(_sessionFromFrb).toList(),
      ),
    frb.BridgeProductEventPayload_McpHealthChanged(:final health) =>
      McpHealthChangedPayload(
        activeMcpServers: health.activeMcpServers,
        servers: health.mcpServers.map(_mcpServerFromFrb).toList(),
      ),
    frb.BridgeProductEventPayload_LspHealthChanged(:final health) =>
      LspHealthChangedPayload(activeLspServers: health.activeLspServers),
    frb.BridgeProductEventPayload_SessionTaskChanged(
      :final sessionId,
      :final task,
    ) =>
      SessionTaskChangedPayload(
        sessionId: sessionId,
        task: task == null ? null : _taskRuntimeFromFrb(task),
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
    workUnits: [
      for (final unit in task.workUnits)
        TaskWorkUnitView(
          id: unit.id,
          title: unit.title,
          status: unit.status,
          worktreePath: unit.worktreePath,
          branch: unit.branch,
          agentId: unit.agentId,
        ),
    ],
    agents: [
      for (final agent in task.agents)
        TaskAgentOutcomeView(
          agentId: agent.agentId,
          role: agent.role,
          status: agent.status,
          initiatedBy: agent.initiatedBy,
          requestedByCallId: agent.requestedByCallId,
          summary: agent.summary,
          error: agent.error,
          headCommit: agent.headCommit,
        ),
    ],
    merges: [
      for (final merge in task.merges)
        TaskMergeView(
          id: merge.id,
          agentId: merge.agentId,
          status: merge.status,
          mergeCommit: merge.mergeCommit,
          conflictFiles: merge.conflictFiles,
          resolutionSummary: merge.resolutionSummary,
        ),
    ],
    reviews: [
      for (final review in task.reviews)
        TaskReviewView(
          round: review.round.toInt(),
          headCommit: review.headCommit,
          verdict: review.verdict,
          reviewerAgentId: review.reviewerAgentId,
          summary: review.summary,
          designReferences: review.designReferences,
        ),
    ],
  );
}

StudioProject _projectFromFrb(frb.ProjectDto project) {
  return StudioProject(id: project.id, name: project.name, path: project.path);
}

StudioSession _sessionFromFrb(frb.SessionDto session) {
  return StudioSession(
    id: session.id,
    projectId: session.projectId,
    title: session.title,
    mode: _compileMode(session.mode),
    createdAt: _dateFromUnix(session.createdAt),
    updatedAt: _dateFromUnix(session.updatedAt),
    visibility: session.visibility,
    parentSessionId: session.parentSessionId,
    rootSessionId: session.rootSessionId,
    sessionKind: session.sessionKind == 'agent'
        ? StudioSessionKind.agent
        : StudioSessionKind.root,
    ownerAgentId: session.ownerAgentId,
    ownerRole: session.ownerRole,
    agentStatus: session.agentStatus,
    agentSummary: session.agentSummary,
    agentError: session.agentError,
    agentUpdatedAt: session.agentUpdatedAt == null
        ? null
        : _dateFromUnix(session.agentUpdatedAt!),
  );
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

Map<String, Map<String, StudioAgentView>> _agentsFromTyped(
  Iterable<StudioAgentView> agents,
) {
  final bySession = <String, Map<String, StudioAgentView>>{};
  for (final agent in agents) {
    if (agent.id.isEmpty || agent.sessionId.isEmpty) {
      continue;
    }
    bySession.putIfAbsent(agent.sessionId, () => {})[agent.id] = agent;
  }
  return bySession;
}

StudioState studioStateFromFrbSnapshot(frb.BridgeStudioSnapshotResponse value) {
  return _stateFromTypedSnapshot(
    projects: value.projects.map(_projectFromFrb).toList(),
    sessions: value.sessions.map(_sessionFromFrb).toList(),
    selectedProjectId: value.selectedProjectId,
    selectedSessionId: value.selectedSessionId,
    messages: const [],
    parts: const [],
    agents: const [],
    interactions: const [],
    recoveryIssues: value.recoveryIssues.map(_recoveryIssueFromFrb).toList(),
    runtime: _emptyRuntimeView().copyWith(
      task: value.selectedSessionTask == null
          ? null
          : _taskRuntimeFromFrb(value.selectedSessionTask!),
    ),
    config: _decodeJson(value.configJson),
    generalSettings: _decodeJson(value.generalSettingsJson),
    webSearch: value.webSearch,
    eventNextSequence: 0,
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
      frb.BridgeRecoveryIssueScope.session => RecoveryIssueScope.session,
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
          frb.BridgeRecoveryIssueAction.cleanupSession =>
            RecoveryIssueAction.cleanupSession,
          frb.BridgeRecoveryIssueAction.removeProject =>
            RecoveryIssueAction.removeProject,
        },
    ],
    projectId: issue.projectId,
    sessionId: issue.sessionId,
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
      frb.BridgeRecoveryIssueScope.session => RecoveryIssueScope.session,
    },
    projectId: preview.projectId,
    sessionId: preview.sessionId,
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
