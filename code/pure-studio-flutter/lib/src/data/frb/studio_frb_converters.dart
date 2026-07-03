part of 'studio_api.dart';

StudioBridgeEventPayload _bridgePayloadFromFrb(
  frb.BridgeEventPayload payload, {
  required BigInt sequence,
}) {
  final itemSequence = sequence.toInt();
  return switch (payload) {
    frb.BridgeEventPayload_TurnChanged(:final turn) => TurnChangedPayload(
      turn: StudioTurnView(sessionId: turn.sessionId, status: turn.status),
    ),
    frb.BridgeEventPayload_MessageUpdated(:final message) =>
      MessageUpdatedPayload(
        message: _timelineMessageFromFrb(message, sequence: itemSequence),
      ),
    frb.BridgeEventPayload_MessageRemoved(:final messageId) =>
      MessageRemovedPayload(messageId: messageId),
    frb.BridgeEventPayload_MessagePartUpdated(:final part_) =>
      _isIgnoredTimelinePartType(part_.partType)
          ? const IgnoredBridgeEventPayload()
          : MessagePartUpdatedPayload(
              part: _timelinePartSnapshotFromFrb(part_, sequence: itemSequence),
            ),
    frb.BridgeEventPayload_MessagePartRemoved(
      :final messageId,
      :final partId,
    ) =>
      MessagePartRemovedPayload(messageId: messageId, partId: partId),
    frb.BridgeEventPayload_MessagePartDelta(:final delta) =>
      MessagePartDeltaPayload(delta: _timelinePartDeltaFromFrb(delta)),
    frb.BridgeEventPayload_InteractionChanged(:final event) =>
      _interactionChangedPayloadFromFrb(event),
    frb.BridgeEventPayload_AgentChanged(:final agent) => AgentChangedPayload(
      agent: _agentViewFromFrb(agent),
    ),
    frb.BridgeEventPayload_AgentTimelineChanged(:final event) =>
      AgentTimelineChangedPayload(event: _timelineAgentEventFromFrb(event)),
    frb.BridgeEventPayload_SessionRuntimeChanged(:final runtime) =>
      SessionRuntimeChangedPayload(
        runtime: _sessionRuntimeFromFrb(runtime),
        sessionId: runtime.sessionId,
      ),
    frb.BridgeEventPayload_SkillActivated(:final activation) =>
      SkillActivatedPayload(name: activation.name),
    frb.BridgeEventPayload_PlanLifecycleChanged(:final event) =>
      PlanLifecycleChangedPayload(state: event.state),
    frb.BridgeEventPayload_SessionListChanged(
      :final projectId,
      :final sessions,
    ) =>
      SessionListChangedPayload(
        projectId: _emptyToNull(projectId),
        sessions: sessions.map(_sessionFromFrb).toList(),
      ),
    frb.BridgeEventPayload_McpHealthChanged(:final health) =>
      McpHealthChangedPayload(
        activeMcpServers: health.activeMcpServers,
        servers: health.mcpServers.map(_mcpServerFromFrb).toList(),
      ),
    frb.BridgeEventPayload_LspHealthChanged(:final health) =>
      LspHealthChangedPayload(activeLspServers: health.activeLspServers),
    frb.BridgeEventPayload_Stale(:final laggedEvents) => StalePayload(
      laggedEvents: laggedEvents.toInt(),
    ),
  };
}

TimelineMessage _timelineMessageFromFrb(
  frb.BridgeStudioMessageDto message, {
  required int sequence,
}) {
  return TimelineMessage(
    id: message.messageId,
    sessionId: message.sessionId,
    turnId: message.turnId,
    role: message.role.isEmpty ? 'assistant' : message.role,
    status: message.status.isEmpty ? 'completed' : message.status,
    createdAt: _dateFromUnix(message.createdAt),
    updatedAt: _dateFromUnix(message.updatedAt),
    completedAt: message.completedAt == null
        ? null
        : _dateFromUnix(message.completedAt!),
    error: message.error,
    sequence: sequence,
  );
}

