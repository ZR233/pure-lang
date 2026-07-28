part of 'studio_api.dart';

StudioState _stateFromTypedSnapshot({
  required List<StudioProject> projects,
  required List<StudioSession> sessions,
  required String? selectedProjectId,
  required String? selectedSessionId,
  required List<TimelineMessage> messages,
  required List<TimelinePartSnapshot> parts,
  Iterable<StudioBridgeEvent> events = const [],
  Iterable<TimelineAgentEvent> agentEvents = const [],
  Map<String, Map<String, TimelineAgentEvent>>? agentTimelineEventsBySession,
  required Iterable<StudioAgentView> agents,
  required List<PendingInteraction> interactions,
  List<StudioRecoveryIssue> recoveryIssues = const [],
  required SessionRuntimeView runtime,
  required Map<String, Object?> config,
  required Map<String, Object?> generalSettings,
  frb.BridgeWebSearchSettingsDto? webSearch,
  required int eventNextSequence,
}) {
  final messagesBySession = <String, List<TimelineMessage>>{};
  for (final message in messages) {
    if (message.id.isEmpty || message.sessionId.isEmpty) {
      continue;
    }
    messagesBySession.putIfAbsent(message.sessionId, () => []).add(message);
  }
  for (final messages in messagesBySession.values) {
    messages.sort((a, b) => a.createdAt.compareTo(b.createdAt));
  }
  final partSnapshotsBySession = <String, Map<String, TimelinePartSnapshot>>{};
  for (final part in parts) {
    if (part.id.isEmpty || part.messageId.isEmpty || part.sessionId.isEmpty) {
      continue;
    }
    partSnapshotsBySession.putIfAbsent(part.sessionId, () => {})[part.id] =
        part;
  }
  for (final session in sessions) {
    messagesBySession.putIfAbsent(session.id, () => []);
  }
  final agentEventsBySession =
      agentTimelineEventsBySession ??
      _agentTimelineEventsFromTyped(agentEvents);
  final agentsBySession = _agentsFromTyped(agents);
  final latest = events.fold<StudioState>(
    StudioState(
      projects: projects,
      sessions: sessions,
      messagesBySession: messagesBySession,
      partSnapshotsBySession: partSnapshotsBySession,
      partOverlaysBySession: const {},
      agentTimelineEventsBySession: agentEventsBySession,
      agentsBySession: agentsBySession,
      providers: _providersFromConfig(config),
      defaultProviderId: _defaultProviderIdFromConfig(config),
      roles: _rolesFromConfig(config),
      mcpServers: _mcpServersFromConfig(config),
      instructions: _instructionsFromConfig(config),
      skills: _skillsFromConfig(config),
      general: _generalFromJson(generalSettings),
      webSearch: webSearch == null
          ? const WebSearchSettingsView()
          : _webSearchFromFrb(webSearch),
      selectedProjectId: selectedProjectId,
      selectedSessionId: selectedSessionId,
      selectedRootSessionId: sessions
          .where((session) => session.id == selectedSessionId)
          .firstOrNull
          ?.effectiveRootSessionId,
      permissionMode: _permissionMode(
        _firstValue(_map(config['runtime']), const [
          'permissionMode',
          'permission_mode',
        ]),
      ),
      turnPhasesBySession: selectedSessionId == null
          ? const {}
          : {selectedSessionId: TurnPhase.idle},
      runtimesBySession: selectedSessionId == null
          ? const {}
          : {selectedSessionId: runtime},
      pendingInteractions: interactions,
      recoveryIssues: recoveryIssues,
      eventCursorsBySession: selectedSessionId == null || eventNextSequence <= 0
          ? const {}
          : {selectedSessionId: eventNextSequence - 1},
    ),
    _applySnapshotEvent,
  );
  if (selectedSessionId == null) {
    return latest;
  }
  return latest.copyWith(
    runtimesBySession: {
      selectedSessionId: latest.runtime.copyWith(
        agentCount: agentsBySession[selectedSessionId]?.length ?? 0,
      ),
    },
  );
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

StudioState _applySnapshotEvent(StudioState state, StudioBridgeEvent event) {
  return switch (event.payload) {
    TurnChangedPayload(:final turn) => state.copyWith(
      turnPhasesBySession: {turn.sessionId: _turnPhaseFromStatus(turn.status)},
    ),
    InteractionChangedPayload(:final interaction, :final status) =>
      _withInteraction(state, interaction, status),
    SessionRuntimeChangedPayload(:final sessionId, :final runtime) =>
      state.copyWith(
        runtimesBySession: {
          sessionId: runtime.copyWith(
            agentCount:
                state.runtimesBySession[sessionId]?.agentCount ??
                runtime.agentCount,
          ),
        },
      ),
    AgentTimelineChangedPayload(:final event) => state.copyWith(
      agentTimelineEventsBySession: {
        ...state.agentTimelineEventsBySession,
        event.sessionId: {
          ...(state.agentTimelineEventsBySession[event.sessionId] ?? const {}),
          event.eventId: event,
        },
      },
    ),
    McpHealthChangedPayload(:final activeMcpServers, :final servers) =>
      _withSessionRuntime(
        state.copyWith(mcpServers: servers),
        event.sessionId,
        (runtime) => runtime.copyWith(activeMcpServers: activeMcpServers),
      ),
    LspHealthChangedPayload(:final activeLspServers) => _withSessionRuntime(
      state,
      event.sessionId,
      (runtime) => runtime.copyWith(activeLspServers: activeLspServers),
    ),
    SessionListChangedPayload(:final projectId, :final sessions)
        when projectId == null || projectId == state.selectedProjectId =>
      state.copyWith(sessions: sessions),
    _ => state,
  };
}

StudioState _withSessionRuntime(
  StudioState state,
  String? sessionId,
  SessionRuntimeView Function(SessionRuntimeView runtime) update,
) {
  if (sessionId == null) {
    return state;
  }
  final runtime = state.runtimesBySession[sessionId] ?? _emptyRuntimeView();
  return state.copyWith(runtimesBySession: {sessionId: update(runtime)});
}

StudioState _withInteraction(
  StudioState state,
  PendingInteraction interaction,
  String status,
) {
  final interactions = [...state.pendingInteractions];
  final index = interactions.indexWhere((item) => item.id == interaction.id);
  if (status == 'pending') {
    if (index >= 0) {
      interactions[index] = interaction;
    } else {
      interactions.add(interaction);
    }
  } else if (index >= 0) {
    interactions.removeAt(index);
  }
  return state.copyWith(pendingInteractions: interactions);
}

WebSearchSettingsView _webSearchFromFrb(frb.BridgeWebSearchSettingsDto value) {
  return WebSearchSettingsView(
    configuredMode: value.configuredMode,
    effectiveMode: value.effectiveMode,
    availability: value.availability,
    contextSize: value.contextSize,
    allowedDomains: value.allowedDomains,
    country: value.country,
    region: value.region,
    city: value.city,
    timezone: value.timezone,
    providerId: value.providerId,
    model: value.model,
  );
}

String _partText(Map<String, Object?> json, TimelinePartType type) {
  final text = _string(json['text']);
  if (text.isNotEmpty) {
    return text;
  }
  return switch (type) {
    TimelinePartType.tool => [
      _string(_map(json['tool'])['arguments']),
      _string(_map(json['tool'])['result']),
    ].where((part) => part.isNotEmpty).join('\n'),
    TimelinePartType.plan => _partPlanContent(json),
    TimelinePartType.agent => _string(
      _map(json['agent'])['summary'],
      fallback: _string(_map(json['agent'])['task']),
    ),
    TimelinePartType.reasoning ||
    TimelinePartType.text ||
    TimelinePartType.turn ||
    TimelinePartType.inference ||
    TimelinePartType.file => '',
  };
}

TimelineTextChannel? _textChannel(Object? value) {
  final label = _normalizedWireLabel(value);
  if (label.isEmpty) {
    return null;
  }
  return switch (label) {
    'user' => TimelineTextChannel.user,
    'commentary' => TimelineTextChannel.commentary,
    'final' ||
    'finalanswer' ||
    'final_answer' => TimelineTextChannel.finalAnswer,
    _ => throw FormatException('Unknown text channel: $label'),
  };
}

TimelineToolPart? _toolPart(Object? value) {
  final json = _map(value);
  if (json.isEmpty) {
    return null;
  }
  return TimelineToolPart(
    toolCallId: _string(json['toolCallId']),
    callId: _nullableString(json['callId']),
    providerItemId: _nullableString(json['providerItemId']),
    name: _string(json['name'], fallback: 'tool'),
    arguments: _string(json['arguments']),
    result: _nullableString(json['result']),
    outputArtifacts: _list(json['outputArtifacts']),
    exitCode: _nullableInt(json['exitCode']),
    timedOut: _bool(json['timedOut']),
    workingDirectory: _nullableString(json['workingDirectory']),
    denialReason: _nullableString(json['denialReason']),
  );
}

TimelineAgentPart? _agentPart(Object? value) {
  final json = _map(value);
  if (json.isEmpty) {
    return null;
  }
  return TimelineAgentPart(
    id: _string(json['id']),
    path: _string(json['path']),
    parentPath: _nullableString(json['parentPath']),
    role: _string(json['role'], fallback: 'agent'),
    task: _string(json['task']),
    status: _string(json['status']),
    summary: _nullableString(json['summary']),
    depth: _int(json['depth']),
    error: _nullableString(json['error']),
    reason: _nullableString(json['reason']),
  );
}

String _partPlanContent(Map<String, Object?> json) {
  final direct = _string(json['planContent']);
  if (direct.isNotEmpty) {
    return direct;
  }
  final plan = _map(json['plan']);
  if (plan.isNotEmpty) {
    return _string(plan['content']);
  }
  return _string(json['plan']);
}

String _interactionTitle(InteractionKind kind, Map<String, Object?> payload) {
  return switch (kind) {
    InteractionKind.toolApproval => _string(
      payload['name'],
      fallback: 'Tool approval',
    ),
    InteractionKind.userInput => 'User input requested',
    InteractionKind.planConfirmation => 'Plan confirmation',
  };
}

String _interactionBody(InteractionKind kind, Map<String, Object?> payload) {
  return switch (kind) {
    InteractionKind.toolApproval => _jsonText(payload['arguments']),
    InteractionKind.userInput =>
      _list(payload['questions'])
          .map((question) => _string(_map(question)['prompt']))
          .where((prompt) => prompt.isNotEmpty)
          .join('\n'),
    InteractionKind.planConfirmation => _string(payload['content']),
  };
}

TimelinePartType _partType(Object? value) {
  final label = _normalizedWireLabel(value);
  return switch (label) {
    'text' => TimelinePartType.text,
    'reasoning' || 'reasoning_summary' => TimelinePartType.reasoning,
    'tool' || 'tool_activity' => TimelinePartType.tool,
    'plan' => TimelinePartType.plan,
    'agent' || 'agent_activity' => TimelinePartType.agent,
    'turn' || 'turn_lifecycle' => TimelinePartType.turn,
    'inference' || 'inference_lifecycle' => TimelinePartType.inference,
    'file' => TimelinePartType.file,
    _ => throw FormatException('Unknown timeline part type: $label'),
  };
}
