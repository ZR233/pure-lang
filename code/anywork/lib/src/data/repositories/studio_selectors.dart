import 'package:flutter/foundation.dart' show listEquals;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../domain/models/studio_models.dart';
import 'studio_controller.dart';

part 'studio_selectors.g.dart';

typedef WorkspaceLayoutView = ({
  String threadId,
  bool isLoading,

  /// 已选中但尚未打开：首屏恢复的选择不打开会话，由用户显式交互触发。
  bool needsOpen,
  String? loadError,
  TimelineTodoListUpdate? todo,
  PlanConfirmationView? planConfirmation,
});

class TimelinePaneView {
  const TimelinePaneView({
    required this.rows,
    required this.turn,
    required this.isLoading,
    required this.hasOlderHistory,
    required this.isLoadingOlderHistory,
    required this.history,
  });

  final List<TimelineRow> rows;
  final StudioTurnView? turn;
  final bool isLoading;
  final bool hasOlderHistory;
  final bool isLoadingOlderHistory;
  final ThreadHistoryWindow history;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is TimelinePaneView &&
            listEquals(rows, other.rows) &&
            turn == other.turn &&
            isLoading == other.isLoading &&
            hasOlderHistory == other.hasOlderHistory &&
            isLoadingOlderHistory == other.isLoadingOlderHistory &&
            history == other.history;
  }

  @override
  int get hashCode => Object.hash(
    Object.hashAll(rows),
    turn,
    isLoading,
    hasOlderHistory,
    isLoadingOlderHistory,
    history,
  );
}

typedef StartPageView = StudioStartPageProjection;

/// Fine-grained projections all read the same AsyncNotifier state. None of
/// these providers opens a second event stream or keeps a durable copy.
final runtimeProjectionProvider = Provider<AsyncValue<StudioRuntimeProjection>>(
  (ref) {
    return ref.watch(
      studioControllerProvider.select(
        (state) => state.whenData(StudioRuntimeProjection.fromState),
      ),
    );
  },
);

final contextUsageProjectionProvider =
    Provider<AsyncValue<StudioContextProjection>>((ref) {
      return ref.watch(
        studioControllerProvider.select(
          (state) => state.whenData(StudioContextProjection.fromState),
        ),
      );
    });

final throughputProjectionProvider =
    Provider<AsyncValue<StudioThroughputProjection>>((ref) {
      return ref.watch(
        studioControllerProvider.select(
          (state) => state.whenData(StudioThroughputProjection.fromState),
        ),
      );
    });

final statusProjectionProvider = Provider<AsyncValue<StudioStatusProjection>>((
  ref,
) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData(StudioStatusProjection.fromState),
    ),
  );
});

/// Immutable start-session projection.
///
/// It is derived from the latest controller state at selector evaluation time;
/// widgets never resolve a missing model by mutating the mode or by capturing
/// an old `view` in a callback.  Pending route mutations are represented as a
/// separate projection so an optimistic visual state cannot become canonical
/// by accident.
final class StartSessionViewModel {
  const StartSessionViewModel({
    required this.isStartPage,
    required this.project,
    required this.composer,
    required this.permissionMode,
    required this.canSubmit,
    required this.mode,
    required this.workspaceMode,
    required this.providers,
    required this.modeModelRoutes,
    required this.modeRequiresFunctionCalling,
    required this.modelRouteMutationPending,
    required this.targetDiagnostic,
  });

  factory StartSessionViewModel.fromState(StudioState state) {
    final projectId = state.selectedProjectId;
    final project = state.projects
        .where((candidate) => candidate.id == projectId)
        .firstOrNull;
    final healthy =
        project != null &&
        state.recoveryIssue(
              blockingOnly: true,
              scope: RecoveryIssueScope.project,
              projectId: project.id,
            ) ==
            null;
    final mode = state.newThreadMode;
    return StartSessionViewModel(
      isStartPage: state.selectedThreadId == null,
      project: project,
      composer: state.newThreadComposer,
      permissionMode: state.permissionMode,
      canSubmit: healthy,
      mode: mode,
      workspaceMode: state.newThreadWorkspaceMode,
      providers: List.unmodifiable(state.providers),
      modeModelRoutes: List.unmodifiable(state.modeModelRoutes),
      modeRequiresFunctionCalling:
          state.threadModeCatalog.modes
              .where((descriptor) => descriptor.id == mode.id)
              .firstOrNull
              ?.hasWorkflow ??
          false,
      modelRouteMutationPending:
          state.mutationPending('new-thread-route:model:${mode.id}') ||
          state.mutationPending('new-thread-route:effort:${mode.id}'),
      targetDiagnostic: state.composerDiagnostic(projectId, null),
    );
  }

