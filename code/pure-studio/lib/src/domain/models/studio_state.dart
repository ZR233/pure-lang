import 'agent_models.dart';
import 'agent_workspace_view.dart';
import 'collection_extensions.dart';
import 'composer_models.dart';
import 'interaction_models.dart';
import 'provider_models.dart';
import 'recovery_models.dart';
import 'runtime_models.dart';
import 'thread_directory_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'thread_models.dart';
import 'timeline_models.dart';
import 'turn_models.dart';

class _StudioStateUnset {
  const _StudioStateUnset();
}

const _studioStateUnset = _StudioStateUnset();
const _emptySessionRuntime = ThreadRuntimeView(
  model: '',
  contextTokens: 0,
  contextWindow: 0,
  totalTokens: 0,
  costLabel: '',
  activeSkills: [],
  activeMcpServers: [],
  activeLspServers: [],
  agentCount: 0,
);

class StudioState {
  StudioState({
    required this.projects,
    required this.threads,
    this.workspacesByThread = const {},
    this.workspaceUiByThread = const {},
    this.tasksByRootThread = const {},
    this.agentsByThread = const {},
    required this.providers,
    this.providerCatalog = const ProviderCatalogView.empty(),
    this.defaultProviderId,
    this.providerUsages = const [],
    required this.roles,
    required this.mcpServers,
    this.instructions = const InstructionsSettingsView(),
    this.skills = const SkillsSettingsView(),
    this.general = const GeneralSettingsView(),
    this.webSearch = const WebSearchSettingsView(),
    required this.selectedProjectId,
    required this.selectedThreadId,
    required this.permissionMode,
    this.recoveryIssues = const [],
  });

  final List<StudioProject> projects;
  final List<StudioThread> threads;
  final Map<String, ThreadWorkspace> workspacesByThread;
  final Map<String, WorkspaceUiState> workspaceUiByThread;
  final Map<String, TaskRuntimeView> tasksByRootThread;
  final Map<String, StudioAgentView> agentsByThread;
  final List<ProviderSettingsView> providers;
  final ProviderCatalogView providerCatalog;
  final String? defaultProviderId;
  final List<ProviderUsageView> providerUsages;
  final List<RoleSettingsView> roles;
  final List<McpServerSettingsView> mcpServers;
  final InstructionsSettingsView instructions;
  final SkillsSettingsView skills;
  final GeneralSettingsView general;
  final WebSearchSettingsView webSearch;
  final String? selectedProjectId;
  final String? selectedThreadId;
  final PermissionMode permissionMode;
  final List<StudioRecoveryIssue> recoveryIssues;

  ThreadWorkspace? get selectedWorkspace {
    final id = selectedThreadId;
    return id == null ? null : workspacesByThread[id];
  }

  WorkspaceUiState get selectedWorkspaceUi {
    final id = selectedThreadId;
    return id == null
        ? const WorkspaceUiState()
        : workspaceUiByThread[id] ?? const WorkspaceUiState();
  }

  StudioTurnView? get turn => selectedWorkspace?.activeTurn;

  ThreadRuntimeView get runtime {
    final selected = selectedThread;
    final canonical = selectedWorkspace?.runtime ?? _emptySessionRuntime;
    final task = selected == null
        ? null
        : tasksByRootThread[selected.effectiveRootThreadId];
    return canonical.copyWith(
      task: task,
      agentCount: threadsForSelectedRoot.length,
    );
  }

  ComposerThreadState get composer => selectedWorkspaceUi.composer;

  List<StudioThread> get rootThreads =>
      threads.where((thread) => thread.isRoot).toList();

  StudioThread? get selectedThread {
    final id = selectedThreadId;
    return id == null
        ? null
        : threads.where((thread) => thread.id == id).firstOrNull;
  }

  StudioThread? get selectedRootThread {
    final selected = selectedThread;
    if (selected == null) return null;
    final rootId = selected.effectiveRootThreadId;
    return threads
        .where((thread) => thread.id == rootId && thread.isRoot)
        .firstOrNull;
  }

  List<StudioThread> get threadsForSelectedRoot {
    final root = selectedRootThread;
    if (root == null) return const [];
    final scoped = threads
        .where((thread) => thread.effectiveRootThreadId == root.id)
        .toList();
    final children = <String?, List<StudioThread>>{};
    for (final thread in scoped) {
      children.putIfAbsent(thread.parentThreadId, () => []).add(thread);
    }
    for (final siblings in children.values) {
      siblings.sort((left, right) {
        final created = left.effectiveCreatedAt.compareTo(
          right.effectiveCreatedAt,
        );
        return created != 0 ? created : left.id.compareTo(right.id);
      });
    }
    final ordered = <StudioThread>[];
    final visited = <String>{};
    void append(StudioThread thread) {
      if (!visited.add(thread.id)) return;
      ordered.add(thread);
      for (final child in children[thread.id] ?? const <StudioThread>[]) {
        append(child);
      }
    }

    append(root);
    for (final thread in scoped) {
      append(thread);
    }
    return ordered;
  }

  List<TimelineRow> get selectedTimelineRows =>
      timelineRowsFromThreadItems(selectedWorkspace?.items ?? const []);

