import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../domain/models/studio_models.dart';
import 'studio_controller.dart';

part 'studio_selectors.g.dart';

typedef WorkspaceLayoutView = ({
  String sessionId,
  bool isLoading,
  TimelineTodoListUpdate? todo,
});

typedef TimelinePaneView = ({
  List<TimelineRow> rows,
  TurnPhase turnPhase,
  bool isLoading,
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
          sessionId: workspace.sessionId,
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
AsyncValue<TimelinePaneView?> agentTimeline(Ref ref, String sessionId) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData((state) {
        if (state.selectedAgentSessionId != sessionId) {
          return null;
        }
        final workspace = state.selectedAgentWorkspace;
        if (workspace == null) {
          return null;
        }
        return (
          rows: workspace.timelineRows,
          turnPhase: workspace.turnPhase,
          isLoading: workspace.isLoading,
        );
      }),
    ),
  );
}

@riverpod
AsyncValue<AgentWorkspaceView?> agentWorkspace(Ref ref, String sessionId) {
  return ref.watch(
    studioControllerProvider.select(
      (state) => state.whenData((state) {
        if (state.selectedAgentSessionId != sessionId) {
          return null;
        }
        return state.selectedAgentWorkspace;
      }),
    ),
  );
}