TimelinePartSnapshot _timelinePartSnapshotFromFrb(
  frb.BridgeStudioPartDto part, {
  required int sequence,
}) {
  return TimelinePartSnapshot(
    id: part.partId,
    messageId: part.messageId,
    sessionId: part.sessionId,
    turnId: part.turnId,
    type: _partType(part.partType),
    order: part.order.toInt(),
    revision: part.revision.toInt(),
    sequence: sequence,
    text: _frbPartText(part),
    status: part.status.isEmpty ? 'completed' : part.status,
    createdAt: _dateFromUnix(part.createdAt),
    updatedAt: _dateFromUnix(part.updatedAt),
    completedAt: part.completedAt == null
        ? null
        : _dateFromUnix(part.completedAt!),
    error: part.error,
    textChannel: _textChannel(part.textChannel),
    activityGroupId: part.activityGroupId,
    tool: _toolPartFromFrb(part.tool),
    agent: _agentPartFromFrb(part.agent),
    planContent: part.plan?.content,
    synthetic: part.synthetic,
    ignored: part.ignored,
  );
}

String _frbPartText(frb.BridgeStudioPartDto part) {
  if (part.text.isNotEmpty) {
    return part.text;
  }
  return switch (_partType(part.partType)) {
    TimelinePartType.tool => [
      part.tool?.arguments,
      part.tool?.result,
    ].whereType<String>().where((value) => value.isNotEmpty).join('\n'),
    TimelinePartType.plan => part.plan?.content ?? '',
    TimelinePartType.agent => part.agent?.summary ?? part.agent?.task ?? '',
    TimelinePartType.reasoning ||
    TimelinePartType.text ||
    TimelinePartType.turn ||
    TimelinePartType.inference ||
    TimelinePartType.file => '',
  };
}

TimelineToolPart? _toolPartFromFrb(frb.BridgeStudioToolPartDto? tool) {
  if (tool == null) {
    return null;
  }
  return TimelineToolPart(
    toolCallId: tool.toolCallId,
    callId: tool.callId,
    providerItemId: tool.providerItemId,
    name: tool.name.isEmpty ? 'tool' : tool.name,
    arguments: tool.arguments,
    result: tool.result,
    exitCode: tool.exitCode,
    timedOut: tool.timedOut,
    workingDirectory: tool.workingDirectory,
    denialReason: tool.denialReason,
  );
}

TimelineAgentPart? _agentPartFromFrb(frb.BridgeStudioAgentPartDto? agent) {
  if (agent == null) {
    return null;
  }
  return TimelineAgentPart(
    id: agent.id,
    path: agent.path,
    parentPath: agent.parentPath,
    role: agent.role.isEmpty ? 'agent' : agent.role,
    task: agent.task,
    status: agent.status,
    summary: agent.summary,
    depth: agent.depth,
    error: agent.error,
    reason: agent.reason,
  );
}

TimelinePartDelta _timelinePartDeltaFromFrb(
  frb.BridgeStudioPartDeltaDto delta,
) {
  return TimelinePartDelta(
    partId: delta.partId,
    revision: delta.revision.toInt(),
    field: _timelineDeltaField(delta.field),
    delta: delta.delta,
    chunkIndex: delta.chunkIndex,
  );
}

InteractionChangedPayload _interactionChangedPayloadFromFrb(
  frb.BridgeInteractionChangedDto event,
) {
  final payload = _interactionPayloadFromFrb(event.payload);
  final kind = _interactionKind(event.kind);
  final interaction = PendingInteraction(
    id: event.interactionId,
    sessionId: event.sessionId,
    kind: kind,
    title: _interactionTitle(kind, payload),
    body: _interactionBody(kind, payload),
    payload: payload,
  );
  return InteractionChangedPayload(
    interaction: interaction,
    status: event.status,
  );
}

TimelineAgentEvent _timelineAgentEventFromFrb(
  frb.BridgeAgentTimelineEventDto event,
) {
  return TimelineAgentEvent(
    eventId: event.eventId,
    sessionId: event.sessionId,
    sequence: event.sequence.toInt(),
    payload: _agentTimelinePayloadFromFrb(event.payload),
    createdAt: _dateFromUnix(event.createdAt),
  );
}

