import 'package:freezed_annotation/freezed_annotation.dart';

import 'agent_workspace_view.dart';
import 'interaction_models.dart';
import 'provider_models.dart';
import 'recovery_models.dart';
import 'runtime_models.dart';
import 'session_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'studio_state.dart';

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
    required List<StudioSession> rootSessions,
    required String? selectedProjectId,
    required String? selectedRootSessionId,
    required bool isBusy,
    required List<StudioRecoveryIssue> recoveryIssues,
  }) = _SidebarView;

  factory SidebarView.fromState(StudioState state) {
    return SidebarView(
      projects: state.projects,
      rootSessions: state.rootSessions,
      selectedProjectId: state.selectedProjectId,
      selectedRootSessionId: state.selectedRootSession?.id,
      isBusy: state.isBusy,
      recoveryIssues: state.recoveryIssues,
    );
  }

  StudioRecoveryIssue? recoveryIssueForProject(String projectId) {
    for (final issue in recoveryIssues) {
      if (issue.scope == RecoveryIssueScope.project &&
          issue.projectId == projectId) {
        return issue;
      }
    }
    return null;
  }

  StudioRecoveryIssue? recoveryIssueForSession(String sessionId) {
    for (final issue in recoveryIssues) {
      if (issue.scope == RecoveryIssueScope.session &&
          issue.sessionId == sessionId) {
        return issue;
      }
    }
    return null;
  }
}

@freezed
abstract class HeaderView with _$HeaderView {
  const factory HeaderView({
    required StudioSession? selectedRootSession,
    required StudioProject? selectedProject,
    required String? selectedProjectId,
    required List<StudioSession> agentSessions,
    required String? selectedAgentSessionId,
    required SessionRuntimeView runtime,
    required List<PendingInteraction> pendingInteractions,
  }) = _HeaderView;

  factory HeaderView.fromState(StudioState state) {
    final root = state.selectedRootSession;
    final projectId = root?.projectId ?? state.selectedProjectId;
    StudioProject? selectedProject;
    for (final project in state.projects) {
      if (project.id == projectId) {
        selectedProject = project;
        break;
      }
    }
    return HeaderView(
      selectedRootSession: root,
      selectedProject: selectedProject,
      selectedProjectId: state.selectedProjectId,
      agentSessions: state.agentSessionsForSelectedRoot,
      selectedAgentSessionId: state.selectedAgentSessionId,
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
    required StudioSession session,
    required SessionRuntimeView runtime,
    required PermissionMode permissionMode,
    required List<ProviderSettingsView> providers,
    required List<RoleSettingsView> roles,
    required bool isBusy,
  }) = _StatusBarView;

  factory StatusBarView.fromWorkspace(AgentWorkspaceView workspace) {
    return StatusBarView(
      session: workspace.session,
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
