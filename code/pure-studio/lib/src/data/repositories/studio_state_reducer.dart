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
      _withTurn(current, studioEventSessionId(event) ?? turn.sessionId, turn),
    ),
    InteractionChangedPayload(:final interaction, :final status) =>
      StudioReduceResult(_upsertInteraction(current, interaction, status)),
    SessionRuntimeChangedPayload(
      :final runtime,
      :final agentCount,
      :final sessionId,
    ) =>
      StudioReduceResult(
        _withRuntime(
          current,
          sessionId,
          runtime.copyWith(
            agentCount:
                agentCount ?? _runtimeFor(current, sessionId).agentCount,
            task: _runtimeFor(current, sessionId).task,
          ),
        ),
      ),
    SessionListChangedPayload() => StudioReduceResult(
      _mergeSessionListChanged(current, event),
    ),
    SessionTaskChangedPayload(:final sessionId, :final task) =>
      StudioReduceResult(
        _withRuntime(
          current,
          sessionId,
          _runtimeFor(current, sessionId).copyWith(task: task),
        ),
      ),
    AgentChangedPayload(:final agent) => StudioReduceResult(
      _applyAgentChanged(current, agent),
    ),
    AgentDirectoryChangedPayload(:final rootSessionId, :final agent) =>
      StudioReduceResult(
        _applyAgentDirectoryChanged(current, rootSessionId, agent),
      ),
    AgentTimelineChangedPayload(:final event) => StudioReduceResult(
      _upsertAgentTimelineEvent(current, event),
    ),
    SkillActivatedPayload(:final name) => StudioReduceResult(
      _applySkillActivation(
        current,
        studioEventSessionId(event) ?? current.selectedSessionId,
        name,
      ),
    ),
    McpHealthChangedPayload(:final activeMcpServers, :final servers) =>
      StudioReduceResult(
        _applyMcpHealth(
          current,
          studioEventSessionId(event) ?? current.selectedSessionId,
          activeMcpServers,
          servers,
        ),
      ),
    LspHealthChangedPayload(:final activeLspServers) => StudioReduceResult(
      _applyLspHealth(
        current,
        studioEventSessionId(event) ?? current.selectedSessionId,
        activeLspServers,
      ),
    ),
    PlanLifecycleChangedPayload() => StudioReduceResult(current),
    StalePayload() ||
    IgnoredBridgeEventPayload() ||
    SettingsDraftSavedPayload() => StudioReduceResult(current),
  };
}

/// 合并历史页中可回放的 durable timeline 事实，不触碰当前 turn/runtime/interaction。
StudioState mergeSessionHistoryPage(
  StudioState current,
  String sessionId,
  SessionHistoryPage page,
) {
  final items = [for (final turn in page.turns) ...turn.items]
    ..sort((left, right) => left.sequence.compareTo(right.sequence));
  var merged = current;
  for (final item in items) {
    final event = item.event;
    if (studioEventSessionId(event) != sessionId) {
      continue;
    }
    merged = switch (event.payload) {
      MessageUpdatedPayload(:final message) => _upsertMessageSnapshot(
        merged,
        message,
      ),
      MessageRemovedPayload(:final messageId) => _removeHistoricalMessage(
        merged,
        sessionId,
        messageId,
        item.sequence,
      ),
      MessagePartUpdatedPayload(:final part) => _upsertPartSnapshot(
        merged,
        part,
        recoverOnInvalid: false,
      ).state,
      MessagePartRemovedPayload(:final messageId, :final partId) =>
        _removeHistoricalPart(
          merged,
          sessionId,
          messageId,
          partId,
          item.sequence,
        ),
      AgentTimelineChangedPayload(:final event) => _withAgentTimelineEvent(
        merged,
        event,
      ),
      TurnChangedPayload() ||
      InteractionChangedPayload() ||
      SessionRuntimeChangedPayload() ||
      SessionListChangedPayload() ||
      SessionTaskChangedPayload() ||
      AgentChangedPayload() ||
      AgentDirectoryChangedPayload() ||
      SkillActivatedPayload() ||
      McpHealthChangedPayload() ||
      LspHealthChangedPayload() ||
      PlanLifecycleChangedPayload() ||
      MessagePartDeltaPayload() ||
      StalePayload() ||
      IgnoredBridgeEventPayload() ||
      SettingsDraftSavedPayload() => merged,
    };
  }
  return merged;
}

