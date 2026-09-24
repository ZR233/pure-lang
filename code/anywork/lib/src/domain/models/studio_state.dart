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

// Canonical workspace values are replaced by reducers. UI-only anchor/composer
// changes must not rebuild all row projections; old entries are weakly held.
final _timelineRowsByWorkspace =
    Expando<({bool detached, List<TimelineRow> rows})>();
final _timelineHistoryPrefix = Expando<_TimelineHistoryProjection>();

class _TimelineHistoryProjection {
  const _TimelineHistoryProjection(this.split, this.turns, this.rows);
  final int split;
  final Map<String, TimelineTurnView> turns;
  final List<TimelineRow> rows;
}

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
    this.threadModeCatalog = const ThreadModeCatalogView(),
    required this.providerUsageState,
    this.modelPerformance = const ModelPerformanceSnapshotView(),
    required this.updaterState,
    this.persistenceState = const PersistenceStateSnapshot.ready(),
    this.workspacesByThread = const {},
    this.workspaceUiByThread = const {},
    this.openedThreadIds = const {},
    this.newThreadComposerByProject = const {},
    this.newThreadModeByProject = const {},
    this.newThreadWorkspaceModeByProject = const {},
    this.providerCatalog = const ProviderCatalogView.empty(),
    required this.selectedProjectId,
    required this.selectedThreadId,
  });

  final Map<String, ThreadWorkspace> workspacesByThread;
  final Map<String, WorkspaceUiState> workspaceUiByThread;

  /// 已被用户显式打开过的会话。
  ///
  /// 首屏恢复的上次选择只表达选择，不在此集合中（§6.1）：打开是显式用户动作，
  /// 只有打开过的会话才建立订阅、读取当前状态与首个历史窗口。
  final Set<String> openedThreadIds;
  final Map<String, ComposerThreadState> newThreadComposerByProject;
  final Map<String, ThreadModeId> newThreadModeByProject;

  /// 按项目隔离的起始页工作区模式草稿；提交后仍保留，供同一项目再次创建会话。
  final Map<String, ThreadWorkspaceMode> newThreadWorkspaceModeByProject;
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
  final ThreadModeCatalogView threadModeCatalog;
  final ProviderUsageStateSnapshot providerUsageState;
  final ModelPerformanceSnapshotView modelPerformance;
  final UpdaterStateSnapshot updaterState;
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
  List<ModeModelRouteView> get modeModelRoutes => settingsState.modeModelRoutes;
  List<ProviderUsageView> get providerUsages => providerUsageState.usages;
  SessionCostView? get selectedSessionCost =>
      modelPerformance.sessionCost(selectedRootThread?.id);
  List<RoleSettingsView> get roles => settingsState.roles;
  List<McpServerSettingsView> get mcpServers => settingsState.mcpServers;
  InstructionsSettingsView get instructions => settingsState.instructions;
  SkillsSettingsView get skills => settingsState.skills;
  GeneralSettingsView get general => settingsState.general;
  WebSearchSettingsView get webSearch => settingsState.webSearch;
  DeepSeekWebSearchSettingsView get deepSeekWebSearch =>
      settingsState.deepSeekWebSearch;
  PermissionMode get permissionMode => settingsState.permissionMode;
  List<StudioRecoveryIssue> get recoveryIssues => recoveryState.values;
  int get settingsRevision => settingsState.revision;

  ThreadWorkspace? get selectedWorkspace {
    final id = selectedThreadId;
    return id == null ? null : workspacesByThread[id];
  }

  WorkspaceUiState get selectedWorkspaceUi {
    final id = selectedThreadId;
    // 选中但从未打开过的会话没有 workspace UI 条目：按 `idle`（未打开）而不是
    // 默认的 `loading` 处理，避免首屏把“尚未打开”显示成永久加载中。
    return id == null
        ? const WorkspaceUiState()
        : workspaceUiByThread[id] ??
              const WorkspaceUiState(syncState: AgentWorkspaceSyncState.idle);
  }

  StudioTurnView? get turn => selectedWorkspace?.activeTurn;

  ThreadRuntimeView get runtime =>
      selectedWorkspace?.runtime ?? _emptySessionRuntime;

  ComposerThreadState get composer => selectedWorkspaceUi.composer;

  ComposerThreadState get newThreadComposer {
    final projectId = selectedProjectId;
    return projectId == null
        ? const ComposerThreadState.idle()
        : newThreadComposerByProject[projectId] ??
              const ComposerThreadState.idle();
  }

  ThreadModeId get newThreadMode {
    final projectId = selectedProjectId;
    return projectId == null
        ? ThreadModeId.simple
        : newThreadModeByProject[projectId] ?? ThreadModeId.simple;
  }

  /// 当前项目的会话工作区模式草稿；默认 `local`。
  ThreadWorkspaceMode get newThreadWorkspaceMode {
    final projectId = selectedProjectId;
    return projectId == null
        ? ThreadWorkspaceMode.local
        : newThreadWorkspaceModeByProject[projectId] ??
              ThreadWorkspaceMode.local;
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

  List<TimelineRow> get selectedTimelineRows {
    final workspace = selectedWorkspace;
    if (workspace == null) return const [];
    final detached = selectedWorkspaceUi.history.detached;
    final cached = _timelineRowsByWorkspace[workspace];
    if (cached != null && cached.detached == detached) return cached.rows;
    final rows = _projectTimelineRows(workspace, detached);
    _timelineRowsByWorkspace[workspace] = (detached: detached, rows: rows);
    return rows;
  }

  List<TimelineRow> _projectTimelineRows(
    ThreadWorkspace workspace,
    bool detached,
  ) {
    final history = workspace.historyItems;
    final historicalIds = {for (final item in history) item.id};
    final overlay = detached
        ? workspace.liveItems.values
              .where((item) => historicalIds.contains(item.id))
              .toList()
        : workspace.liveItems.values.toList();
    if (overlay.isEmpty) {
      return _historyPrefixRows(workspace, history.length);
    }
    final firstOrdinal = overlay
        .map((item) => item.ordinal)
        .reduce((a, b) => a < b ? a : b);
    var split = history.length;
    while (split > 0 && history[split - 1].ordinal >= firstOrdinal) {
      split--;
    }
    // A tool/reasoning group straddling the SQL/overlay boundary is projected
    // together so its group identity and display order stay stable.
    final first = overlay.where((item) => item.ordinal == firstOrdinal).first;
    if (first.kind == ThreadItemKind.toolCall ||
        first.kind == ThreadItemKind.reasoning) {
      while (split > 0 &&
          history[split - 1].kind == first.kind &&
          history[split - 1].turnId == first.turnId) {
        split--;
      }
    }
    final prefix = _historyPrefixRows(workspace, split);
    final overlayById = {for (final item in overlay) item.id: item};
    final tail = timelineRowsFromThreadItems([
      for (final item in history.skip(split))
        if (!overlayById.containsKey(item.id)) item,
      ...overlay,
    ], turns: workspace.timelineTurns);
    final rows = [...prefix, ...tail];
    if (prefix.isEmpty ||
        tail.isEmpty ||
        prefix.last.sequence < tail.first.sequence) {
      return rows;
    }
    return rows..sort((a, b) {
      final sequence = a.sequence.compareTo(b.sequence);
      if (sequence != 0) return sequence;
      final order = a.order.compareTo(b.order);
      return order != 0 ? order : a.id.compareTo(b.id);
    });
  }

  List<TimelineRow> _historyPrefixRows(ThreadWorkspace workspace, int split) {
    final history = workspace.historyItems;
    final cached = _timelineHistoryPrefix[history];
    if (cached != null &&
        cached.split == split &&
        identical(cached.turns, workspace.timelineTurns)) {
      return cached.rows;
    }
    final rows = timelineRowsFromThreadItems(
      history.take(split).toList(),
      turns: workspace.timelineTurns,
    );
    _timelineHistoryPrefix[history] = _TimelineHistoryProjection(
      split,
      workspace.timelineTurns,
      rows,
    );
    return rows;
  }

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
      loadError: selectedWorkspaceUi.loadError,
      timelineRows: selectedTimelineRows,
      todo: selectedTodoList,
      runtime: runtime,
      turn: turn,
      lastTurn: selectedWorkspace?.lastTurn,
      activeInteraction: activeInteraction,
      composer: composer,
      composerMode: thread.isAgent
          ? AgentComposerMode.runtimeDriven
          : AgentComposerMode.editable,
      permissionMode: permissionMode,
      providers: providers,
      modeModelRoutes: modeModelRoutes,
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
    Set<String>? openedThreadIds,
    Map<String, ComposerThreadState>? newThreadComposerByProject,
    Map<String, ThreadModeId>? newThreadModeByProject,
    Map<String, ThreadWorkspaceMode>? newThreadWorkspaceModeByProject,
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
    ThreadModeCatalogView? threadModeCatalog,
    ProviderUsageStateSnapshot? providerUsageState,
    ModelPerformanceSnapshotView? modelPerformance,
    UpdaterStateSnapshot? updaterState,
    PersistenceStateSnapshot? persistenceState,
  }) {
    return StudioState(
      workspacesByThread: workspacesByThread ?? this.workspacesByThread,
      workspaceUiByThread: workspaceUiByThread ?? this.workspaceUiByThread,
      openedThreadIds: openedThreadIds ?? this.openedThreadIds,
      newThreadComposerByProject:
          newThreadComposerByProject ?? this.newThreadComposerByProject,
      newThreadModeByProject:
          newThreadModeByProject ?? this.newThreadModeByProject,
      newThreadWorkspaceModeByProject:
          newThreadWorkspaceModeByProject ??
          this.newThreadWorkspaceModeByProject,
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
      threadModeCatalog: threadModeCatalog ?? this.threadModeCatalog,
      providerUsageState: providerUsageState ?? this.providerUsageState,
      modelPerformance: modelPerformance ?? this.modelPerformance,
      updaterState: updaterState ?? this.updaterState,
      persistenceState: persistenceState ?? this.persistenceState,
    );
  }
}
