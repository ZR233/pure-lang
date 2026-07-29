import 'package:freezed_annotation/freezed_annotation.dart';

import 'agent_models.dart';
import 'interaction_models.dart';
import 'provider_models.dart';
import 'runtime_models.dart';
import 'session_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'timeline_models.dart';

part 'agent_workspace_view.freezed.dart';

enum AgentWorkspaceSyncState { loading, ready, reconnecting, stale }

enum AgentComposerMode { editable, runtimeDriven }

@freezed
abstract class AgentWorkspaceView with _$AgentWorkspaceView {
  const AgentWorkspaceView._();

  const factory AgentWorkspaceView({
    required StudioSession session,
    required StudioSession rootSession,
    required AgentWorkspaceSyncState syncState,
    required List<TimelineRow> timelineRows,
    required TimelineTodoListUpdate? todo,
    required SessionRuntimeView runtime,
    required TurnPhase turnPhase,
    required PendingInteraction? activeInteraction,
    required String composerText,
    required AgentComposerMode composerMode,
    required PermissionMode permissionMode,
    required List<ProviderSettingsView> providers,
    required List<RoleSettingsView> roles,
    required List<StudioAgentView> agents,
  }) = _AgentWorkspaceView;

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
