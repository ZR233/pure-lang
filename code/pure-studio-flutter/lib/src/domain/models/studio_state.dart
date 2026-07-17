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
    required this.permissionMode,
    required this.turnPhase,
    required this.runtime,
    required this.pendingInteractions,
    this.eventCursorsBySession = const {},
    this.composerText = '',
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
  final PermissionMode permissionMode;
  final TurnPhase turnPhase;
  final SessionRuntimeView runtime;
  final List<PendingInteraction> pendingInteractions;
  final Map<String, int> eventCursorsBySession;
  final String composerText;

  List<TimelineMessage> get selectedMessages {
    final sessionId = selectedSessionId;
    if (sessionId == null) {
      return const [];
    }
    return [...(messagesBySession[sessionId] ?? const [])]
      ..sort(_compareTimelineMessages);
  }

  List<TimelineRow> get selectedTimelineRows {
    final sessionId = selectedSessionId;
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
    PermissionMode? permissionMode,
    TurnPhase? turnPhase,
    SessionRuntimeView? runtime,
    List<PendingInteraction>? pendingInteractions,
    Map<String, int>? eventCursorsBySession,
    String? composerText,
  }) {
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
      selectedSessionId: identical(selectedSessionId, _studioStateUnset)
          ? this.selectedSessionId
          : selectedSessionId as String?,
      permissionMode: permissionMode ?? this.permissionMode,
      turnPhase: turnPhase ?? this.turnPhase,
      runtime: runtime ?? this.runtime,
      pendingInteractions: pendingInteractions ?? this.pendingInteractions,
      eventCursorsBySession:
          eventCursorsBySession ?? this.eventCursorsBySession,
      composerText: composerText ?? this.composerText,
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
