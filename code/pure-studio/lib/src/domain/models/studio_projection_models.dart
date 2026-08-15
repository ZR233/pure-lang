import 'package:freezed_annotation/freezed_annotation.dart';

import 'agent_models.dart';
import 'agent_workspace_view.dart';
import 'interaction_models.dart';
import 'provider_models.dart';
import 'recovery_models.dart';
import 'runtime_models.dart';
import 'thread_directory_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'studio_state.dart';
import 'studio_state_snapshots.dart';

part 'studio_projection_models.freezed.dart';

@freezed
abstract class ShellChromeView with _$ShellChromeView {
  const factory ShellChromeView({
    required List<StudioRecoveryIssue> applicationRecoveryIssues,
  }) = _ShellChromeView;

  factory ShellChromeView.fromState(StudioState state) {
    return ShellChromeView(
      applicationRecoveryIssues: state.applicationRecoveryIssues,
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
      pendingInteractions: state.pendingInteractions,
    );
  }
}

@freezed
abstract class SettingsPageView with _$SettingsPageView {
  const factory SettingsPageView({
    required List<ProviderSettingsView> providers,
    required ProviderCatalogView providerCatalog,
    required String? defaultProviderId,
    required List<RoleSettingsView> roles,
    required InstructionsSettingsView instructions,
    required SkillsSettingsView skills,
    required List<String> activeSkills,
    required String? selectedProjectId,
    required List<McpServerSettingsView> mcpServers,
    required McpStateSnapshot mcpState,
    required LspStateSnapshot lspState,
    required PermissionMode permissionMode,
    required GeneralSettingsView general,
    required WebSearchSettingsView webSearch,
    required bool runtimeBusy,
  }) = _SettingsPageView;

  factory SettingsPageView.fromState(StudioState state) {
    return SettingsPageView(
      providers: state.providers,
      providerCatalog: state.providerCatalog,
      defaultProviderId: state.defaultProviderId,
      roles: state.roles,
      instructions: state.instructions,
      skills: state.skills,
      activeSkills: state.runtime.activeSkills,
      selectedProjectId: state.selectedProjectId,
      mcpServers: state.mcpServers,
      mcpState: state.mcpState,
      lspState: state.lspState,
      permissionMode: state.permissionMode,
      general: state.general,
      webSearch: state.webSearch,
      runtimeBusy: state.isBusy || state.runtime.hasActiveTask,
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

  RoleSettingsView? role(String key) {
    for (final role in roles) {
      if (role.key == key) {
        return role;
      }
    }
    return null;
  }
}