  final bool isStartPage;
  final StudioProject? project;
  final ComposerThreadState composer;
  final PermissionMode permissionMode;
  final bool canSubmit;
  final ThreadModeId mode;
  final ThreadWorkspaceMode workspaceMode;
  final List<ProviderSettingsView> providers;
  final List<ModeModelRouteView> modeModelRoutes;
  final bool modeRequiresFunctionCalling;
  final bool modelRouteMutationPending;
  final ComposerTargetDiagnostic? targetDiagnostic;
}

final settingsProjectionProvider =
    Provider<AsyncValue<StudioSettingsProjection>>((ref) {
      return ref.watch(
        studioControllerProvider.select(
          (state) => state.whenData(StudioSettingsProjection.fromState),
        ),
      );
    });

final providerUsageProjectionProvider =
    Provider<AsyncValue<StudioProviderUsageProjection>>((ref) {
      return ref.watch(
        studioControllerProvider.select(
          (state) => state.whenData(StudioProviderUsageProjection.fromState),
        ),
      );
    });

final composerDiagnosticsProjectionProvider =
    Provider<AsyncValue<StudioComposerDiagnosticsProjection>>((ref) {
      return ref.watch(
        studioControllerProvider.select(
          (state) =>
              state.whenData(StudioComposerDiagnosticsProjection.fromState),
        ),
      );
    });

final composerProjectionProvider =
    Provider<AsyncValue<StudioComposerProjection?>>((ref) {
      return ref.watch(
        studioControllerProvider.select(
          (state) => state.whenData((value) {
            return StudioComposerProjection.fromState(value);
          }),
        ),
      );
    });

final interactionProjectionProvider =
    Provider<AsyncValue<StudioInteractionProjection>>((ref) {
      return ref.watch(
        studioControllerProvider.select(
          (state) => state.whenData(StudioInteractionProjection.fromState),
        ),
      );
    });

final storageProjectionProvider = Provider<AsyncValue<StudioStorageProjection>>(
  (ref) {
    return ref.watch(
      studioControllerProvider.select(
        (state) => state.whenData(StudioStorageProjection.fromState),
      ),
    );
  },
);

final activityProjectionProvider =
    Provider.family<AsyncValue<StudioActivityProjection?>, String>((
      ref,
      threadId,
    ) {
      return ref.watch(
        studioControllerProvider.select(
          (asyncState) => asyncState.whenData((studioState) {
            final view = _conversationActivityForState(studioState, threadId);
            return view == null
                ? null
                : StudioActivityProjection(threadId: threadId, view: view);
          }),
        ),
      );
    });

final timelineProjectionProvider =
    Provider.family<AsyncValue<TimelinePaneView?>, String>((ref, threadId) {
      return ref.watch(
        studioControllerProvider.select(
          (asyncState) => asyncState.whenData(
            (studioState) => _timelinePaneForState(studioState, threadId),
          ),
        ),
      );
    });

ConversationActivityView? _conversationActivityForState(
  StudioState studioState,
  String threadId,
) {
  if (studioState.selectedThreadId != threadId) return null;
  final workspace = studioState.workspacesByThread[threadId];
  if (workspace == null) return null;
  final ui =
      studioState.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  final interaction = studioState.activeInteraction;
  final scoped = interaction != null && interaction.threadId == threadId
      ? interaction
      : null;
  final activity = workspace.activity;
  final detailState = ui.activityDetail;
  final matches = activity != null && detailState.matches(activity.identity);
  return projectConversationActivity(
    activity: activity,
    storage: workspace.storage,
    interaction: scoped,
    detail: matches ? detailState.detail : null,
    detailsLoading: matches && detailState.loading,
    detailsError: matches ? detailState.error : null,
  );
}