StudioState _removeHistoricalMessage(
  StudioState current,
  String sessionId,
  String messageId,
  int removeSequence,
) {
  final existing = current.messagesBySession[sessionId]
      ?.where((message) => message.id == messageId)
      .firstOrNull;
  return existing != null && existing.sequence > removeSequence
      ? current
      : _removeMessage(current, sessionId, messageId);
}

StudioState _removeHistoricalPart(
  StudioState current,
  String sessionId,
  String messageId,
  String partId,
  int removeSequence,
) {
  final existing = current.partSnapshotsBySession[sessionId]?[partId];
  return existing != null && existing.sequence > removeSequence
      ? current
      : _removePart(current, sessionId, messageId, partId);
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
    selectedRootSessionId:
        sessionState.selectedRootSession?.id ?? merged.selectedRootSession?.id,
    pendingInteractions: sessionState.pendingInteractions,
    eventCursorsBySession: _mergeEventCursors(
      merged.eventCursorsBySession,
      sessionState.eventCursorsBySession,
    ),
  );
}

StudioState mergeStudioConfigState(StudioState current, StudioState next) {
  final nextProviders = next.providers.isEmpty
      ? current.providers
      : next.providers;
  return current.copyWith(
    providers: [
      for (final provider in nextProviders)
        providerWithCatalogMetadata(provider, current.providerCatalog),
    ],
    defaultProviderId: next.defaultProviderId,
    providerUsages: next.providerUsages.isEmpty
        ? current.providerUsages
        : next.providerUsages,
    roles: next.roles.isEmpty ? current.roles : next.roles,
    mcpServers: next.mcpServers.isEmpty ? current.mcpServers : next.mcpServers,
    instructions: next.instructions,
    skills: next.skills,
    general: next.general,
    webSearch: next.webSearch,
    permissionMode: next.permissionMode,
    runtimesBySession: {
      ...current.runtimesBySession,
      ...next.runtimesBySession,
    },
  );
}

bool targetsSelectedSession(StudioState current, StudioBridgeEvent event) {
  if (event.payload is AgentDirectoryChangedPayload) {
    return true;
  }
  final sessionId = studioEventSessionId(event);
  return sessionId == null ||
      current.selectedSessionId == null ||
      sessionId == current.selectedSessionId;
}

