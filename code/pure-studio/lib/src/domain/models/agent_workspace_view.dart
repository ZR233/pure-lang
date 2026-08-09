import 'package:freezed_annotation/freezed_annotation.dart';

import 'agent_models.dart';
import 'composer_models.dart';
import 'interaction_models.dart';
import 'provider_models.dart';
import 'runtime_models.dart';
import 'thread_directory_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'timeline_models.dart';
import 'turn_models.dart';

part 'agent_workspace_view.freezed.dart';

enum AgentWorkspaceSyncState { loading, ready, reconnecting, stale }

enum AgentComposerMode { editable, runtimeDriven }

@freezed
abstract class AgentWorkspaceView with _$AgentWorkspaceView {
  const AgentWorkspaceView._();

  const factory AgentWorkspaceView({
    required StudioThread thread,
    required StudioThread rootThread,
    required AgentWorkspaceSyncState syncState,
    required List<TimelineRow> timelineRows,
    required TimelineTodoListUpdate? todo,
    required ThreadRuntimeView runtime,
    required StudioTurnView? turn,
    required PendingInteraction? activeInteraction,
    required ComposerThreadState composer,
    required AgentComposerMode composerMode,
    required PermissionMode permissionMode,
    required List<ProviderSettingsView> providers,
    required List<RoleSettingsView> roles,
    required List<StudioAgentView> agents,
  }) = _AgentWorkspaceView;

  String get threadId => thread.id;

  bool get isRoot => thread.isRoot;

  bool get isLoading => syncState == AgentWorkspaceSyncState.loading;

  bool get isBusy => turn?.state.isBusy ?? false;

  bool get isTaskPaused =>
      isRoot &&
      runtime.hasActiveTask &&
      !isBusy &&
      (rootThread.status == 'interrupted' ||
          runtime.task!.hasRecoverableExecutorFailure);

  RoleSettingsView? role(String key) {
    for (final role in roles) {
      if (role.key == key) {
        return role;
      }
    }
    return null;
  }
}
