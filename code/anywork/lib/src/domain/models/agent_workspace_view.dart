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

/// 工作区同步状态。
///
/// `idle` 表示“已选中但尚未打开”：会话状态、history 数据库与历史条目都还没有加载，
/// 需要一次显式用户交互才会打开（见 controller 的 openThread/openSelectedThread）。
/// 打开只读取一次当前状态并建立订阅，不重发模型请求、不重跑工具、不续跑未完成工作流。
enum AgentWorkspaceSyncState {
  idle,
  loading,
  ready,
  reconnecting,
  stale,
  failed,
}

enum AgentComposerMode { editable, runtimeDriven }

@freezed
abstract class AgentWorkspaceView with _$AgentWorkspaceView {
  const AgentWorkspaceView._();

  const factory AgentWorkspaceView({
    required StudioThread thread,
    required StudioThread rootThread,
    required AgentWorkspaceSyncState syncState,
    String? loadError,
    required List<TimelineRow> timelineRows,
    required TimelineTodoListUpdate? todo,
    required ThreadRuntimeView runtime,
    required StudioTurnView? turn,
    StudioTurnView? lastTurn,
    required PendingInteraction? activeInteraction,
    required ComposerThreadState composer,
    required AgentComposerMode composerMode,
    required PermissionMode permissionMode,
    required List<ProviderSettingsView> providers,
    @Default(<ModeModelRouteView>[]) List<ModeModelRouteView> modeModelRoutes,
    required List<RoleSettingsView> roles,
    required List<StudioAgentView> agents,
  }) = _AgentWorkspaceView;

  String get threadId => thread.id;

  bool get isRoot => thread.isRoot;

  bool get isLoading =>
      syncState == AgentWorkspaceSyncState.loading ||
      syncState == AgentWorkspaceSyncState.reconnecting;

  bool get isBusy => turn?.state.isBusy ?? false;
}
