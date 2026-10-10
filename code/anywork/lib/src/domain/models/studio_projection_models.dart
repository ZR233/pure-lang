import 'package:freezed_annotation/freezed_annotation.dart';

import 'agent_models.dart';
import 'agent_workspace_view.dart';
import 'interaction_models.dart';
import 'persistence_models.dart';
import 'provider_models.dart';
import 'recovery_models.dart';
import 'runtime_models.dart';
import 'thread_directory_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'studio_state.dart';

part 'studio_projection_models.freezed.dart';

@freezed
abstract class ShellChromeView with _$ShellChromeView {
  const factory ShellChromeView({
    required List<StudioRecoveryIssue> applicationRecoveryIssues,
    required PersistenceStateSnapshot persistenceState,
  }) = _ShellChromeView;

  factory ShellChromeView.fromState(StudioState state) {
    return ShellChromeView(
      applicationRecoveryIssues: state.applicationRecoveryIssues,
      persistenceState: state.persistenceState,
    );
  }
}

@freezed
abstract class SidebarView with _$SidebarView {
  const SidebarView._();

  const factory SidebarView({
    required List<StudioProject> projects,
    required List<StudioThread> rootThreads,
    required String? selectedProjectId,
    required String? selectedRootThreadId,
    required bool isBusy,
    required Map<String, StudioRecoveryIssue> projectRecoveryIssues,
    required Map<String, StudioRecoveryIssue> threadRecoveryIssues,
    required Map<String, String> modeDisplayNames,
    @Default(false) bool directoryHasMore,
    @Default(false) bool directoryIsLoading,
  }) = _SidebarView;

  factory SidebarView.fromState(StudioState state) {
    final projectRecoveryIssues = <String, StudioRecoveryIssue>{};
    for (final project in state.projects) {
      final issue = state.recoveryIssue(
        scope: RecoveryIssueScope.project,
        projectId: project.id,
      );
      if (issue != null) projectRecoveryIssues[project.id] = issue;
    }
    final threadRecoveryIssues = <String, StudioRecoveryIssue>{};
    for (final thread in state.rootThreads) {
      final issue = state.recoveryIssue(
        scope: RecoveryIssueScope.thread,
        threadId: thread.id,
      );
      if (issue != null) threadRecoveryIssues[thread.id] = issue;
    }
    return SidebarView(
      projects: state.projects,
      rootThreads: state.rootThreads,
      selectedProjectId: state.selectedProjectId,
      selectedRootThreadId: state.selectedRootThread?.id,
      isBusy: state.isBusy,
      projectRecoveryIssues: projectRecoveryIssues,
      threadRecoveryIssues: threadRecoveryIssues,
      modeDisplayNames: {
        for (final mode in state.threadModeCatalog.modes)
          mode.id: mode.displayName,
      },
      directoryHasMore: state.threadDirectory.hasMore,
      directoryIsLoading: state.threadDirectory.isLoading,
    );
  }
}

@freezed
abstract class HeaderView with _$HeaderView {
  const factory HeaderView({
    required StudioThread? selectedRootThread,
    required StudioProject? selectedProject,
    required String? selectedProjectId,
    required List<StudioThread> workspaceThreads,
    required List<StudioAgentView> agents,
    required String? selectedThreadId,
    required ThreadRuntimeView runtime,
    required SessionCostView? sessionCost,
    required List<PendingInteraction> pendingInteractions,
  }) = _HeaderView;

  factory HeaderView.fromState(StudioState state) {
    final root = state.selectedRootThread;
    final projectId = root?.projectId ?? state.selectedProjectId;
    StudioProject? selectedProject;
    for (final project in state.projects) {
      if (project.id == projectId) {
        selectedProject = project;
        break;
      }
    }
    return HeaderView(
      selectedRootThread: root,
      selectedProject: selectedProject,
      selectedProjectId: state.selectedProjectId,
      workspaceThreads: state.threadsForSelectedRoot,
      agents: state.selectedAgents,
      selectedThreadId: state.selectedThreadId,
      runtime: state.runtime,
      sessionCost: state.selectedSessionCost,
      pendingInteractions: state.pendingInteractions,
    );
  }
}

@freezed
abstract class StatusBarView with _$StatusBarView {
  const StatusBarView._();

  const factory StatusBarView({
    required StudioThread thread,
    required ThreadRuntimeView runtime,
    required PermissionMode permissionMode,
    required List<ProviderSettingsView> providers,
    required List<RoleSettingsView> roles,
    required bool isBusy,
  }) = _StatusBarView;

  factory StatusBarView.fromWorkspace(AgentWorkspaceView workspace) {
    return StatusBarView(
      thread: workspace.thread,
      runtime: workspace.runtime,
      permissionMode: workspace.permissionMode,
      providers: workspace.providers,
      roles: workspace.roles,
      isBusy: workspace.isBusy,
    );
  }
}
