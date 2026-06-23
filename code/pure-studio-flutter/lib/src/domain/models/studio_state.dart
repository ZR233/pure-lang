import 'collection_extensions.dart';
import 'interaction_models.dart';
import 'provider_models.dart';
import 'runtime_models.dart';
import 'session_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'timeline_models.dart';

class StudioState {
  const StudioState({
    required this.projects,
    required this.sessions,
    required this.messagesBySession,
    required this.providers,
    this.providerUsages = const [],
    required this.roles,
    required this.mcpServers,
    this.instructions = const InstructionsSettingsView(),
    this.skills = const SkillsSettingsView(),
    this.general = const GeneralSettingsView(),
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
  final List<ProviderSettingsView> providers;
  final List<ProviderUsageView> providerUsages;
  final List<RoleSettingsView> roles;
  final List<McpServerSettingsView> mcpServers;
  final InstructionsSettingsView instructions;
  final SkillsSettingsView skills;
  final GeneralSettingsView general;
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
    return messagesBySession[sessionId] ?? const [];
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
    List<ProviderSettingsView>? providers,
    List<ProviderUsageView>? providerUsages,
    List<RoleSettingsView>? roles,
    List<McpServerSettingsView>? mcpServers,
    InstructionsSettingsView? instructions,
    SkillsSettingsView? skills,
    GeneralSettingsView? general,
    String? selectedProjectId,
    String? selectedSessionId,
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
      providers: providers ?? this.providers,
      providerUsages: providerUsages ?? this.providerUsages,
      roles: roles ?? this.roles,
      mcpServers: mcpServers ?? this.mcpServers,
      instructions: instructions ?? this.instructions,
      skills: skills ?? this.skills,
      general: general ?? this.general,
      selectedProjectId: selectedProjectId ?? this.selectedProjectId,
      selectedSessionId: selectedSessionId ?? this.selectedSessionId,
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
