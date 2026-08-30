import 'agent_models.dart';
import 'agent_workspace_view.dart';
import 'collection_extensions.dart';
import 'composer_models.dart';
import 'interaction_models.dart';
import 'persistence_models.dart';
import 'provider_models.dart';
import 'recovery_models.dart';
import 'runtime_models.dart';
import 'thread_directory_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'studio_state_snapshots.dart';
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
    required this.projectDirectory,
    required this.threadDirectory,
    required this.agentDirectory,
    required this.settingsState,
    required this.recoveryState,
    required this.mcpState,
    required this.lspState,
    required this.skillsByProject,
    required this.providerUsageState,
    this.modelPerformance = const ModelPerformanceSnapshotView(),
    required this.updaterState,
    this.configRecoveryNotice,
    this.persistenceState = const PersistenceStateSnapshot.ready(),
    this.workspacesByThread = const {},
    this.workspaceUiByThread = const {},
    this.newThreadComposerByProject = const {},
    this.newThreadModeByProject = const {},
    this.providerCatalog = const ProviderCatalogView.empty(),
    required this.selectedProjectId,
    required this.selectedThreadId,
  });

  final Map<String, ThreadWorkspace> workspacesByThread;
  final Map<String, WorkspaceUiState> workspaceUiByThread;
  final Map<String, ComposerThreadState> newThreadComposerByProject;
  final Map<String, StudioMode> newThreadModeByProject;
  final ProviderCatalogView providerCatalog;
  final String? selectedProjectId;
  final String? selectedThreadId;
  final ProjectDirectoryState projectDirectory;
  final ThreadDirectoryWindow threadDirectory;
  final AgentDirectoryState agentDirectory;
  final SettingsStateSnapshot settingsState;
  final RecoveryStateSnapshot recoveryState;
  final McpStateSnapshot mcpState;
  final LspStateSnapshot lspState;
  final Map<String, SkillsStateSnapshot> skillsByProject;
  final ProviderUsageStateSnapshot providerUsageState;
  final ModelPerformanceSnapshotView modelPerformance;
  final UpdaterStateSnapshot updaterState;
  final ConfigRecoveryNotice? configRecoveryNotice;
  final PersistenceStateSnapshot persistenceState;

  List<StudioProject> get projects => projectDirectory.values;
  List<StudioThread> get threads => threadDirectory.threads;
  Map<String, StudioAgentView> get agentsByThread => {
    for (final agent in agentDirectory.values) agent.threadId: agent,
  };
  List<ProviderSettingsView> get providers => [
    for (final provider in settingsState.providers)
      providerWithCatalogMetadata(provider, providerCatalog),
  ];
  String? get defaultProviderId => settingsState.defaultProviderId;
  List<ProviderUsageView> get providerUsages => providerUsageState.usages;
  SessionCostView? get selectedSessionCost =>
      modelPerformance.sessionCost(selectedRootThread?.id);
  List<RoleSettingsView> get roles => settingsState.roles;
  List<McpServerSettingsView> get mcpServers => settingsState.mcpServers;
  InstructionsSettingsView get instructions => settingsState.instructions;
  SkillsSettingsView get skills => settingsState.skills;
  GeneralSettingsView get general => settingsState.general;
  WebSearchSettingsView get webSearch => settingsState.webSearch;
  PermissionMode get permissionMode => settingsState.permissionMode;
  List<StudioRecoveryIssue> get recoveryIssues => recoveryState.values;
  int get settingsRevision => settingsState.revision;

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
    final canonical = selectedWorkspace?.runtime ?? _emptySessionRuntime;
    return canonical.copyWith(agentCount: threadsForSelectedRoot.length);
  }

  ComposerThreadState get composer => selectedWorkspaceUi.composer;

  ComposerThreadState get newThreadComposer {
    final projectId = selectedProjectId;
    return projectId == null
        ? const ComposerThreadState.idle()
        : newThreadComposerByProject[projectId] ??
              const ComposerThreadState.idle();
  }

  StudioMode get newThreadMode {
    final projectId = selectedProjectId;
    return projectId == null
        ? StudioMode.simple
        : newThreadModeByProject[projectId] ?? StudioMode.simple;
  }

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
        (left, right) =>
            interactionPriority(left.kind)
                .compareTo(interactionPriority(right.kind)),
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
                state: const IdleStudioAgent(),
                summary: null,
                depth: thread.isRoot ? 0 : 1,
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
    Map<String, ThreadWorkspace>? workspacesByThread,
    Map<String, WorkspaceUiState>? workspaceUiByThread,
    Map<String, ComposerThreadState>? newThreadComposerByProject,
    Map<String, StudioMode>? newThreadModeByProject,
    ProviderCatalogView? providerCatalog,
    Object? selectedProjectId = _studioStateUnset,
    Object? selectedThreadId = _studioStateUnset,
    ProjectDirectoryState? projectDirectory,
    ThreadDirectoryWindow? threadDirectory,
    AgentDirectoryState? agentDirectory,
    SettingsStateSnapshot? settingsState,
    RecoveryStateSnapshot? recoveryState,
    McpStateSnapshot? mcpState,
    LspStateSnapshot? lspState,
    Map<String, SkillsStateSnapshot>? skillsByProject,
    ProviderUsageStateSnapshot? providerUsageState,
    ModelPerformanceSnapshotView? modelPerformance,
    UpdaterStateSnapshot? updaterState,
    Object? configRecoveryNotice = _studioStateUnset,
    PersistenceStateSnapshot? persistenceState,
  }) {
    return StudioState(
      workspacesByThread: workspacesByThread ?? this.workspacesByThread,
      workspaceUiByThread: workspaceUiByThread ?? this.workspaceUiByThread,
      newThreadComposerByProject:
          newThreadComposerByProject ?? this.newThreadComposerByProject,
      newThreadModeByProject:
          newThreadModeByProject ?? this.newThreadModeByProject,
      providerCatalog: providerCatalog ?? this.providerCatalog,
      selectedProjectId: identical(selectedProjectId, _studioStateUnset)
          ? this.selectedProjectId
          : selectedProjectId as String?,
      selectedThreadId: identical(selectedThreadId, _studioStateUnset)
          ? this.selectedThreadId
          : selectedThreadId as String?,
      projectDirectory: projectDirectory ?? this.projectDirectory,
      threadDirectory: threadDirectory ?? this.threadDirectory,
      agentDirectory: agentDirectory ?? this.agentDirectory,
      settingsState: settingsState ?? this.settingsState,
      recoveryState: recoveryState ?? this.recoveryState,
      mcpState: mcpState ?? this.mcpState,
      lspState: lspState ?? this.lspState,
      skillsByProject: skillsByProject ?? this.skillsByProject,
      providerUsageState: providerUsageState ?? this.providerUsageState,
      modelPerformance: modelPerformance ?? this.modelPerformance,
      updaterState: updaterState ?? this.updaterState,
      configRecoveryNotice: identical(configRecoveryNotice, _studioStateUnset)
          ? this.configRecoveryNotice
          : configRecoveryNotice as ConfigRecoveryNotice?,
      persistenceState: persistenceState ?? this.persistenceState,
    );
  }
}