  TimelineTodoListUpdate? get selectedTodoList => selectedWorkspace?.todo;

  List<PendingInteraction> get pendingInteractions => [
    for (final workspace in workspacesByThread.values)
      ...workspace.interactions,
  ];

  PendingInteraction? get activeInteraction {
    final interactions = [...(selectedWorkspace?.interactions ?? const [])]
      ..sort(
        (left, right) => interactionPriority(
          left.kind,
        ).compareTo(interactionPriority(right.kind)),
      );
    return interactions.firstOrNull;
  }

  List<StudioAgentView> get selectedAgents {
    return threadsForSelectedRoot
        .map(
          (thread) =>
              agentsByThread[thread.id] ??
              StudioAgentView(
                id: thread.id,
                threadId: thread.id,
                path: thread.agentPath.isEmpty ? thread.id : thread.agentPath,
                parentPath: thread.parentThreadId,
                role: thread.role,
                task: thread.title,
                status: thread.status,
                activity: StudioAgentActivity.idle,
                summary: null,
                depth: thread.isRoot ? 0 : 1,
                error: null,
                reason: null,
                updatedAt: thread.updatedAt,
              ),
        )
        .toList();
  }

  AgentWorkspaceView? get selectedAgentWorkspace {
    final thread = selectedThread;
    final root = selectedRootThread;
    if (thread == null || root == null) return null;
    return AgentWorkspaceView(
      thread: thread,
      rootThread: root,
      syncState: selectedWorkspaceUi.syncState,
      timelineRows: selectedTimelineRows,
      todo: selectedTodoList,
      runtime: runtime,
      turn: turn,
      activeInteraction: activeInteraction,
      composer: composer,
      composerMode: thread.isAgent
          ? AgentComposerMode.runtimeDriven
          : AgentComposerMode.editable,
      permissionMode: permissionMode,
      providers: providers,
      roles: roles,
      agents: selectedAgents,
    );
  }

  List<StudioRecoveryIssue> get applicationRecoveryIssues => recoveryIssues
      .where((issue) => issue.scope == RecoveryIssueScope.application)
      .toList();

  StudioRecoveryIssue? recoveryIssue({
    RecoveryIssueScope? scope,
    String? projectId,
    String? threadId,
    String? id,
  }) {
    return recoveryIssues
        .where(
          (issue) =>
              (scope == null || issue.scope == scope) &&
              (projectId == null || issue.projectId == projectId) &&
              (threadId == null || issue.threadId == threadId) &&
              (id == null || issue.id == id),
        )
        .firstOrNull;
  }

  RoleSettingsView? role(String key) =>
      roles.where((role) => role.key == key).firstOrNull;

  bool get isBusy => turn?.state.isBusy ?? false;

  StudioState copyWith({
    List<StudioProject>? projects,
    List<StudioThread>? threads,
    Map<String, ThreadWorkspace>? workspacesByThread,
    Map<String, WorkspaceUiState>? workspaceUiByThread,
    Map<String, TaskRuntimeView>? tasksByRootThread,
    Map<String, StudioAgentView>? agentsByThread,
    List<ProviderSettingsView>? providers,
    ProviderCatalogView? providerCatalog,
    Object? defaultProviderId = _studioStateUnset,
    List<ProviderUsageView>? providerUsages,
    List<RoleSettingsView>? roles,
    List<McpServerSettingsView>? mcpServers,
    InstructionsSettingsView? instructions,
    SkillsSettingsView? skills,
    GeneralSettingsView? general,
    WebSearchSettingsView? webSearch,
    Object? selectedProjectId = _studioStateUnset,
    Object? selectedThreadId = _studioStateUnset,
    PermissionMode? permissionMode,
    List<StudioRecoveryIssue>? recoveryIssues,
  }) {
    return StudioState(
      projects: projects ?? this.projects,
      threads: threads ?? this.threads,
      workspacesByThread: workspacesByThread ?? this.workspacesByThread,
      workspaceUiByThread: workspaceUiByThread ?? this.workspaceUiByThread,
      tasksByRootThread: tasksByRootThread ?? this.tasksByRootThread,
      agentsByThread: agentsByThread ?? this.agentsByThread,
      providers: providers ?? this.providers,
      providerCatalog: providerCatalog ?? this.providerCatalog,
      defaultProviderId: identical(defaultProviderId, _studioStateUnset)
          ? this.defaultProviderId
          : defaultProviderId as String?,
      providerUsages: providerUsages ?? this.providerUsages,
      roles: roles ?? this.roles,
      mcpServers: mcpServers ?? this.mcpServers,
      instructions: instructions ?? this.instructions,
      skills: skills ?? this.skills,
      general: general ?? this.general,
      webSearch: webSearch ?? this.webSearch,
      selectedProjectId: identical(selectedProjectId, _studioStateUnset)
          ? this.selectedProjectId
          : selectedProjectId as String?,
      selectedThreadId: identical(selectedThreadId, _studioStateUnset)
          ? this.selectedThreadId
          : selectedThreadId as String?,
      permissionMode: permissionMode ?? this.permissionMode,
      recoveryIssues: recoveryIssues ?? this.recoveryIssues,
    );
  }
}