TimelinePaneView? _timelinePaneForState(
  StudioState studioState,
  String threadId,
) {
  if (studioState.selectedThreadId != threadId) return null;
  final workspace = studioState.workspacesByThread[threadId];
  if (workspace == null) return null;
  final history = studioState.workspaceUiByThread[threadId]?.history;
  return TimelinePaneView(
    rows: studioState.selectedTimelineRows,
    turn: workspace.activeTurn,
    isLoading:
        studioState.selectedWorkspaceUi.syncState ==
            AgentWorkspaceSyncState.loading ||
        studioState.selectedWorkspaceUi.syncState ==
            AgentWorkspaceSyncState.reconnecting,
    hasOlderHistory: history?.hasOlder ?? false,
    isLoadingOlderHistory:
        history?.isLoading == true &&
        history?.direction == TimelineDirection.older,
    history: history ?? const ThreadHistoryWindow(),
  );
}

@riverpod
AsyncValue<AgentWorkspaceView?> selectedAgentWorkspace(Ref ref) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData((state) => state.selectedAgentWorkspace),
    ),
  );
}

@riverpod
AsyncValue<ShellChromeView> shellChrome(Ref ref) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData(ShellChromeView.fromState),
    ),
  );
}

@riverpod
AsyncValue<SidebarView> sidebar(Ref ref) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData(SidebarView.fromState),
    ),
  );
}

@riverpod
AsyncValue<HeaderView> studioHeader(Ref ref) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData(HeaderView.fromState),
    ),
  );
}

@riverpod
AsyncValue<SettingsPageView> settingsPage(Ref ref) {
  return ref.watch(
    settingsProjectionProvider.select(
      (state) => state.whenData((projection) => projection.view),
    ),
  );
}

@riverpod
AsyncValue<WorkspaceLayoutView?> selectedWorkspaceLayout(Ref ref) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData((state) {
        final workspace = state.selectedAgentWorkspace;
        if (workspace == null) {
          return null;
        }
        return (
          threadId: workspace.threadId,
          isLoading: workspace.isLoading,
          // 首帧之后自动打开所选会话；打开完成前仍展示未打开状态。
          needsOpen: !state.openedThreadIds.contains(workspace.threadId),
          loadError: workspace.loadError,
          todo: workspace.todo,
          planConfirmation: workspace.activeInteraction?.planConfirmation,
        );
      }),
    ),
  );
}

@riverpod
AsyncValue<AgentWorkspaceView?> selectedWorkspaceControls(Ref ref) {
  return ref.watch(
    composerProjectionProvider.select(
      (state) => state.whenData((projection) => projection?.legacyWorkspace),
    ),
  );
}

@riverpod
AsyncValue<StartSessionViewModel> startPage(Ref ref) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData(StartSessionViewModel.fromState),
    ),
  );
}

@riverpod
AsyncValue<StatusBarView?> statusBar(Ref ref) {
  final status = ref.watch(statusProjectionProvider);
  final runtime = ref.watch(runtimeProjectionProvider);
  return status.when(
    loading: () => const AsyncLoading(),
    error: (error, stackTrace) => AsyncError(error, stackTrace),
    data: (statusProjection) => runtime.when(
      loading: () => const AsyncLoading(),
      error: (error, stackTrace) => AsyncError(error, stackTrace),
      data: (runtimeProjection) {
        final thread = statusProjection.thread;
        if (thread == null || runtimeProjection.threadId != thread.id) {
          return const AsyncData(null);
        }
        return AsyncData(
          StatusBarView(
            thread: thread,
            runtime: runtimeProjection.runtime,
            permissionMode: statusProjection.permissionMode,
            providers: statusProjection.providers,
            roles: statusProjection.roles,
            isBusy: statusProjection.isBusy,
          ),
        );
      },
    ),
  );
}

@riverpod
AsyncValue<TimelinePaneView?> agentTimeline(Ref ref, String threadId) {
  return ref.watch(timelineProjectionProvider(threadId));
}

/// 固定活动条的唯一输入。
///
/// 完全由后端 typed 活动投影、typed 存储状态与待处理交互派生；展开详情是 controller
/// 按活动身份**按需**读取的结果（不依赖消息窗口、不查 SQL 历史）。没有任何窗口可见性
/// 推断或默认阶段。
///
/// 待接线：后端状态流暂无 storage 通知，存储事实目前只在快照刷新时更新（见
/// activity-contract §6/§8）。这里不解析错误字符串、也不在本地猜测“已恢复”。
@riverpod
AsyncValue<ConversationActivityView?> conversationActivity(
  Ref ref,
  String threadId,
) {
  return ref.watch(
    activityProjectionProvider(threadId)
        .select((state) => state.whenData((projection) => projection?.view)),
  );
}
