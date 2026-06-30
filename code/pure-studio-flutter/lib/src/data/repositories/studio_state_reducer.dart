import '../../domain/models/studio_models.dart';
import '../frb/studio_api.dart';

part 'studio_timeline_reducer.dart';

class StudioReduceResult {
  const StudioReduceResult(this.state, {this.staleSessionId});

  final StudioState state;
  final String? staleSessionId;
}

StudioReduceResult reduceStudioEvent(
  StudioState current,
  StudioBridgeEvent event,
) {
  return switch (event.payload) {
    MessageUpdatedPayload(:final message) => StudioReduceResult(
      _upsertMessageSnapshot(current, message),
    ),
    MessageRemovedPayload(:final messageId) => StudioReduceResult(
      _removeMessage(current, event.sessionId, messageId),
    ),
    MessagePartUpdatedPayload(:final part) => _upsertPartSnapshot(
      current,
      part,
    ),
    MessagePartRemovedPayload(:final messageId, :final partId) =>
      StudioReduceResult(
        _removePart(current, event.sessionId, messageId, partId),
      ),
    MessagePartDeltaPayload(:final delta) => _appendPartDelta(
      current,
      event.sessionId,
      delta,
    ),
    TurnChangedPayload(:final turn) => StudioReduceResult(
      current.copyWith(turnPhase: _turnPhase(turn)),
    ),
    InteractionChangedPayload(:final interaction, :final status) =>
      StudioReduceResult(_upsertInteraction(current, interaction, status)),
    SessionRuntimeChangedPayload(:final runtime) => StudioReduceResult(
      current.copyWith(
        runtime: runtime.copyWith(agentCount: current.runtime.agentCount),
      ),
    ),
    SessionListChangedPayload() => StudioReduceResult(
      _mergeSessionListChanged(current, event),
    ),
    AgentChangedPayload(:final agent) => StudioReduceResult(
      _applyAgentChanged(current, agent),
    ),
    AgentTimelineChangedPayload(:final event) => StudioReduceResult(
      _upsertAgentTimelineEvent(current, event),
    ),
    SkillActivatedPayload(:final name) => StudioReduceResult(
      _applySkillActivation(current, name),
    ),
    McpHealthChangedPayload(:final activeMcpServers, :final servers) =>
      StudioReduceResult(_applyMcpHealth(current, activeMcpServers, servers)),
    LspHealthChangedPayload(:final activeLspServers) => StudioReduceResult(
      _applyLspHealth(current, activeLspServers),
    ),
    PlanLifecycleChangedPayload(:final state) => StudioReduceResult(
      _applyPlanLifecycle(current, state),
    ),
    StalePayload() ||
    IgnoredBridgeEventPayload() ||
    SettingsDraftSavedPayload() => StudioReduceResult(current),
  };
}

StudioState mergeStudioSessionState(
  StudioState current,
  StudioState sessionState,
) {
  final sessionId = sessionState.selectedSessionId;
  var merged = current;
  if (sessionId != null) {
    for (final message
        in sessionState.messagesBySession[sessionId] ?? const []) {
      merged = _upsertMessageSnapshot(merged, message);
    }
    final snapshots = sessionState.partSnapshotsBySession[sessionId] ?? {};
    final orderedSnapshots = snapshots.values.toList()
      ..sort((a, b) {
        final order = a.order.compareTo(b.order);
        if (order != 0) {
          return order;
        }
        final sequence = a.sequence.compareTo(b.sequence);
        return sequence != 0 ? sequence : a.id.compareTo(b.id);
      });
    for (final snapshot in orderedSnapshots) {
      merged = _upsertPartSnapshot(
        merged,
        snapshot,
        recoverOnInvalid: false,
      ).state;
    }
    final agentEvents =
        sessionState.agentTimelineEventsBySession[sessionId] ?? const {};
    if (agentEvents.isNotEmpty) {
      merged = _withAgentTimelineEvents(merged, sessionId, {
        ...(merged.agentTimelineEventsBySession[sessionId] ?? const {}),
        ...agentEvents,
      });
    }
    final agents =
        sessionState.agentsBySession[sessionId] ??
        const <String, StudioAgentView>{};
    if (agents.isNotEmpty) {
      merged = _withAgents(merged, sessionId, agents);
    }
  }
  return merged.copyWith(
    sessions: sessionState.sessions.isEmpty
        ? merged.sessions
        : sessionState.sessions,
    selectedProjectId:
        sessionState.selectedProjectId ?? merged.selectedProjectId,
    selectedSessionId: sessionId ?? merged.selectedSessionId,
    runtime: sessionState.runtime,
    pendingInteractions: sessionState.pendingInteractions,
    eventCursorsBySession: _mergeEventCursors(
      merged.eventCursorsBySession,
      sessionState.eventCursorsBySession,
    ),
  );
}

