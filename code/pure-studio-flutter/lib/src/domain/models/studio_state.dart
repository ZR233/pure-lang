import 'agent_models.dart';
import 'agent_workspace_view.dart';
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
  StudioState({
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
    this.turnPhasesBySession = const {},
    this.runtimesBySession = const {},
    Map<String, AgentWorkspaceSyncState> workspaceSyncBySession = const {},
    required this.pendingInteractions,
    this.eventCursorsBySession = const {},
    this.composerTextsBySession = const {},
  }) : workspaceSyncBySession = _withInitialWorkspaceSync(
         workspaceSyncBySession,
         selectedSessionId,
         selectedSessionId != null &&
             (turnPhasesBySession.containsKey(selectedSessionId) ||
                 runtimesBySession.containsKey(selectedSessionId) ||
                 composerTextsBySession.containsKey(selectedSessionId)),
       );

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
  final Map<String, TurnPhase> turnPhasesBySession;
  final Map<String, SessionRuntimeView> runtimesBySession;
  final Map<String, AgentWorkspaceSyncState> workspaceSyncBySession;
  final List<PendingInteraction> pendingInteractions;
  final Map<String, int> eventCursorsBySession;
  final Map<String, String> composerTextsBySession;

  String? get selectedAgentSessionId => selectedSessionId;

  TurnPhase get turnPhase {
    final sessionId = selectedSessionId;
    return sessionId == null
        ? TurnPhase.idle
        : turnPhasesBySession[sessionId] ?? TurnPhase.idle;
  }

  SessionRuntimeView get runtime {
    final sessionId = selectedSessionId;
    return sessionId == null
        ? _emptySessionRuntime
        : runtimesBySession[sessionId] ?? _emptySessionRuntime;
  }

  String get composerText {
    final sessionId = selectedSessionId;
    return sessionId == null ? '' : composerTextsBySession[sessionId] ?? '';
  }

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

  String? get selectedTimelineSessionId => selectedAgentSession?.id;

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

  AgentWorkspaceView? get selectedAgentWorkspace {
    final session = selectedAgentSession;
    final rootSession = selectedRootSession;
    if (session == null || rootSession == null) {
      return null;
    }
    return AgentWorkspaceView(
      session: session,
      rootSession: rootSession,
      syncState:
          workspaceSyncBySession[session.id] ?? AgentWorkspaceSyncState.loading,
      timelineRows: selectedTimelineRows,
      todo: selectedTodoList,
      runtime: runtime,
      turnPhase: turnPhase,
      activeInteraction: activeInteraction,
      composerText: composerText,
      composerMode: session.isAgent
          ? AgentComposerMode.runtimeDriven
          : AgentComposerMode.editable,
      permissionMode: permissionMode,
      providers: providers,
      roles: roles,
      agents: selectedAgents,
    );
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
    Map<String, TurnPhase>? turnPhasesBySession,
    Map<String, SessionRuntimeView>? runtimesBySession,
    Map<String, AgentWorkspaceSyncState>? workspaceSyncBySession,
    List<PendingInteraction>? pendingInteractions,
    Map<String, int>? eventCursorsBySession,
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
    final nextWorkspaceSync = {
      ...this.workspaceSyncBySession,
      ...?workspaceSyncBySession,
    };
    final nextComposerTexts = {
      ...this.composerTextsBySession,
      ...?composerTextsBySession,
    };
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
      turnPhasesBySession: nextTurnPhases,
      runtimesBySession: nextRuntimes,
      workspaceSyncBySession: nextWorkspaceSync,
      pendingInteractions: pendingInteractions ?? this.pendingInteractions,
      eventCursorsBySession:
          eventCursorsBySession ?? this.eventCursorsBySession,
      composerTextsBySession: nextComposerTexts,
    );
  }
}

Map<String, AgentWorkspaceSyncState> _withInitialWorkspaceSync(
  Map<String, AgentWorkspaceSyncState> values,
  String? selectedSessionId,
  bool hasSelectedWorkspace,
) {
  if (selectedSessionId == null ||
      !hasSelectedWorkspace ||
      values.containsKey(selectedSessionId)) {
    return values;
  }
  return {...values, selectedSessionId: AgentWorkspaceSyncState.ready};
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