bool isDuplicateDurableEvent(StudioState current, StudioBridgeEvent event) {
  if (event.origin != StudioBridgeEventOrigin.session ||
      isLiveOnlyStudioEvent(event)) {
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
  if (event.origin != StudioBridgeEventOrigin.session ||
      isLiveOnlyStudioEvent(event)) {
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
  InteractionResolutionCommand resolution,
) {
  final content = resolution is PlanConfirmationResolutionCommand
      ? resolution.content?.trim() ?? ''
      : '';
  if (content.isNotEmpty) {
    return content;
  }
  final reason = resolution is PlanConfirmationResolutionCommand
      ? resolution.reason?.trim() ?? ''
      : resolution is ToolApprovalResolutionCommand
      ? resolution.reason?.trim() ?? ''
      : '';
  if (reason.isNotEmpty) {
    return reason;
  }
  return switch (interaction.payload) {
    PlanConfirmationInteractionPayload(:final content)
        when content.trim().isNotEmpty =>
      content.trim(),
    _ => interaction.body.trim(),
  };
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
  var selectedSessionId = current.selectedSessionId;
  var selectedRootSessionId = current.selectedRootSession?.id;
  final selected = sessions
      .where((session) => session.id == selectedSessionId)
      .firstOrNull;
  if (selected != null) {
    selectedRootSessionId = selected.effectiveRootSessionId;
  } else if (selectedSessionId != null &&
      !sessions.any((session) => session.id == selectedSessionId)) {
    final fallbackRoot = sessions
        .where(
          (session) =>
              session.isRoot &&
              projectIds.contains(session.projectId) &&
              (selectedRootSessionId == null ||
                  session.id == selectedRootSessionId),
        )
        .firstOrNull;
    final anyRoot =
        fallbackRoot ??
        sessions
            .where(
              (session) =>
                  session.isRoot && projectIds.contains(session.projectId),
            )
            .firstOrNull;
    selectedSessionId = anyRoot?.id;
    selectedRootSessionId = anyRoot?.id;
  }
  return current.copyWith(
    sessions: sessions,
    selectedSessionId: selectedSessionId,
    selectedRootSessionId: selectedRootSessionId,
  );
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
  final sessionId = agent.sessionId;
  if (sessionId.isEmpty || agent.id.isEmpty) {
    return current;
  }
  final agents = <String, StudioAgentView>{
    ...(current.agentsBySession[sessionId] ?? const {}),
    agent.id: agent,
  };
  return _withAgents(
    _withRuntime(
      current,
      sessionId,
      _runtimeFor(current, sessionId).copyWith(agentCount: agents.length),
    ),
    sessionId,
    agents,
  );
}

StudioState _applyAgentDirectoryChanged(
  StudioState current,
  String rootSessionId,
  StudioAgentView agent,
) {
  if (agent.id.isEmpty || agent.sessionId.isEmpty || rootSessionId.isEmpty) {
    return current;
  }
  var next = current;
  for (final sessionId in {rootSessionId, agent.sessionId}) {
    final agents = <String, StudioAgentView>{
      ...(next.agentsBySession[sessionId] ?? const {}),
      agent.id: agent,
    };
    next = _withAgents(
      _withRuntime(
        next,
        sessionId,
        _runtimeFor(next, sessionId).copyWith(agentCount: agents.length),
      ),
      sessionId,
      agents,
    );
  }
  return next;
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

StudioState _applySkillActivation(
  StudioState current,
  String? sessionId,
  String name,
) {
  if (sessionId == null || name.isEmpty) {
    return current;
  }
  final runtime = _runtimeFor(current, sessionId);
  if (runtime.activeSkills.contains(name)) {
    return current;
  }
  return _withRuntime(
    current,
    sessionId,
    runtime.copyWith(activeSkills: [...runtime.activeSkills, name]..sort()),
  );
}

StudioState _applyMcpHealth(
  StudioState current,
  String? sessionId,
  List<String> activeMcpServers,
  List<McpServerSettingsView> servers,
) {
  final next = current.copyWith(
    mcpServers: servers.isEmpty ? current.mcpServers : servers,
  );
  return sessionId == null
      ? next
      : _withRuntime(
          next,
          sessionId,
          _runtimeFor(
            next,
            sessionId,
          ).copyWith(activeMcpServers: activeMcpServers),
        );
}

StudioState _applyLspHealth(
  StudioState current,
  String? sessionId,
  List<String> activeLspServers,
) {
  return sessionId == null
      ? current
      : _withRuntime(
          current,
          sessionId,
          _runtimeFor(
            current,
            sessionId,
          ).copyWith(activeLspServers: activeLspServers),
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

SessionRuntimeView _runtimeFor(StudioState state, String sessionId) {
  return state.runtimesBySession[sessionId] ??
      const SessionRuntimeView(
        model: '',
        contextTokens: 0,
        contextWindow: 0,
        totalTokens: 0,
        costLabel: '',
        activeSkills: [],
        activeMcpServers: [],
        activeLspServers: [],
        agentCount: 0,
      );
}

StudioState _withRuntime(
  StudioState state,
  String sessionId,
  SessionRuntimeView runtime,
) {
  if (sessionId.isEmpty) {
    return state;
  }
  return state.copyWith(
    runtimesBySession: {...state.runtimesBySession, sessionId: runtime},
  );
}

StudioState _withTurn(
  StudioState state,
  String sessionId,
  StudioTurnView turn,
) {
  if (sessionId.isEmpty) {
    return state;
  }
  return state.copyWith(
    turnsBySession: {...state.turnsBySession, sessionId: turn},
  );
}
