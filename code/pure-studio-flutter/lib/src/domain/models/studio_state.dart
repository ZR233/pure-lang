import 'agent_models.dart';
import 'collection_extensions.dart';
import 'interaction_models.dart';
import 'provider_models.dart';
import 'runtime_models.dart';
import 'session_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'timeline_models.dart';

class _StudioStateUnset {
  const _StudioStateUnset();
}

const _studioStateUnset = _StudioStateUnset();
const _emptySessionRuntime = SessionRuntimeView(
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

class StudioState {
  const StudioState({
    required this.projects,
    required this.sessions,
    required this.messagesBySession,
    this.partSnapshotsBySession = const {},
    this.partOverlaysBySession = const {},
    this.agentTimelineEventsBySession = const {},
    this.agentsBySession = const {},
    required this.providers,
    this.providerCatalog = const ProviderCatalogView.empty(),
    this.defaultProviderId,
    this.providerUsages = const [],
    required this.roles,
    required this.mcpServers,
    this.instructions = const InstructionsSettingsView(),
    this.skills = const SkillsSettingsView(),
    this.general = const GeneralSettingsView(),
    this.webSearch = const WebSearchSettingsView(),
    required this.selectedProjectId,
    required this.selectedSessionId,
    this.selectedRootSessionId,
    required this.permissionMode,
    required this.turnPhase,
    required this.runtime,
    this.turnPhasesBySession = const {},
    this.runtimesBySession = const {},
    required this.pendingInteractions,
    this.eventCursorsBySession = const {},
    this.composerText = '',
    this.composerTextsBySession = const {},
  });

  final List<StudioProject> projects;
  final List<StudioSession> sessions;
  final Map<String, List<TimelineMessage>> messagesBySession;
  final Map<String, Map<String, TimelinePartSnapshot>> partSnapshotsBySession;
  final Map<String, Map<String, TimelinePartOverlay>> partOverlaysBySession;
  final Map<String, Map<String, TimelineAgentEvent>>
  agentTimelineEventsBySession;
  final Map<String, Map<String, StudioAgentView>> agentsBySession;
  final List<ProviderSettingsView> providers;
  final ProviderCatalogView providerCatalog;
  final String? defaultProviderId;
  final List<ProviderUsageView> providerUsages;
  final List<RoleSettingsView> roles;
  final List<McpServerSettingsView> mcpServers;
  final InstructionsSettingsView instructions;
  final SkillsSettingsView skills;
  final GeneralSettingsView general;
  final WebSearchSettingsView webSearch;
  final String? selectedProjectId;
  final String? selectedSessionId;
  final String? selectedRootSessionId;
  final PermissionMode permissionMode;
  final TurnPhase turnPhase;
  final SessionRuntimeView runtime;
  final Map<String, TurnPhase> turnPhasesBySession;
  final Map<String, SessionRuntimeView> runtimesBySession;
  final List<PendingInteraction> pendingInteractions;
  final Map<String, int> eventCursorsBySession;
  final String composerText;
  final Map<String, String> composerTextsBySession;

  String? get selectedAgentSessionId => selectedSessionId;

  List<StudioSession> get rootSessions =>
      sessions.where((session) => session.isRoot).toList();

  StudioSession? get selectedAgentSession {
    final sessionId = selectedSessionId;
    if (sessionId == null) {
      return null;
    }
    return sessions.where((session) => session.id == sessionId).firstOrNull;
  }

  StudioSession? get selectedRootSession {
    final explicitRootId = selectedRootSessionId;
    if (explicitRootId != null) {
      final explicit = sessions
          .where((session) => session.id == explicitRootId && session.isRoot)
          .firstOrNull;
      if (explicit != null) {
        return explicit;
      }
    }
    final selected = selectedAgentSession;
    final rootId = selected?.effectiveRootSessionId;
    if (rootId != null) {
      return sessions
          .where((session) => session.id == rootId && session.isRoot)
          .firstOrNull;
    }
    return null;
  }

  String? get selectedTimelineSessionId {
    final selected = selectedAgentSession;
    if (selected == null) {
      return null;
    }
    final role = selected.ownerRole.trim().toLowerCase();
    if (selected.isAgent && role == 'executor') {
      return selectedRootSession?.id ?? selected.id;
    }
    return selected.id;
  }

  List<StudioSession> get agentSessionsForSelectedRoot {
    final root = selectedRootSession;
    if (root == null) {
      return const [];
    }
    final scoped = sessions
        .where((session) => session.effectiveRootSessionId == root.id)
        .toList();
    final children = <String?, List<StudioSession>>{};
    for (final session in scoped) {
      children.putIfAbsent(session.parentSessionId, () => []).add(session);
    }
    for (final siblings in children.values) {
      siblings.sort((left, right) {
        final created = left.effectiveCreatedAt.compareTo(
          right.effectiveCreatedAt,
        );
        return created != 0 ? created : left.id.compareTo(right.id);
      });
    }
    final ordered = <StudioSession>[];
    final visited = <String>{};
    void appendBranch(StudioSession session) {
      if (!visited.add(session.id)) {
        return;
      }
      ordered.add(session);
      for (final child in children[session.id] ?? const <StudioSession>[]) {
        appendBranch(child);
      }
    }

    appendBranch(root);
    for (final session in scoped) {
      appendBranch(session);
    }
    return ordered;
  }

  List<TimelineMessage> get selectedMessages {
    final sessionId = selectedTimelineSessionId;
    if (sessionId == null) {
      return const [];
    }
    return [...(messagesBySession[sessionId] ?? const [])]
      ..sort(_compareTimelineMessages);
  }

  List<TimelineRow> get selectedTimelineRows {
    final sessionId = selectedTimelineSessionId;
    if (sessionId == null) {
      return const [];
    }
    final snapshots = partSnapshotsBySession[sessionId] ?? const {};
    final overlays = partOverlaysBySession[sessionId] ?? const {};
    return timelineRowsFromMessages(
      selectedMessages,
      parts: [
        for (final snapshot in snapshots.values)
          timelinePartFromSnapshot(snapshot, overlay: overlays[snapshot.id]),
      ],
      agentEvents: agentTimelineEventsBySession[sessionId]?.values ?? const [],
    );
  }

  TimelineTodoListUpdate? get selectedTodoList {
    final sessionId = selectedTimelineSessionId;
    if (sessionId == null) {
      return null;
    }
    final updates =
        (agentTimelineEventsBySession[sessionId]?.values ?? const [])
            .where((event) => event.payload is TimelineTodoListUpdate)
            .toList()
          ..sort(compareTimelineAgentEvents);
    return updates.isEmpty
        ? null
        : updates.last.payload as TimelineTodoListUpdate;
  }

  PendingInteraction? get activeInteraction {
    final sessionId = selectedSessionId;
    if (sessionId == null) {
      return null;
    }
    final scoped = pendingInteractions
        .where((interaction) => interaction.sessionId == sessionId)
        .toList();
    scoped.sort(
      (a, b) =>
          interactionPriority(a.kind).compareTo(interactionPriority(b.kind)),
    );
    return scoped.firstOrNull;
  }

  List<StudioAgentView> get selectedAgents {
    final sessionId = selectedSessionId;
    if (sessionId == null) {
      return const [];
    }
    final agents = [
      ...(agentsBySession[sessionId]?.values ??
          const Iterable<StudioAgentView>.empty()),
    ];
    agents.sort((left, right) {
      final path = left.path.compareTo(right.path);
      if (path != 0) {
        return path;
      }
      return left.id.compareTo(right.id);
    });
    return agents;
  }

  RoleSettingsView? role(String key) {
    return roles.where((role) => role.key == key).firstOrNull;
  }

  bool get isBusy {
    return switch (turnPhase) {
      TurnPhase.queued ||
      TurnPhase.contextLoading ||
      TurnPhase.waitingForModel ||
      TurnPhase.streaming ||
      TurnPhase.waitingForInteraction ||
      TurnPhase.runningTool => true,
      TurnPhase.idle ||
      TurnPhase.completed ||
      TurnPhase.failed ||
      TurnPhase.cancelled => false,
    };
  }

  StudioState copyWith({
    List<StudioProject>? projects,
    List<StudioSession>? sessions,
    Map<String, List<TimelineMessage>>? messagesBySession,
    Map<String, Map<String, TimelinePartSnapshot>>? partSnapshotsBySession,
    Map<String, Map<String, TimelinePartOverlay>>? partOverlaysBySession,
    Map<String, Map<String, TimelineAgentEvent>>? agentTimelineEventsBySession,
    Map<String, Map<String, StudioAgentView>>? agentsBySession,
    List<ProviderSettingsView>? providers,
    ProviderCatalogView? providerCatalog,
    Object? defaultProviderId = _studioStateUnset,
    List<ProviderUsageView>? providerUsages,
    List<RoleSettingsView>? roles,
    List<McpServerSettingsView>? mcpServers,
    InstructionsSettingsView? instructions,
    SkillsSettingsView? skills,
    GeneralSettingsView? general,
    WebSearchSettingsView? webSearch,
    Object? selectedProjectId = _studioStateUnset,
    Object? selectedSessionId = _studioStateUnset,
    Object? selectedRootSessionId = _studioStateUnset,
    PermissionMode? permissionMode,
    TurnPhase? turnPhase,
    SessionRuntimeView? runtime,
    Map<String, TurnPhase>? turnPhasesBySession,
    Map<String, SessionRuntimeView>? runtimesBySession,
    List<PendingInteraction>? pendingInteractions,
    Map<String, int>? eventCursorsBySession,
    String? composerText,
    Map<String, String>? composerTextsBySession,
  }) {
    final nextSelectedSessionId =
        identical(selectedSessionId, _studioStateUnset)
        ? this.selectedSessionId
        : selectedSessionId as String?;
    final nextTurnPhases = {
      ...this.turnPhasesBySession,
      ...?turnPhasesBySession,
    };
    final nextRuntimes = {...this.runtimesBySession, ...?runtimesBySession};
    final currentSessionId = this.selectedSessionId;
    if (currentSessionId != null) {
      nextTurnPhases[currentSessionId] = this.turnPhase;
      nextRuntimes[currentSessionId] = this.runtime;
    }
    if (nextSelectedSessionId != null) {
      if (turnPhase != null) {
        nextTurnPhases[nextSelectedSessionId] = turnPhase;
      }
      if (runtime != null) {
        nextRuntimes[nextSelectedSessionId] = runtime;
      }
    }
    final selectedTurnPhase =
        turnPhase ??
        (nextSelectedSessionId == this.selectedSessionId
            ? this.turnPhase
            : nextTurnPhases[nextSelectedSessionId] ?? TurnPhase.idle);
    final selectedRuntime =
        runtime ??
        (nextSelectedSessionId == this.selectedSessionId
            ? this.runtime
            : nextRuntimes[nextSelectedSessionId] ?? _emptySessionRuntime);
    return StudioState(
      projects: projects ?? this.projects,
      sessions: sessions ?? this.sessions,
      messagesBySession: messagesBySession ?? this.messagesBySession,
      partSnapshotsBySession:
          partSnapshotsBySession ?? this.partSnapshotsBySession,
      partOverlaysBySession:
          partOverlaysBySession ?? this.partOverlaysBySession,
      agentTimelineEventsBySession:
          agentTimelineEventsBySession ?? this.agentTimelineEventsBySession,
      agentsBySession: agentsBySession ?? this.agentsBySession,
      providers: providers ?? this.providers,
      providerCatalog: providerCatalog ?? this.providerCatalog,
      defaultProviderId: identical(defaultProviderId, _studioStateUnset)
          ? this.defaultProviderId
          : defaultProviderId as String?,
      providerUsages: providerUsages ?? this.providerUsages,
      roles: roles ?? this.roles,
      mcpServers: mcpServers ?? this.mcpServers,
      instructions: instructions ?? this.instructions,
      skills: skills ?? this.skills,
      general: general ?? this.general,
      webSearch: webSearch ?? this.webSearch,
      selectedProjectId: identical(selectedProjectId, _studioStateUnset)
          ? this.selectedProjectId
          : selectedProjectId as String?,
      selectedSessionId: nextSelectedSessionId,
      selectedRootSessionId: identical(selectedRootSessionId, _studioStateUnset)
          ? this.selectedRootSessionId
          : selectedRootSessionId as String?,
      permissionMode: permissionMode ?? this.permissionMode,
      turnPhase: selectedTurnPhase,
      runtime: selectedRuntime,
      turnPhasesBySession: nextTurnPhases,
      runtimesBySession: nextRuntimes,
      pendingInteractions: pendingInteractions ?? this.pendingInteractions,
      eventCursorsBySession:
          eventCursorsBySession ?? this.eventCursorsBySession,
      composerText: composerText ?? this.composerText,
      composerTextsBySession:
          composerTextsBySession ?? this.composerTextsBySession,
    );
  }
}

int _compareTimelineMessages(TimelineMessage left, TimelineMessage right) {
  final sequence = left.sequence.compareTo(right.sequence);
  if (sequence != 0) {
    return sequence;
  }
  final createdAt = left.createdAt.compareTo(right.createdAt);
  if (createdAt != 0) {
    return createdAt;
  }
  return left.id.compareTo(right.id);
}