StudioState mergeStudioConfigState(StudioState current, StudioState next) {
  return current.copyWith(
    providers: next.providers.isEmpty ? current.providers : next.providers,
    defaultProviderId: next.defaultProviderId,
    providerUsages: next.providerUsages.isEmpty
        ? current.providerUsages
        : next.providerUsages,
    roles: next.roles.isEmpty ? current.roles : next.roles,
    mcpServers: next.mcpServers.isEmpty ? current.mcpServers : next.mcpServers,
    instructions: next.instructions,
    skills: next.skills,
    general: next.general,
    permissionMode: next.permissionMode,
    runtime: next.runtime.model.isEmpty ? current.runtime : next.runtime,
  );
}

bool targetsSelectedSession(StudioState current, StudioBridgeEvent event) {
  final sessionId = studioEventSessionId(event);
  return sessionId == null ||
      current.selectedSessionId == null ||
      sessionId == current.selectedSessionId;
}

bool isDuplicateDurableEvent(StudioState current, StudioBridgeEvent event) {
  if (isLiveOnlyStudioEvent(event)) {
    return false;
  }
  final sessionId = studioEventSessionId(event);
  final sequence = event.sequence?.toInt();
  if (sessionId == null || sequence == null || sequence <= 0) {
    return false;
  }
  return sequence <= (current.eventCursorsBySession[sessionId] ?? 0);
}

StudioState withStudioEventCursor(
  StudioState current,
  StudioBridgeEvent event,
) {
  if (isLiveOnlyStudioEvent(event)) {
    return current;
  }
  final sessionId = studioEventSessionId(event);
  final sequence = event.sequence?.toInt();
  if (sessionId == null || sequence == null || sequence <= 0) {
    return current;
  }
  final existing = current.eventCursorsBySession[sessionId] ?? 0;
  if (sequence <= existing) {
    return current;
  }
  return current.copyWith(
    eventCursorsBySession: {
      ...current.eventCursorsBySession,
      sessionId: sequence,
    },
  );
}

bool isLiveOnlyStudioEvent(StudioBridgeEvent event) {
  return event.payload is MessagePartDeltaPayload ||
      event.payload is StalePayload;
}

String? studioEventSessionId(StudioBridgeEvent event) {
  if (event.sessionId != null && event.sessionId!.isNotEmpty) {
    return event.sessionId;
  }
  return event.payload.sessionId;
}

List<RoleSettingsView> replaceStudioRole(
  List<RoleSettingsView> roles,
  RoleSettingsView replacement,
) {
  var replaced = false;
  final next = [
    for (final role in roles)
      if (role.key == replacement.key) ...[replacement] else role,
  ];
  replaced = roles.any((role) => role.key == replacement.key);
  if (!replaced) {
    next.add(replacement);
  }
  return next;
}

String defaultEffortForModel(
  StudioState current,
  String providerId,
  String model,
) {
  for (final provider in current.providers) {
    if (provider.id != providerId) {
      continue;
    }
    for (final candidate in provider.models) {
      if (candidate.slug == model && candidate.reasoningEfforts.isNotEmpty) {
        return candidate.reasoningEfforts.first;
      }
    }
  }
  return current.role('planner')?.effort ?? 'high';
}

String planFollowUpPrompt(
  PendingInteraction interaction,
  Map<String, Object?> resolution,
) {
  final content = resolution['content']?.toString().trim() ?? '';
  if (content.isNotEmpty) {
    return content;
  }
  final reason = resolution['reason']?.toString().trim() ?? '';
  if (reason.isNotEmpty) {
    return reason;
  }
  return (interaction.payload['content'] ?? interaction.body).toString().trim();
}

StudioState _mergeSessionListChanged(
  StudioState current,
  StudioBridgeEvent event,
) {
  final payload = event.payload;
  if (payload is! SessionListChangedPayload) {
    return current;
  }
  final incoming = payload.sessions;
  final projectId = payload.projectId;
  if (projectId == null && incoming.isEmpty) {
    return current.copyWith(sessions: const []);
  }
  final Set<String> projectIds = projectId == null
      ? incoming.map((session) => session.projectId).toSet()
      : {projectId};
  final sessions = [
    for (final session in current.sessions)
      if (!projectIds.contains(session.projectId)) session,
    ...incoming,
  ];
  return current.copyWith(sessions: sessions);
}

Map<String, int> _mergeEventCursors(
  Map<String, int> current,
  Map<String, int> incoming,
) {
  final merged = {...current};
  for (final entry in incoming.entries) {
    final existing = merged[entry.key] ?? 0;
    if (entry.value > existing) {
      merged[entry.key] = entry.value;
    }
  }
  return merged;
}

StudioState _applyAgentChanged(StudioState current, StudioAgentView agent) {
  if (agent.sessionId.isNotEmpty &&
      agent.sessionId != current.selectedSessionId) {
    return current;
  }
  final sessionId = agent.sessionId;
  if (sessionId.isEmpty || agent.id.isEmpty) {
    return current;
  }
  final agents = <String, StudioAgentView>{
    ...(current.agentsBySession[sessionId] ?? const {}),
    agent.id: agent,
  };
  return _withAgents(
    current.copyWith(
      runtime: current.runtime.copyWith(agentCount: agents.length),
    ),
    sessionId,
    agents,
  );
}

