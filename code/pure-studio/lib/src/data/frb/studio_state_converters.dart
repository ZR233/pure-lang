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
        _map(config['runtime'])['permissionMode'],
      ),
      turnsBySession: const {},
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
      turnsBySession: {turn.sessionId: turn},
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
  final index = interactions.indexWhere(
    (item) =>
        item.id == interaction.id && item.sessionId == interaction.sessionId,
  );
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

String _interactionTitle(InteractionKind kind, InteractionPayload payload) {
  return switch (payload) {
    ToolApprovalInteractionPayload(:final toolName) =>
      toolName.isEmpty ? 'Tool approval' : toolName,
    UserInputInteractionPayload() => 'User input requested',
    PlanConfirmationInteractionPayload() => 'Plan confirmation',
    UnknownInteractionPayload() => switch (kind) {
      InteractionKind.toolApproval => 'Tool approval',
      InteractionKind.userInput => 'User input requested',
      InteractionKind.planConfirmation => 'Plan confirmation',
    },
  };
}

String _interactionBody(InteractionKind kind, InteractionPayload payload) {
  return switch (payload) {
    ToolApprovalInteractionPayload(:final arguments) => _jsonText(arguments),
    UserInputInteractionPayload(:final questions) =>
      questions
          .map((question) => question.question)
          .where((question) => question.isNotEmpty)
          .join('\n'),
    PlanConfirmationInteractionPayload(:final content) => content,
    UnknownInteractionPayload() => '',
  };
}
