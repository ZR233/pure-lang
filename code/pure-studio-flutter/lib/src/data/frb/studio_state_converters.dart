part of 'studio_api.dart';

StudioState _stateFromJson(
  Map<String, Object?> json, {
  required String? selectedProjectId,
  required String? selectedSessionId,
}) {
  final timeline = _timelineFromJson(
    _list(json['messages']),
    _list(json['parts']),
  );
  for (final session in studioSessionsFromJson(json['sessions'])) {
    timeline.messagesBySession.putIfAbsent(session.id, () => []);
  }
  final config = _map(json['config']);
  final runtimeJson = Map<String, Object?>.from(_map(json['sessionRuntime']));
  if (!runtimeJson.containsKey('agents')) {
    runtimeJson['agents'] = _list(json['agents']);
  }
  final eventNextSequence = _int(
    _firstValue(json, const ['eventNextSequence', 'event_next_sequence']),
  );
  return _stateFromTypedSnapshot(
    projects: _list(json['projects']).map(_projectFromJson).toList(),
    sessions: studioSessionsFromJson(json['sessions']),
    selectedProjectId: selectedProjectId,
    selectedSessionId: selectedSessionId,
    messages: timeline.messagesBySession.values
        .expand((messages) => messages)
        .toList(),
    parts: timeline.partSnapshotsBySession.values
        .expand((parts) => parts.values)
        .toList(),
    agentTimelineEventsBySession: _agentTimelineEventsFromJson(
      json['agentEvents'],
    ),
    interactions: _list(json['interactions'])
        .map(pendingInteractionFromJson)
        .where((interaction) => interaction.id.isNotEmpty)
        .toList(),
    runtime: sessionRuntimeFromJson(runtimeJson),
    config: config,
    generalSettings: _map(json['generalSettings']),
    eventNextSequence: eventNextSequence,
    agents: const [],
  );
}

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
  required Iterable<frb.BridgeAgentSnapshotDto> agents,
  required List<PendingInteraction> interactions,
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
      permissionMode: _permissionMode(
        _firstValue(_map(config['runtime']), const [
          'permissionMode',
          'permission_mode',
        ]),
      ),
      turnPhase: TurnPhase.idle,
      runtime: runtime,
      pendingInteractions: interactions,
      eventCursorsBySession: selectedSessionId == null || eventNextSequence <= 0
          ? const {}
          : {selectedSessionId: eventNextSequence - 1},
    ),
    _applySnapshotEvent,
  );
  return latest.copyWith(
    runtime: latest.runtime.copyWith(
      agentCount: agentsBySession[selectedSessionId]?.length ?? 0,
    ),
  );
}