TimelineAgentEventPayload _agentTimelinePayloadFromFrb(
  frb.BridgeAgentTimelinePayloadDto payload,
) {
  return switch (payload) {
    frb.BridgeAgentTimelinePayloadDto_SubAgentActivity(
      :final callId,
      :final agentId,
      :final path,
      :final parentPath,
      :final kind,
      :final status,
      :final message,
      :final timedOut,
      :final error,
    ) =>
      TimelineSubAgentActivity(
        callId: callId,
        agentId: agentId,
        path: path,
        parentPath: parentPath,
        kind: kind,
        statusValue: status,
        message: message,
        timedOut: timedOut,
        error: error,
      ),
    frb.BridgeAgentTimelinePayloadDto_TodoListUpdated(:final snapshot) =>
      TimelineTodoListUpdate(
        callId: snapshot.callId,
        agentId: snapshot.agentId,
        path: snapshot.path,
        parentPath: snapshot.parentPath,
        explanation: snapshot.explanation,
        items: [
          for (final item in snapshot.items)
            TimelineTodoItem(step: item.step, status: item.status),
        ],
      ),
  };
}

SessionRuntimeView _sessionRuntimeFromFrb(frb.BridgeSessionRuntimeDto runtime) {
  return _sessionRuntimeFromFrbWithAgents(runtime, agentCount: 0);
}

SessionRuntimeView _sessionRuntimeFromFrbWithAgents(
  frb.BridgeSessionRuntimeDto runtime, {
  required int agentCount,
}) {
  return SessionRuntimeView(
    model: runtime.model,
    contextTokens: runtime.latestContextTokens.toInt(),
    contextWindow: runtime.contextWindow?.toInt() ?? 0,
    totalTokens: runtime.totalTokens.toInt(),
    costLabel: _costLabel(
      runtime.estimatedCosts
          .map((cost) => {'currency': cost.currency, 'amount': cost.amount})
          .toList(),
      runtime.hasUnpricedUsage,
    ),
    activeSkills: runtime.activeSkills,
    activeMcpServers: runtime.activeMcpServers,
    activeLspServers: runtime.activeLspServers,
    agentCount: agentCount,
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
    updatedAt: _dateFromUnix(session.updatedAt),
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
  );
}

String? _emptyToNull(String value) {
  return value.isEmpty ? null : value;
}

PendingInteraction _pendingInteractionFromFrb(
  frb.BridgeInteractionChangedDto event,
) {
  final payload = _interactionPayloadFromFrb(event.payload);
  final kind = _interactionKind(event.kind);
  return PendingInteraction(
    id: event.interactionId,
    sessionId: event.sessionId,
    kind: kind,
    title: _interactionTitle(kind, payload),
    body: _interactionBody(kind, payload),
    payload: payload,
  );
}

Map<String, Object?> _interactionPayloadFromFrb(
  frb.BridgeInteractionPayloadDto payload,
) {
  return switch (payload) {
    frb.BridgeInteractionPayloadDto_UserInput(:final questions) => {
      'type': 'userInput',
      'questions': questions
          .map(
            (question) => {
              'id': question.id,
              'header': question.header,
              'question': question.question,
              'prompt': question.question,
              'isOther': question.isOther,
              'isSecret': question.isSecret,
              if (question.options != null)
                'options': question.options!
                    .map(
                      (option) => {
                        'label': option.label,
                        'description': option.description,
                      },
                    )
                    .toList(),
            },
          )
          .toList(),
    },
    frb.BridgeInteractionPayloadDto_ToolApproval(
      :final name,
      :final argumentsJson,
      :final workingDirectory,
      :final parentAgentId,
    ) =>
      {
        'type': 'toolApproval',
        'name': name,
        'arguments': _tryDecodeJsonValue(argumentsJson),
        'workingDirectory': ?workingDirectory,
        'parentAgentId': ?parentAgentId,
      },
    frb.BridgeInteractionPayloadDto_PlanConfirmation(
      :final planId,
      :final content,
    ) =>
      {'type': 'planConfirmation', 'planId': planId, 'content': content},
  };
}

