import 'agent_models.dart';
import 'interaction_models.dart';
import 'provider_models.dart';
import 'runtime_models.dart';
import 'session_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'timeline_models.dart';

enum AgentWorkspaceSyncState { loading, ready, reconnecting, stale }

enum AgentComposerMode { editable, runtimeDriven }

class AgentWorkspaceView {
  const AgentWorkspaceView({
    required this.session,
    required this.rootSession,
    required this.syncState,
    required this.timelineRows,
    required this.todo,
    required this.runtime,
    required this.turnPhase,
    required this.activeInteraction,
    required this.composerText,
    required this.composerMode,
    required this.permissionMode,
    required this.providers,
    required this.roles,
    required this.agents,
  });

  final StudioSession session;
  final StudioSession rootSession;
  final AgentWorkspaceSyncState syncState;
  final List<TimelineRow> timelineRows;
  final TimelineTodoListUpdate? todo;
  final SessionRuntimeView runtime;
  final TurnPhase turnPhase;
  final PendingInteraction? activeInteraction;
  final String composerText;
  final AgentComposerMode composerMode;
  final PermissionMode permissionMode;
  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;
  final List<StudioAgentView> agents;

  String get sessionId => session.id;

  bool get isRoot => session.isRoot;

  bool get isLoading => syncState == AgentWorkspaceSyncState.loading;

  TurnPhase get statusPhase {
    if (isRoot &&
        session.agentStatus.trim() == 'waiting' &&
        (turnPhase == TurnPhase.idle || turnPhase == TurnPhase.completed)) {
      return TurnPhase.waitingForAgents;
    }
    return turnPhase;
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
      TurnPhase.waitingForAgents ||
      TurnPhase.completed ||
      TurnPhase.failed ||
      TurnPhase.cancelled => false,
    };
  }

  RoleSettingsView? role(String key) {
    for (final role in roles) {
      if (role.key == key) {
        return role;
      }
    }
    return null;
  }
}
