import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../domain/models/studio_models.dart';
import 'studio_controller.dart';

part 'studio_selectors.g.dart';

typedef WorkspaceLayoutView = ({
  String threadId,
  bool isLoading,
  TimelineTodoListUpdate? todo,
});

typedef TimelinePaneView = ({
  List<TimelineRow> rows,
  StudioTurnView? turn,
  bool isLoading,
  bool hasOlderHistory,
  bool isLoadingOlderHistory,
});

typedef StartPageView = ({
  bool isStartPage,
  StudioProject? project,
  ComposerThreadState composer,
  PermissionMode permissionMode,
  bool canSubmit,
  StudioMode mode,
  List<ProviderSettingsView> providers,
  List<RoleSettingsView> roles,
});

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
    studioControllerProvider.select(
      (state) => state.whenData(SettingsPageView.fromState),
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
          todo: workspace.todo,
        );
      }),
    ),
  );
}

@riverpod
AsyncValue<AgentWorkspaceView?> selectedWorkspaceControls(Ref ref) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData(
        (state) => state.selectedAgentWorkspace?.copyWith(
          timelineRows: const [],
          todo: null,
        ),
      ),
    ),
  );
}

@riverpod
AsyncValue<StartPageView> startPage(Ref ref) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData((state) {
        final projectId = state.selectedProjectId;
        final project = state.projects
            .where((project) => project.id == projectId)
            .firstOrNull;
        final healthy =
            project != null &&
            state.recoveryIssue(
                  scope: RecoveryIssueScope.project,
                  projectId: project.id,
                ) ==
                null;
        return (
          isStartPage: state.selectedThreadId == null,
          project: project,
          composer: state.newThreadComposer,
          permissionMode: state.permissionMode,
          canSubmit: healthy,
          mode: state.newThreadMode,
          providers: state.providers,
          roles: state.roles,
        );
      }),
    ),
  );
}

@riverpod
AsyncValue<StatusBarView?> statusBar(Ref ref) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData((state) {
        final workspace = state.selectedAgentWorkspace;
        return workspace == null
            ? null
            : StatusBarView.fromWorkspace(workspace);
      }),
    ),
  );
}

@riverpod
AsyncValue<TimelinePaneView?> agentTimeline(Ref ref, String threadId) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData((state) {
        if (state.selectedThreadId != threadId) {
          return null;
        }
        final workspace = state.selectedAgentWorkspace;
        if (workspace == null) {
          return null;
        }
        final history = state.workspaceUiByThread[threadId]?.history;
        return (
          rows: workspace.timelineRows,
          turn: workspace.turn,
          isLoading: workspace.isLoading,
          hasOlderHistory: history?.hasOlder ?? false,
          isLoadingOlderHistory: history?.isLoading ?? false,
        );
      }),
    ),
  );
}

@riverpod
AsyncValue<AgentWorkspaceView?> agentWorkspace(Ref ref, String threadId) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData((state) {
        if (state.selectedThreadId != threadId) {
          return null;
        }
        return state.selectedAgentWorkspace;
      }),
    ),
  );
}