Map<String, Map<String, TimelineAgentEvent>> _agentTimelineEventsFromTyped(
  Iterable<TimelineAgentEvent> events,
) {
  final bySession = <String, Map<String, TimelineAgentEvent>>{};
  for (final event in events) {
    if (event.eventId.isEmpty || event.sessionId.isEmpty) {
      continue;
    }
    bySession.putIfAbsent(event.sessionId, () => {})[event.eventId] = event;
  }
  return bySession;
}

Map<String, Map<String, StudioAgentView>> _agentsFromTyped(
  Iterable<frb.BridgeAgentSnapshotDto> agents,
) {
  final bySession = <String, Map<String, StudioAgentView>>{};
  for (final agent in agents.map(_agentViewFromFrb)) {
    if (agent.id.isEmpty || agent.sessionId.isEmpty) {
      continue;
    }
    bySession.putIfAbsent(agent.sessionId, () => {})[agent.id] = agent;
  }
  return bySession;
}

StudioAgentView _agentViewFromFrb(frb.BridgeAgentSnapshotDto value) {
  return StudioAgentView(
    id: value.id,
    sessionId: value.sessionId,
    path: value.path,
    parentPath: value.parentPath,
    role: value.role,
    task: value.task,
    status: value.status,
    summary: value.summary,
    depth: value.depth,
    error: value.error,
    reason: value.reason,
    updatedAt: _dateFromUnix(value.updatedAt),
  );
}

StudioState studioStateFromFrbSnapshot(frb.BridgeStudioSnapshotResponse value) {
  return _stateFromTypedSnapshot(
    projects: value.projects.map(_projectFromFrb).toList(),
    sessions: value.sessions.map(_sessionFromFrb).toList(),
    selectedProjectId: value.selectedProjectId,
    selectedSessionId: value.selectedSessionId,
    messages: const [],
    parts: const [],
    agentEvents: value.agentEvents.map(_timelineAgentEventFromFrb).toList(),
    agents: value.agents,
    interactions: value.interactions
        .map(_pendingInteractionFromFrb)
        .where((interaction) => interaction.id.isNotEmpty)
        .toList(),
    runtime: value.sessionRuntime == null
        ? _emptyRuntimeView()
        : _sessionRuntimeFromFrbWithAgents(
            value.sessionRuntime!,
            agentCount: value.agents.length,
          ),
    config: _decodeJson(value.configJson),
    generalSettings: _decodeJson(value.generalSettingsJson),
    eventNextSequence: 0,
  );
}

StudioState studioStateFromFrbSession(frb.BridgeSessionStateResponse value) {
  return _stateFromTypedSnapshot(
    projects: const [],
    sessions: value.sessions.map(_sessionFromFrb).toList(),
    selectedProjectId: value.session.projectId,
    selectedSessionId: value.sessionId.isEmpty
        ? value.session.id
        : value.sessionId,
    messages: value.messages
        .map(
          (item) => _timelineMessageFromFrb(
            item.message,
            sequence: item.sequence.toInt(),
          ),
        )
        .toList(),
    parts: value.parts
        .where((item) => !_isIgnoredTimelinePartType(item.part_.partType))
        .map(
          (item) => _timelinePartSnapshotFromFrb(
            item.part_,
            sequence: item.sequence.toInt(),
          ),
        )
        .toList(),
    events: value.events.map(StudioBridgeEvent.fromFrb).toList(),
    agentEvents: value.agentEvents.map(_timelineAgentEventFromFrb).toList(),
    agents: value.agents,
    interactions: value.interactions
        .map(_pendingInteractionFromFrb)
        .where((interaction) => interaction.id.isNotEmpty)
        .toList(),
    runtime: value.sessionRuntime == null
        ? _emptyRuntimeView()
        : _sessionRuntimeFromFrbWithAgents(
            value.sessionRuntime!,
            agentCount: value.agents.length,
          ),
    config: const {},
    generalSettings: const {},
    eventNextSequence: value.eventNextSequence.toInt(),
  );
}

ProviderUsageView _providerUsageFromFrb(frb.ProviderUsageDto usage) {
  return ProviderUsageView(
    providerId: usage.providerId,
    updatedAt: usage.updatedAt,
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
                    nextResetAt: item.nextResetAt,
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