StudioState _upsertAgentTimelineEvent(
  StudioState current,
  TimelineAgentEvent event,
) {
  if (event.sessionId.isEmpty ||
      event.sessionId != current.selectedSessionId ||
      event.eventId.isEmpty) {
    return current;
  }
  return _withAgentTimelineEvent(current, event);
}

StudioState _applySkillActivation(StudioState current, String name) {
  if (name.isEmpty || current.runtime.activeSkills.contains(name)) {
    return current;
  }
  return current.copyWith(
    runtime: current.runtime.copyWith(
      activeSkills: [...current.runtime.activeSkills, name]..sort(),
    ),
  );
}

StudioState _applyMcpHealth(
  StudioState current,
  List<String> activeMcpServers,
  List<McpServerSettingsView> servers,
) {
  return current.copyWith(
    runtime: current.runtime.copyWith(activeMcpServers: activeMcpServers),
    mcpServers: servers.isEmpty ? current.mcpServers : servers,
  );
}

StudioState _applyLspHealth(
  StudioState current,
  List<String> activeLspServers,
) {
  return current.copyWith(
    runtime: current.runtime.copyWith(activeLspServers: activeLspServers),
  );
}

StudioState _applyPlanLifecycle(StudioState current, String planState) {
  return current.copyWith(
    turnPhase: switch (planState) {
      'pendingConfirmation' => TurnPhase.waitingForInteraction,
      'accepted' || 'implementing' => TurnPhase.runningTool,
      'implementationFailed' => TurnPhase.failed,
      'cancelled' => TurnPhase.cancelled,
      'implemented' ||
      'continuedPlanning' ||
      'dismissed' => TurnPhase.completed,
      _ => current.turnPhase,
    },
  );
}

StudioState _upsertInteraction(
  StudioState current,
  PendingInteraction interaction,
  String status,
) {
  if (interaction.id.isEmpty) {
    return current;
  }
  final interactions = [...current.pendingInteractions];
  final index = interactions.indexWhere(
    (candidate) => candidate.id == interaction.id,
  );
  if (status != 'pending') {
    if (index >= 0) {
      interactions.removeAt(index);
    }
    return current.copyWith(pendingInteractions: interactions);
  }
  if (index >= 0) {
    interactions[index] = interaction;
  } else {
    interactions.add(interaction);
  }
  return current.copyWith(pendingInteractions: interactions);
}

List<TimelineMessage> _messagesFor(StudioState state, String sessionId) {
  return [...(state.messagesBySession[sessionId] ?? const [])];
}

StudioState _withMessages(
  StudioState state,
  String sessionId,
  List<TimelineMessage> messages,
) {
  final bySession = Map<String, List<TimelineMessage>>.from(
    state.messagesBySession,
  );
  bySession[sessionId] = messages;
  return state.copyWith(messagesBySession: bySession);
}

StudioState _withPartState(
  StudioState state,
  String sessionId,
  Map<String, TimelinePartSnapshot> snapshots,
  Map<String, TimelinePartOverlay> overlays,
) {
  return state.copyWith(
    partSnapshotsBySession: {
      ...state.partSnapshotsBySession,
      sessionId: snapshots,
    },
    partOverlaysBySession: {
      ...state.partOverlaysBySession,
      sessionId: overlays,
    },
  );
}

StudioState _withMessageAndPartState(
  StudioState state,
  String sessionId,
  List<TimelineMessage> messages,
  Map<String, TimelinePartSnapshot> snapshots,
  Map<String, TimelinePartOverlay> overlays,
) {
  final withMessages = _withMessages(state, sessionId, messages);
  return _withPartState(withMessages, sessionId, snapshots, overlays);
}

StudioState _withAgentTimelineEvent(
  StudioState state,
  TimelineAgentEvent event,
) {
  final events = {
    ...(state.agentTimelineEventsBySession[event.sessionId] ?? const {}),
    event.eventId: event,
  };
  return _withAgentTimelineEvents(state, event.sessionId, events);
}

StudioState _withAgentTimelineEvents(
  StudioState state,
  String sessionId,
  Map<String, TimelineAgentEvent> events,
) {
  return state.copyWith(
    agentTimelineEventsBySession: {
      ...state.agentTimelineEventsBySession,
      sessionId: events,
    },
  );
}

StudioState _withAgents(
  StudioState state,
  String sessionId,
  Map<String, StudioAgentView> agents,
) {
  return state.copyWith(
    agentsBySession: {...state.agentsBySession, sessionId: agents},
  );
}

TurnPhase _turnPhase(StudioTurnView turn) {
  return switch (turn.status) {
    'queued' => TurnPhase.queued,
    'contextLoading' => TurnPhase.contextLoading,
    'waitingForModel' => TurnPhase.waitingForModel,
    'streaming' => TurnPhase.streaming,
    'waitingForInteraction' => TurnPhase.waitingForInteraction,
    'runningTool' => TurnPhase.runningTool,
    'completed' => TurnPhase.completed,
    'failed' => TurnPhase.failed,
    'cancelled' => TurnPhase.cancelled,
    _ => TurnPhase.idle,
  };
}