StudioState _applySnapshotEvent(StudioState state, StudioBridgeEvent event) {
  return switch (event.payload) {
    TurnChangedPayload(:final turn) => state.copyWith(
      turnPhase: _turnPhaseFromStatus(turn.status),
    ),
    InteractionChangedPayload(:final interaction, :final status) =>
      _withInteraction(state, interaction, status),
    SessionRuntimeChangedPayload(:final runtime) => state.copyWith(
      runtime: runtime.copyWith(agentCount: state.runtime.agentCount),
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
      state.copyWith(
        mcpServers: servers,
        runtime: state.runtime.copyWith(activeMcpServers: activeMcpServers),
      ),
    LspHealthChangedPayload(:final activeLspServers) => state.copyWith(
      runtime: state.runtime.copyWith(activeLspServers: activeLspServers),
    ),
    SessionListChangedPayload(:final projectId, :final sessions)
        when projectId == null || projectId == state.selectedProjectId =>
      state.copyWith(sessions: sessions),
    _ => state,
  };
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
  return StudioState(
    projects: state.projects,
    sessions: state.sessions,
    messagesBySession: state.messagesBySession,
    partSnapshotsBySession: state.partSnapshotsBySession,
    partOverlaysBySession: state.partOverlaysBySession,
    agentTimelineEventsBySession: state.agentTimelineEventsBySession,
    agentsBySession: state.agentsBySession,
    providers: state.providers,
    defaultProviderId: state.defaultProviderId,
    providerUsages: state.providerUsages,
    roles: state.roles,
    mcpServers: state.mcpServers,
    instructions: state.instructions,
    skills: state.skills,
    general: state.general,
    webSearch: state.webSearch,
    selectedProjectId: state.selectedProjectId,
    selectedSessionId: state.selectedSessionId,
    permissionMode: state.permissionMode,
    turnPhase: state.turnPhase,
    runtime: state.runtime,
    pendingInteractions: interactions,
    eventCursorsBySession: state.eventCursorsBySession,
    composerText: state.composerText,
  );
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

Map<String, Map<String, TimelineAgentEvent>> _agentTimelineEventsFromJson(
  Object? value,
) {
  final bySession = <String, Map<String, TimelineAgentEvent>>{};
  for (final item in _list(value)) {
    final json = _map(item);
    final event = timelineAgentEventFromPayload(
      json,
      eventId: _string(json['eventId']),
      sessionId: _string(json['sessionId']),
      sequence: _int(json['sequence']),
      createdAt: _dateFromUnix(_int(json['createdAt'])),
      kindType: _string(json['kindType']),
    );
    if (event.eventId.isEmpty || event.sessionId.isEmpty) {
      continue;
    }
    bySession.putIfAbsent(event.sessionId, () => {})[event.eventId] = event;
  }
  return bySession;
}

class _TimelineLoadResult {
  const _TimelineLoadResult({
    required this.messagesBySession,
    required this.partSnapshotsBySession,
  });

  final Map<String, List<TimelineMessage>> messagesBySession;
  final Map<String, Map<String, TimelinePartSnapshot>> partSnapshotsBySession;
}

_TimelineLoadResult _timelineFromJson(
  List<Object?> messageValues,
  List<Object?> partValues,
) {
  final snapshotsBySession = <String, Map<String, TimelinePartSnapshot>>{};
  for (final value in partValues) {
    final wrapper = _map(value);
    final nested = _map(wrapper['part']);
    final partJson = nested.isEmpty ? _map(value) : nested;
    final part = timelinePartSnapshotFromJson(
      partJson,
      sequence: _int(wrapper['sequence']),
    );
    if (part.id.isEmpty ||
        part.messageId.isEmpty ||
        part.sessionId.isEmpty ||
        part.ignored ||
        isInternalTimelinePartType(part.type)) {
      continue;
    }
    snapshotsBySession.putIfAbsent(part.sessionId, () => {})[part.id] = part;
  }

  final bySession = <String, List<TimelineMessage>>{};
  for (final value in messageValues) {
    final wrapper = _map(value);
    final nested = _map(wrapper['message']);
    final messageJson = nested.isEmpty ? _map(value) : nested;
    final message = timelineMessageFromJson(
      messageJson,
      sequence: _int(wrapper['sequence']),
    );
    if (message.id.isEmpty || message.sessionId.isEmpty) {
      continue;
    }
    bySession.putIfAbsent(message.sessionId, () => []).add(message);
  }
  for (final messages in bySession.values) {
    messages.sort((a, b) => a.createdAt.compareTo(b.createdAt));
  }
  return _TimelineLoadResult(
    messagesBySession: bySession,
    partSnapshotsBySession: snapshotsBySession,
  );
}

StudioProject _projectFromJson(Object? value) {
  final json = _map(value);
  return StudioProject(
    id: _string(json['id']),
    name: _string(json['name']),
    path: _string(json['path']),
  );
}

StudioSession _sessionFromJson(Object? value) {
  final json = _map(value);
  return StudioSession(
    id: _string(json['id']),
    projectId: _string(json['projectId']),
    title: _string(json['title'], fallback: 'Untitled'),
    mode: _compileMode(json['mode']),
    updatedAt: _dateFromUnix(_int(json['updatedAt'])),
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
    toolCallId: _string(
      _firstValue(json, const ['toolCallId', 'tool_call_id']),
    ),
    callId: _nullableString(_firstValue(json, const ['callId', 'call_id'])),
    providerItemId: _nullableString(
      _firstValue(json, const ['providerItemId', 'provider_item_id']),
    ),
    name: _string(json['name'], fallback: 'tool'),
    arguments: _string(json['arguments']),
    result: _nullableString(json['result']),
    outputArtifacts: _list(
      _firstValue(json, const ['outputArtifacts', 'output_artifacts']),
    ),
    exitCode: _nullableInt(_firstValue(json, const ['exitCode', 'exit_code'])),
    timedOut: _bool(_firstValue(json, const ['timedOut', 'timed_out'])),
    workingDirectory: _nullableString(
      _firstValue(json, const ['workingDirectory', 'working_directory']),
    ),
    denialReason: _nullableString(
      _firstValue(json, const ['denialReason', 'denial_reason']),
    ),
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
    parentPath: _nullableString(
      _firstValue(json, const ['parentPath', 'parent_path']),
    ),
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
  final direct = _string(
    _firstValue(json, const ['planContent', 'plan_content']),
  );
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
