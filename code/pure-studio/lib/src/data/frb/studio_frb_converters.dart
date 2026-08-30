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
    frb.BridgeProductEventPayload_ModelPerformanceStateChanged(:final field0) =>
      ModelPerformanceStateChangedPayload(_modelPerformanceFromFrb(field0)),
    frb.BridgeProductEventPayload_UpdaterStateChanged(:final field0) =>
      UpdaterStateChangedPayload(updaterStateFromFrb(field0)),
    frb.BridgeProductEventPayload_PersistenceStateChanged(:final field0) =>
      PersistenceStateChangedPayload(_persistenceStateFromFrb(field0)),
    frb.BridgeProductEventPayload_Stale(:final laggedEvents) => StalePayload(
      laggedEvents: laggedEvents.toInt(),
    ),
  };
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
  return StudioProject(
    id: project.id,
    name: project.name,
    path: project.path,
    sshServerId: project.sshServerId,
  );
}

SshServer _sshServerFromFrb(frb_ssh_types.SshServerDto server) {
  return SshServer(
    id: server.id,
    name: server.name,
    host: server.host,
    port: server.port,
    username: server.username,
    authKind: switch (server.authKind) {
      frb_ssh_types.SshAuthKindDto.agentOrKey => SshAuthKind.agentOrKey,
      frb_ssh_types.SshAuthKindDto.password => SshAuthKind.password,
    },
    identityFile: server.identityFile,
  );
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
    modelPerformance: _modelPerformanceFromFrb(value.modelPerformance),
    updaterState: updaterStateFromFrb(value.updater),
    persistenceState: _persistenceStateFromFrb(value.persistence),
    selectedProjectId: null,
    selectedThreadId: null,
  );
}

PersistenceStateSnapshot _persistenceStateFromFrb(
  frb.BridgePersistenceStateSnapshot snapshot,
) {
  return PersistenceStateSnapshot(
    revision: snapshot.revision.toInt(),
    state: switch (snapshot.state) {
      frb.BridgePersistenceState_Ready(:final pendingCommits) =>
        ReadyPersistenceState(pendingCommits: pendingCommits.toInt()),
      frb.BridgePersistenceState_Flushing(
        :final pendingCommits,
        :final oldestPendingRevision,
      ) =>
        FlushingPersistenceState(
          pendingCommits: pendingCommits.toInt(),
          oldestPendingRevision: oldestPendingRevision?.toInt(),
        ),
      frb.BridgePersistenceState_Degraded(
        :final pendingCommits,
        :final oldestPendingRevision,
        :final firstFailedAt,
        :final error,
      ) =>
        DegradedPersistenceState(
          pendingCommits: pendingCommits.toInt(),
          oldestPendingRevision: oldestPendingRevision?.toInt(),
          firstFailedAt: firstFailedAt.toInt(),
          error: _resourceError(error),
        ),
      frb.BridgePersistenceState_Recovering(
        :final pendingCommits,
        :final oldestPendingRevision,
        :final firstFailedAt,
      ) =>
        RecoveringPersistenceState(
          pendingCommits: pendingCommits.toInt(),
          oldestPendingRevision: oldestPendingRevision?.toInt(),
          firstFailedAt: firstFailedAt.toInt(),
        ),
      frb.BridgePersistenceState_Blocked(
        :final pendingCommits,
        :final oldestPendingRevision,
        :final firstFailedAt,
        :final error,
      ) =>
        BlockedPersistenceState(
          pendingCommits: pendingCommits.toInt(),
          oldestPendingRevision: oldestPendingRevision?.toInt(),
          firstFailedAt: firstFailedAt.toInt(),
          error: _resourceError(error),
        ),
    },
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
    modes: [
      for (final mode in data.modes)
        ModeDescriptorView(
          id: mode.id,
          displayName: mode.displayName,
          description: mode.description,
          order: mode.order,
          source: mode.source,
          providerId: mode.providerId,
        ),
    ],
    warnings: data.warnings,
    complete: data.complete,
    summaries: data.skills
        .map(
          (skill) => SkillSummaryView(
            name: skill.name,
            description: skill.description,
            category: skill.category,
            platforms: skill.platforms,
            source: skill.source,
            providerId: skill.providerId,
            modelInvocable: skill.invocation.modelInvocable,
            userInvocable: skill.invocation.userInvocable,
            resourceBase: skill.resourceBase.when(
              directory: (path) =>
                  SkillResourceBaseView(SkillResourceBaseKind.directory, path),
              url: (url) =>
                  SkillResourceBaseView(SkillResourceBaseKind.url, url),
              opaque: (description) => SkillResourceBaseView(
                SkillResourceBaseKind.opaque,
                description,
              ),
            ),
          ),
        )
        .toList(),
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

ModelPerformanceSnapshotView _modelPerformanceFromFrb(
  frb.BridgeModelPerformanceSnapshot value,
) {
  final updatedAt = value.updatedAt.toInt();
  return ModelPerformanceSnapshotView(
    revision: value.revision.toInt(),
    updatedAt: updatedAt == 0 ? null : _dateFromUnix(value.updatedAt),
    sessionCosts: [
      for (final session in value.sessionCosts)
        SessionCostView(
          rootThreadId: session.rootThreadId,
          estimatedCosts: [
            for (final cost in session.estimatedCosts)
              RuntimeCostView(currency: cost.currency, amount: cost.amount),
          ],
          hasUnpricedUsage: session.hasUnpricedUsage,
        ),
    ],
    summaries: [
      for (final summary in value.summaries)
        ModelPerformanceSummaryView(
          providerInstanceId: summary.providerInstanceId,
          providerDisplayName: summary.providerDisplayName,
          model: summary.model,
          sampleCount: summary.sampleCount.toInt(),
          completionTokens: summary.completionTokens.toInt(),
          totalTtftMillis: summary.totalTtftMillis.toInt(),
          totalDecodeMillis: summary.totalDecodeMillis.toInt(),
          totalResponseMillis: summary.totalResponseMillis.toInt(),
          tokensPerSecond: summary.tokensPerSecond,
          averageTtftMillis: summary.averageTtftMillis,
          averageResponseMillis: summary.averageResponseMillis,
        ),
    ],
    history: [
      for (final sample in value.history)
        ModelPerformanceSampleView(
          completedAt: _dateFromUnix(sample.completedAt),
          providerInstanceId: sample.providerInstanceId,
          providerDisplayName: sample.providerDisplayName,
          model: sample.model,
          completionTokens: sample.completionTokens.toInt(),
          ttftMillis: sample.ttftMillis.toInt(),
          decodeMillis: sample.decodeMillis.toInt(),
          totalResponseMillis: sample.totalResponseMillis.toInt(),
          tokensPerSecond: sample.tokensPerSecond,
        ),
    ],
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
      frb.BridgeRecoveryIssueCategory.repository =>
        RecoveryIssueCategory.repository,
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
    detail: issue.detail,
  );
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
