part of '../widget_test.dart';

class _FakeStudioApi implements StudioApi {
  _FakeStudioApi(
    this.initialState, {
    List<ProviderUsageView>? providerUsages,
    this.providerCatalog = _testProviderCatalog,
  }) : providerUsages = providerUsages ?? _defaultProviderUsages,
       _currentState = initialState;

  final StudioState initialState;
  StudioState _currentState;
  final List<ProviderUsageView> providerUsages;
  final ProviderCatalogView providerCatalog;
  final _global = StreamController<Object>.broadcast();
  StreamController<ThreadStreamFrame> _thread =
      StreamController<ThreadStreamFrame>.broadcast();
  final Map<String, StudioState> sessionStates = {};
  final Map<String, StudioState> selectProjectStates = {};
  final Map<String, StudioState> archiveProjectStates = {};
  Completer<void>? blockedStudioStateLoad;
  final List<String> loadedSessionIds = [];
  final List<String> threadSubscriptions = [];
  final List<({String threadId, String? cursor})> historyRequests = [];
  final Map<String, Map<String?, ThreadHistoryPage>> historyPagesByThread = {};
  final List<String?> directoryPageRequests = [];
  final Map<String?, ThreadDirectoryPage> directoryPages = {};

  /// 排队的历史响应闸门：每次 listThreadTurns 消费队首 Completer，
  /// 用于构造"响应在窗口重建之后才返回"的竞态。
  final List<Completer<void>> historyGates = [];
  final StreamController<StudioShutdownProgress> _shutdownProgress =
      StreamController<StudioShutdownProgress>.broadcast();
  bool shutdownRuntimeCalled = false;
  int bootstrapCount = 0;
  Object? bootstrapError;
  String? openedProjectPath;
  String? selectedProjectRequest;
  int activateCallCount = 0;
  String? activatedProjectId;
  String? createdThreadProjectId;
  StudioMode? createdThreadMode;
  String? newThreadPrompt;
  String? archivedThreadId;
  int archiveThreadCallCount = 0;
  Object? archiveThreadError;
  Completer<ArchiveThreadResult>? blockedArchiveThread;
  String? archiveSelectedThreadId;
  StudioState? createThreadState;
  StudioState? archiveThreadState;
  ArchiveThreadResult? archiveThreadResult;
  String? archivedProjectId;
  String? archiveSelectedProjectId;
  ({String threadId, StudioMode mode})? modeUpdate;
  _RoleUpdate? roleUpdate;
  Completer<SettingsStateSnapshot>? blockedModelRoleSave;
  Map<String, Object?>? savedProviderSettings;
  Map<String, Object?>? savedInstructionsSettings;
  Map<String, Object?>? savedSkillsSettings;
  Map<String, Object?>? savedMcpSettings;
  Map<String, Object?>? savedGeneralSettings;
  WebSearchSettingsCommand? savedWebSearchSettings;
  PermissionMode? savedPermissionMode;
  String? resolvedInteractionId;
  Map<String, Object?>? resolvedInteraction;
  Object? resolveInteractionError;
  int resolveInteractionCount = 0;
  String? discoverProjectId;
  int discoverCallCount = 0;
  int readSkillsCallCount = 0;
  int _skillsCatalogRevision = 0;
  List<String> _discoveredSkills = const [];
  int loadProviderUsagesCount = 0;
  Completer<List<ProviderUsageView>>? blockedProviderUsageLoad;
  Completer<void>? blockedThreadCancellation;
  int readMcpStateCount = 0;
  String? resetMcpServerId;
  int resetAllMcpCount = 0;
  int readLspStateCount = 0;
  String? probedLspProjectId;
  ({String projectId, String serverId})? repairedLspServer;
  ({String projectId, String serverId})? resetLspServerRequest;
  String? resetLspWorkspaceProjectId;
  int submitPromptCount = 0;
  final List<({String threadId, String prompt})> submittedPrompts = [];
  Completer<SubmitPromptReceipt>? blockedPromptSubmit;
  Exception? submitPromptError;
  String? submitReceiptSessionId;
  String submitTurnId = 'turn-1';
  ({String threadId, String turnId})? interruptedTurn;
  final Map<String, RecoveryCleanupPreview> recoveryPreviews = {};
  final Map<String, RecoveryCleanupPreview> projectCleanupPreviews = {};
  int previewProjectCleanupCount = 0;
  Object? previewProjectCleanupError;
  StudioState? projectCleanupState;
  Object? projectCleanupError;
  String? cleanedProjectId;
  String? projectCleanupExpectedRevision;
  int previewRecoveryIssueCleanupCount = 0;
  Object? previewRecoveryIssueCleanupError;
  StudioState? recoveryCleanupState;
  Object? recoveryCleanupError;
  String? cleanedRecoveryIssueId;
  String? cleanupExpectedRevision;
  StudioState? recoveryRetryState;
  Object? recoveryRetryError;
  String? retriedRecoveryIssueId;
  String? retrySelectedProjectId;
  String? retrySelectedThreadId;
  TaskRecoveryPreview? taskRecoveryPreview;
  TaskRecoveryResult? taskRecoveryResult;
  TaskRecoveryRequest? taskRecoveryRequest;
  Object? taskRecoveryPreviewError;
  Object? taskRecoveryApplyError;
  Completer<TaskRecoveryResult>? blockedTaskRecoveryApply;

  void emitGlobal(StudioBridgeEvent event) => _global.add(event);

  /// 测试注入：直接替换下一次 readStudioState 返回的 product 状态
  /// （模拟 resync/reload 快照）。
  void debugReplaceCurrentState(StudioState state) {
    _currentState = state;
  }

  List<String> get discoveredSkills => _discoveredSkills;

  set discoveredSkills(List<String> value) {
    _discoveredSkills = value;
    _skillsCatalogRevision += 1;
  }

  void emitThreadFrame(ThreadStreamFrame frame) => _thread.add(frame);

  Future<void> closeThreadStream() async {
    final closed = _thread;
    _thread = StreamController<ThreadStreamFrame>.broadcast();
    await closed.close();
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async => providerCatalog;

  @override
  Future<StudioState> readStudioState() async {
    bootstrapCount += 1;
    if (bootstrapError case final error?) {
      throw error;
    }
    final blocked = blockedStudioStateLoad;
    if (blocked != null) await blocked.future;
    return _currentState;
  }

  @override
  Future<StudioProject> openProject(String path) async {
    openedProjectPath = path;
    return _currentState.projects.first;
  }

  @override
  Future<void> activateProject(String projectId) async {
    selectedProjectRequest = projectId;
    activateCallCount += 1;
    activatedProjectId = projectId;
    if (selectProjectStates[projectId] case final next?) {
      _currentState = _asNewerProductState(_currentState, next);
    }
  }

  @override
  Future<StartNewThreadResult> startNewThread(
    String projectId,
    String prompt,
    List<String> attachmentIds,
    StudioMode mode,
  ) async {
    createdThreadProjectId = projectId;
    createdThreadMode = mode;
    newThreadPrompt = prompt;
    submitPromptCount += 1;
    submittedPrompts.add((threadId: '<new>', prompt: prompt));
    if (submitPromptError case final error?) throw error;
    final blocked = blockedPromptSubmit;
    final receipt = blocked == null
        ? SubmitPromptReceipt(
            threadId: submitReceiptSessionId ?? 'session-created',
            turnId: submitTurnId,
            cursor: 1,
          )
        : await blocked.future;
    late final StudioThread thread;
    if (createThreadState case final next?) {
      _currentState = _asNewerProductState(_currentState, next);
      thread =
          next.threads
              .where((thread) => !initialState.threads.contains(thread))
              .firstOrNull ??
          next.threads.last;
    } else {
      final now = DateTime.fromMillisecondsSinceEpoch(1);
      thread = StudioThread(
        id: 'session-created',
        projectId: projectId,
        title: 'New Session',
        mode: mode,
        role: mode == StudioMode.task ? 'planner' : 'executor',
        createdAt: now,
        updatedAt: now,
      );
      _currentState = _currentState.copyWith(
        threadDirectory: _currentState.threadDirectory.copyWith(
          threads: [..._currentState.threads, thread],
        ),
      );
    }
    return StartNewThreadResult(
      thread: thread,
      receipt: SubmitPromptReceipt(
        threadId: receipt.threadId == 'session-created'
            ? thread.id
            : receipt.threadId,
        turnId: receipt.turnId,
        cursor: receipt.cursor,
      ),
    );
  }

  @override
  Future<ArchiveThreadResult> archiveThread(String threadId) async {
    archivedThreadId = threadId;
    archiveThreadCallCount += 1;
    if (archiveThreadError case final error?) {
      throw error;
    }
    if (blockedArchiveThread case final gate?) {
      return gate.future;
    }
    final archived = _currentState.threads
        .where((thread) => thread.id == threadId)
        .first;
    final removedThreadIds = _currentState.threads
        .where((thread) => thread.effectiveRootThreadId == threadId)
        .map((thread) => thread.id)
        .toList();
    if (archiveThreadResult case final result?) {
      return result;
    }
    if (archiveThreadState case final next?) {
      _currentState = _asNewerProductState(_currentState, next);
      return ArchiveThreadResult(
        archivedRootId: threadId,
        removedThreadIds: removedThreadIds,
        nextRoot: next.rootThreads
            .where((thread) => thread.id == next.selectedThreadId)
            .firstOrNull,
      );
    }
    final threads = _currentState.threads
        .where((thread) => thread.effectiveRootThreadId != threadId)
        .toList();
    final selectedThreadId = _currentState.selectedThreadId;
    final remainingIds = threads.map((thread) => thread.id).toSet();
    final nextSelected = remainingIds.contains(selectedThreadId)
        ? selectedThreadId
        : threads.where((thread) => thread.isRoot).firstOrNull?.id;
    _currentState = _currentState.copyWith(
      threadDirectory: _currentState.threadDirectory.copyWith(threads: threads),
      selectedThreadId: nextSelected,
    );
    return ArchiveThreadResult(
      archivedRootId: archived.id,
      removedThreadIds: removedThreadIds,
      nextRoot: threads
          .where((thread) => thread.id == nextSelected && thread.isRoot)
          .firstOrNull,
    );
  }

  @override
  Future<void> archiveProject(String projectId) async {
    archivedProjectId = projectId;
    if (archiveProjectStates[projectId] case final next?) {
      _currentState = _asNewerProductState(_currentState, next);
    }
  }

  @override
  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId) async {
    previewProjectCleanupCount += 1;
    if (previewProjectCleanupError case final error?) {
      throw error;
    }
    return projectCleanupPreviews[projectId] ??
        RecoveryCleanupPreview(
          issueId: 'project-cleanup-$projectId',
          expectedRevision: 'revision-$projectId',
          scope: RecoveryIssueScope.project,
          projectId: projectId,
          detail: 'Project cleanup preview',
          resources: const [],
        );
  }

  @override
  Future<void> cleanupProject(String projectId, String expectedRevision) async {
    if (projectCleanupError case final error?) {
      throw error;
    }
    cleanedProjectId = projectId;
    projectCleanupExpectedRevision = expectedRevision;
    if (projectCleanupState case final next?) {
      _currentState = _asNewerProductState(_currentState, next);
    }
  }

  @override
  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(
    String issueId,
  ) async {
    previewRecoveryIssueCleanupCount += 1;
    if (previewRecoveryIssueCleanupError case final error?) {
      throw error;
    }
    return recoveryPreviews[issueId] ??
        RecoveryCleanupPreview(
          issueId: issueId,
          expectedRevision: 'revision-$issueId',
          scope: RecoveryIssueScope.thread,
          detail: 'Recovery cleanup preview',
          resources: const [],
        );
  }

  @override
  Future<void> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision,
  ) async {
    if (recoveryCleanupError case final error?) {
      throw error;
    }
    cleanedRecoveryIssueId = issueId;
    cleanupExpectedRevision = expectedRevision;
    if (recoveryCleanupState case final next?) {
      _currentState = _asNewerProductState(_currentState, next);
    }
  }

  @override
  Future<void> retryRecoveryIssue(String issueId) async {
    if (recoveryRetryError case final error?) {
      throw error;
    }
    retriedRecoveryIssueId = issueId;
    retrySelectedProjectId = _currentState.selectedProjectId;
    retrySelectedThreadId = _currentState.selectedThreadId;
    if (recoveryRetryState case final next?) {
      _currentState = _asNewerProductState(_currentState, next);
    }
  }

  @override
  Future<TaskRecoveryPreview> previewTaskRecovery(String rootThreadId) async {
    if (taskRecoveryPreviewError case final error?) throw error;
    return taskRecoveryPreview ??
        (throw StateError('Missing Task recovery preview for $rootThreadId'));
  }

  @override
  Future<TaskRecoveryResult> applyTaskRecovery(
    TaskRecoveryRequest request,
  ) async {
    taskRecoveryRequest = request;
    if (taskRecoveryApplyError case final error?) throw error;
    final blocked = blockedTaskRecoveryApply;
    if (blocked != null) return blocked.future;
    return taskRecoveryResult ??
        (throw StateError('Missing Task recovery result'));
  }

  @override
  Future<SettingsStateSnapshot> setModelRole({
    required int expectedSettingsRevision,
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    roleUpdate = _RoleUpdate(
      roleKey: roleKey,
      providerId: providerId,
      model: model,
      effort: effort,
    );
    final settings = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
      roles: [
        for (final role in _currentState.roles)
          role.key == roleKey
              ? RoleSettingsView(
                  key: role.key,
                  providerId: providerId,
                  model: model,
                  effort: effort ?? role.effort,
                )
              : role,
      ],
    );
    _currentState = _currentState.copyWith(settingsState: settings);
    return blockedModelRoleSave?.future ?? settings;
  }

  @override
  Future<void> setThreadMode({
    required String threadId,
    required StudioMode mode,
  }) async {
    final thread = _currentState.threads
        .where((candidate) => candidate.id == threadId)
        .firstOrNull;
    if (thread == null) {
      throw StateError('unknown fake thread $threadId');
    }
    if (!thread.isRoot) {
      throw StateError('only a root Thread can change mode');
    }
    if (_currentState.tasksByRootThread[threadId]?.isActive ?? false) {
      throw StateError('thread mode cannot change while a task is active');
    }
    modeUpdate = (threadId: threadId, mode: mode);
    final updated = thread.copyWith(
      mode: mode,
      role: mode == StudioMode.task ? 'planner' : 'executor',
    );
    final workspace = _currentState.workspacesByThread[threadId];
    _currentState = _currentState.copyWith(
      threadDirectory: _currentState.threadDirectory.copyWith(
        threads: [
          for (final candidate in _currentState.threads)
            candidate.id == threadId ? updated : candidate,
        ],
      ),
      workspacesByThread: workspace == null
          ? _currentState.workspacesByThread
          : {
              ..._currentState.workspacesByThread,
              threadId: workspace.copyWith(thread: updated),
            },
    );
  }

  @override
  Future<PendingInteraction> respondInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  ) async {
    resolveInteractionCount += 1;
    if (resolveInteractionError case final error?) {
      throw error;
    }
    resolvedInteractionId = interactionId;
    resolvedInteraction = _interactionResolutionJson(resolution);
    return PendingInteraction(
      id: interactionId,
      threadId: _currentState.selectedThreadId ?? '',
      turnId: 'turn-response',
      kind: InteractionKind.userInput,
      title: '',
      body: '',
    );
  }

  @override
  Stream<Object> subscribeProductEvents() => _global.stream;

  @override
  Stream<ThreadStreamFrame> subscribeThread(String threadId) {
    threadSubscriptions.add(threadId);
    final blockedCancellation = blockedThreadCancellation;
    if (blockedCancellation == null) {
      return _thread.stream;
    }
    late final StreamController<ThreadStreamFrame> controller;
    StreamSubscription<ThreadStreamFrame>? forwarding;
    controller = StreamController<ThreadStreamFrame>(
      onListen: () {
        forwarding = _thread.stream.listen(
          controller.add,
          onError: controller.addError,
          onDone: controller.close,
        );
      },
      onCancel: () async {
        await forwarding?.cancel();
        await blockedCancellation.future;
      },
    );
    return controller.stream;
  }

  @override
  Future<({ThreadWorkspace workspace, String? historyCursor})>
  readThreadSnapshot(String threadId) async {
    final workspace =
        _currentState.workspacesByThread[threadId] ??
        (throw StateError('unknown fake Thread workspace $threadId'));
    return (workspace: workspace, historyCursor: null);
  }

  @override
  Future<ThreadDirectoryPage> listThreadsPage({
    String? cursor,
    int limit = 50,
  }) async {
    directoryPageRequests.add(cursor);
    final fixture = directoryPages[cursor];
    if (fixture != null) return fixture;
    return ThreadDirectoryPage(threads: const [], nextCursor: null);
  }

  @override
  Stream<StudioShutdownProgress> subscribeShutdownProgress() {
    return _shutdownProgress.stream;
  }

  @override
  Future<void> shutdownRuntime() async {
    shutdownRuntimeCalled = true;
  }

  @override
  Future<ThreadHistoryPage> listThreadTurns(
    String threadId, {
    String? cursor,
    int limit = 50,
  }) async {
    historyRequests.add((threadId: threadId, cursor: cursor));
    if (historyGates.isNotEmpty) {
      await historyGates.removeAt(0).future;
    }
    return historyPagesByThread[threadId]?[cursor] ??
        const ThreadHistoryPage(items: [], nextCursor: null);
  }

  @override
  Future<SubmitPromptReceipt> startTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  ) => _submitTurn(threadId, prompt);

  @override
  Future<SubmitPromptReceipt> steerTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  ) => _submitTurn(threadId, prompt);

  Future<SubmitPromptReceipt> _submitTurn(
    String threadId,
    String prompt,
  ) async {
    submitPromptCount += 1;
    submittedPrompts.add((threadId: threadId, prompt: prompt));
    if (submitPromptError case final error?) {
      throw error;
    }
    final blocked = blockedPromptSubmit;
    if (blocked != null) {
      return blocked.future;
    }
    return SubmitPromptReceipt(
      threadId: submitReceiptSessionId ?? threadId,
      turnId: submitTurnId,
      cursor: submitPromptCount,
    );
  }

  @override
  Future<void> interruptTurn(String threadId, String turnId) async {
    interruptedTurn = (threadId: threadId, turnId: turnId);
  }

  @override
  Future<SettingsStateSnapshot> saveRuntimePermissionMode(
    int expectedSettingsRevision,
    PermissionMode mode,
  ) async {
    savedPermissionMode = mode;
    final settings = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
      permissionMode: mode,
    );
    _currentState = _currentState.copyWith(settingsState: settings);
    return settings;
  }

  @override
  Future<SkillsStateSnapshot> readSkillsState(String projectId) async {
    readSkillsCallCount += 1;
    return _skillsState(projectId, revision: _skillsCatalogRevision);
  }

  @override
  Future<SkillsStateSnapshot> discoverSkills(String projectId) async {
    discoverProjectId = projectId;
    discoverCallCount += 1;
    _skillsCatalogRevision += 1;
    return _skillsState(projectId, revision: _skillsCatalogRevision);
  }

  @override
  Future<McpStateSnapshot> readMcpState() async {
    readMcpStateCount += 1;
    return _currentState.mcpState;
  }

  @override
  Future<McpStateSnapshot> resetMcpServer(String serverId) async {
    resetMcpServerId = serverId;
    return _nextMcpState();
  }

  @override
  Future<McpStateSnapshot> resetAllMcp() async {
    resetAllMcpCount += 1;
    return _nextMcpState();
  }

  @override
  Future<LspStateSnapshot> readLspState() async {
    readLspStateCount += 1;
    return _currentState.lspState;
  }

  @override
  Future<LspStateSnapshot> probeLspServer(String projectId) async {
    probedLspProjectId = projectId;
    return _nextLspState();
  }

  @override
  Future<LspStateSnapshot> repairLspServer(
    String projectId,
    String serverId,
  ) async {
    repairedLspServer = (projectId: projectId, serverId: serverId);
    return _nextLspState();
  }

  @override
  Future<LspStateSnapshot> resetLspServer(
    String projectId,
    String serverId,
  ) async {
    resetLspServerRequest = (projectId: projectId, serverId: serverId);
    return _nextLspState();
  }

  @override
  Future<LspStateSnapshot> resetLspWorkspace(String projectId) async {
    resetLspWorkspaceProjectId = projectId;
    return _nextLspState();
  }

  McpStateSnapshot _nextMcpState() {
    final current = _currentState.mcpState;
    final snapshot = McpStateSnapshot(
      revision: current.revision + 1,
      desiredConfigFingerprint: current.desiredConfigFingerprint,
      appliedConfigFingerprint: current.appliedConfigFingerprint,
      activeServers: current.activeServers,
      servers: current.servers,
    );
    _currentState = _currentState.copyWith(mcpState: snapshot);
    return snapshot;
  }

  LspStateSnapshot _nextLspState() {
    final current = _currentState.lspState;
    final snapshot = LspStateSnapshot(
      revision: current.revision + 1,
      activeServers: current.activeServers,
      servers: current.servers,
    );
    _currentState = _currentState.copyWith(lspState: snapshot);
    return snapshot;
  }

  @override
  Future<SettingsStateSnapshot> saveProviderSettings(
    int expectedSettingsRevision,
    ProviderSettingsCommand command,
  ) async {
    final settings = _providerSettingsCommandJson(command);
    savedProviderSettings = settings;
    final snapshot = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
      defaultProviderId: settings['defaultProviderId'] as String?,
      providers: [
        for (final value in settings['providers'] as List<Object?>)
          _providerFromSettings(value),
      ],
      roles: [
        for (final role in command.roles)
          RoleSettingsView(
            key: role.key,
            providerId: role.providerId,
            model: role.model,
            effort: role.effort,
          ),
      ],
    );
    _currentState = _currentState.copyWith(settingsState: snapshot);
    return snapshot;
  }

  @override
  Future<SettingsStateSnapshot> saveInstructionsSettings(
    int expectedSettingsRevision,
    InstructionsSettingsCommand command,
  ) async {
    final settings = <String, Object?>{
      'baseOverride': command.baseOverride,
      'developer': command.developer,
      'user': command.user,
      'projectDocMaxBytes': command.projectDocMaxBytes,
      'projectDocFallbackFilenames': command.projectDocFallbackFilenames,
    };
    savedInstructionsSettings = settings;
    final snapshot = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
      instructions: InstructionsSettingsView(
        baseOverride: settings['baseOverride'] as String? ?? '',
        developer: settings['developer'] as String? ?? '',
        user: settings['user'] as String? ?? '',
        projectDocMaxBytes: settings['projectDocMaxBytes'] as int? ?? 65536,
        projectDocFallbackFilenames: [
          for (final value
              in settings['projectDocFallbackFilenames'] as List<Object?>? ??
                  const <Object?>[])
            value.toString(),
        ],
      ),
    );
    _currentState = _currentState.copyWith(settingsState: snapshot);
    return snapshot;
  }

  @override
  Future<SettingsStateSnapshot> saveSkillsSettings(
    int expectedSettingsRevision,
    SkillsSettingsCommand command,
  ) async {
    final settings = <String, Object?>{
      'enabled': command.enabled,
      'autoLearn': command.autoLearn,
      'systemEnabled': command.systemEnabled,
      'projectDir': command.projectDir,
      'userDir': command.userDir,
      'externalDirs': command.externalDirs,
      'disabled': command.disabled,
      'autoLearnMinToolCalls': command.autoLearnMinToolCalls,
    };
    savedSkillsSettings = settings;
    final snapshot = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
      skills: _currentState.skills.copyWith(
        disabled: [
          for (final value
              in settings['disabled'] as List<Object?>? ?? const <Object?>[])
            value.toString(),
        ],
      ),
    );
    _currentState = _currentState.copyWith(settingsState: snapshot);
    return snapshot;
  }

  @override
  Future<SettingsStateSnapshot> saveMcpSettings(
    int expectedSettingsRevision,
    McpSettingsCommand command,
  ) async {
    final settings = <String, Object?>{
      'servers': [
        for (final server in command.servers)
          {
            'id': server.id,
            'enabled': server.enabled,
            'transport': server.transport,
            'endpoint': server.endpoint,
          },
      ],
    };
    savedMcpSettings = settings;
    final snapshot = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
      mcpServers: [
        for (final server in command.servers)
          McpServerSettingsView(
            id: server.id,
            transport: server.transport,
            endpoint: server.endpoint,
            state: server.enabled
                ? const McpCheckingState(message: 'MCP health check is pending')
                : const McpDisabledState(
                    message: 'MCP server is disabled in configuration',
                  ),
          ),
      ],
    );
    _currentState = _currentState.copyWith(settingsState: snapshot);
    return snapshot;
  }

  @override
  Future<SettingsStateSnapshot> saveGeneralSettings(
    int expectedSettingsRevision,
    GeneralSettingsCommand command,
  ) async {
    final settings = <String, Object?>{
      'followSystemTheme': command.followSystemTheme,
      'followActiveTurn': command.followActiveTurn,
      'compactTimeline': command.compactTimeline,
    };
    savedGeneralSettings = settings;
    final snapshot = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
      general: GeneralSettingsView(
        followSystemTheme: settings['followSystemTheme'] as bool? ?? true,
        followActiveTurn: settings['followActiveTurn'] as bool? ?? true,
        compactTimeline: settings['compactTimeline'] as bool? ?? false,
      ),
    );
    _currentState = _currentState.copyWith(settingsState: snapshot);
    return snapshot;
  }

  @override
  Future<SettingsStateSnapshot> saveWebSearchSettings(
    int expectedSettingsRevision,
    WebSearchSettingsCommand command,
  ) async {
    savedWebSearchSettings = command;
    final snapshot = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
      webSearch: _currentState.webSearch.withConfiguredValues(
        configuredMode: command.mode,
        contextSize: command.contextSize,
        allowedDomains: command.allowedDomains,
        country: command.country,
        region: command.region,
        city: command.city,
        timezone: command.timezone,
      ),
    );
    _currentState = _currentState.copyWith(settingsState: snapshot);
    return snapshot;
  }

  @override
  Future<ProviderUsageStateSnapshot> checkProviderUsage() async {
    loadProviderUsagesCount += 1;
    final blocked = blockedProviderUsageLoad;
    final usages = blocked == null ? providerUsages : await blocked.future;
    final snapshot = ProviderUsageStateSnapshot(
      revision: _currentState.providerUsageState.revision + 1,
      usages: usages,
    );
    _currentState = _currentState.copyWith(providerUsageState: snapshot);
    return snapshot;
  }

  SkillsStateSnapshot _skillsState(String projectId, {int revision = 0}) {
    return SkillsStateSnapshot(
      projectId: projectId,
      configFingerprint: 'fake-skills',
      catalogRevision: revision,
      skills: discoveredSkills,
      warnings: const [],
      revision: revision,
    );
  }
}

SettingsStateSnapshot _settingsSnapshot(
  SettingsStateSnapshot current, {
  required int revision,
  List<ProviderSettingsView>? providers,
  Object? defaultProviderId = _fakeUnset,
  List<RoleSettingsView>? roles,
  List<McpServerSettingsView>? mcpServers,
  InstructionsSettingsView? instructions,
  SkillsSettingsView? skills,
  GeneralSettingsView? general,
  WebSearchSettingsView? webSearch,
  PermissionMode? permissionMode,
}) {
  return SettingsStateSnapshot(
    revision: revision,
    providers: providers ?? current.providers,
    defaultProviderId: identical(defaultProviderId, _fakeUnset)
        ? current.defaultProviderId
        : defaultProviderId as String?,
    roles: roles ?? current.roles,
    mcpServers: mcpServers ?? current.mcpServers,
    instructions: instructions ?? current.instructions,
    skills: skills ?? current.skills,
    general: general ?? current.general,
    webSearch: webSearch ?? current.webSearch,
    permissionMode: permissionMode ?? current.permissionMode,
  );
}

const _fakeUnset = Object();

StudioState _asNewerProductState(StudioState current, StudioState next) {
  return next.copyWith(
    projectDirectory: ProjectDirectoryState(
      revision: current.projectDirectory.revision + 1,
      values: next.projects,
    ),
    threadDirectory: ThreadDirectoryWindow(
      threads: next.threads,
      nextCursor: next.threadDirectory.nextCursor,
      hasMore: next.threadDirectory.hasMore,
    ),
    taskDirectory: TaskDirectoryState(
      revision: current.taskDirectory.revision + 1,
      values: next.taskDirectory.values,
    ),
    agentDirectory: AgentDirectoryState(
      revision: current.agentDirectory.revision + 1,
      values: next.agentDirectory.values,
    ),
    recoveryState: RecoveryStateSnapshot(
      revision: current.recoveryState.revision + 1,
      values: next.recoveryIssues,
    ),
    providerUsageState: ProviderUsageStateSnapshot(
      revision: current.providerUsageState.revision + 1,
      configFingerprint: next.providerUsageState.configFingerprint,
      usages: next.providerUsages,
    ),
    updaterState: _nextUpdaterState(next.updaterState),
  );
}

UpdaterStateSnapshot _nextUpdaterState(UpdaterStateSnapshot state) {
  final revision = state.revision + 1;
  return switch (state) {
    DisabledUpdaterStateSnapshot(:final updatedAt) =>
      DisabledUpdaterStateSnapshot(revision: revision, updatedAt: updatedAt),
    IdleUpdaterStateSnapshot(:final updatedAt) => IdleUpdaterStateSnapshot(
      revision: revision,
      updatedAt: updatedAt,
    ),
    CheckingUpdaterStateSnapshot(:final operationId, :final startedAt) =>
      CheckingUpdaterStateSnapshot(
        revision: revision,
        operationId: operationId,
        startedAt: startedAt,
      ),
    UpToDateUpdaterStateSnapshot(:final checkedAt) =>
      UpToDateUpdaterStateSnapshot(revision: revision, checkedAt: checkedAt),
    AvailableUpdaterStateSnapshot(:final checkedAt, :final update) =>
      AvailableUpdaterStateSnapshot(
        revision: revision,
        checkedAt: checkedAt,
        update: update,
      ),
    DownloadingUpdaterStateSnapshot(
      :final updatedAt,
      :final update,
      :final downloaded,
      :final total,
    ) =>
      DownloadingUpdaterStateSnapshot(
        revision: revision,
        updatedAt: updatedAt,
        update: update,
        downloaded: downloaded,
        total: total,
      ),
    VerifyingUpdaterStateSnapshot(
      :final updatedAt,
      :final update,
      :final downloaded,
      :final total,
    ) =>
      VerifyingUpdaterStateSnapshot(
        revision: revision,
        updatedAt: updatedAt,
        update: update,
        downloaded: downloaded,
        total: total,
      ),
    InstallerLaunchedUpdaterStateSnapshot(:final launchedAt, :final update) =>
      InstallerLaunchedUpdaterStateSnapshot(
        revision: revision,
        launchedAt: launchedAt,
        update: update,
      ),
    CheckFailedUpdaterStateSnapshot(:final failedAt, :final error) =>
      CheckFailedUpdaterStateSnapshot(
        revision: revision,
        failedAt: failedAt,
        error: error,
      ),
    InstallFailedUpdaterStateSnapshot(
      :final failedAt,
      :final update,
      :final error,
    ) =>
      InstallFailedUpdaterStateSnapshot(
        revision: revision,
        failedAt: failedAt,
        update: update,
        error: error,
      ),
  };
}

Map<String, Object?> _providerSettingsCommandJson(
  ProviderSettingsCommand command,
) {
  return {
    'defaultProviderId': command.defaultProviderId,
    'providers': [
      for (final provider in command.providers)
        {
          'id': provider.id,
          'originalId': provider.originalId,
          'templateKind': provider.templateKind,
          'name': provider.name,
          'baseUrl': provider.baseUrl,
          'bearerToken': provider.secret.value ?? '',
          'capabilitySource': provider.capabilitySource,
          'hostedWebSearch': provider.hostedWebSearch,
          'standaloneWebSearch': provider.standaloneWebSearch,
          'promptCacheDialect': provider.promptCacheDialect,
          'responsesToolSearch': provider.responsesToolSearch,
          'responsesProgrammaticToolCalling':
              provider.responsesProgrammaticToolCalling,
          'defaultModel': provider.defaultModel,
          'customModels': [
            for (final model in provider.customModels)
              {
                'slug': model.slug,
                'displayName': model.displayName,
                'reasoningEfforts': model.reasoningEfforts,
                'baseInstructions': model.baseInstructions,
                'wireProtocol': model.wireProtocol,
                'supportedConnectionModes': model.supportedConnectionModes,
                'defaultConnectionMode': model.defaultConnectionMode,
              },
          ],
          'modelConnectionModes': [
            for (final model in provider.modelConnectionModes)
              {'slug': model.slug, 'connectionMode': model.connectionMode},
          ],
        },
    ],
    'roles': [
      for (final role in command.roles)
        {
          'key': role.key,
          'provider': role.providerId,
          'model': role.model,
          'effort': role.effort,
        },
    ],
  };
}

Map<String, Object?> _interactionResolutionJson(
  InteractionResolutionCommand resolution,
) {
  return switch (resolution) {
    UserInputResolutionCommand(:final answers) => {
      'type': 'userInput',
      'answers': {
        for (final answer in answers)
          answer.questionId: {'answers': answer.answers},
      },
    },
    ToolApprovalResolutionCommand(:final decision, :final reason) => {
      'type': 'toolApproval',
      'decision': decision.name,
      'reason': ?reason,
    },
    PlanConfirmationResolutionCommand(
      :final decision,
      :final content,
      :final reason,
    ) =>
      {
        'type': 'planConfirmation',
        'decision': decision.name,
        'content': ?content,
        'reason': ?reason,
      },
  };
}

const _defaultProviderUsages = [
  ProviderUsageView(
    providerId: 'deepseek',
    updatedAt: 1,
    state: ReadyProviderUsageView(
      data: DeepSeekBalanceProviderUsageView(
        balance: DeepSeekBalanceUsageView(
          isAvailable: true,
          balances: [
            DeepSeekBalanceInfoView(
              currency: 'CNY',
              totalBalance: '88.00',
              grantedBalance: '8.00',
              toppedUpBalance: '80.00',
            ),
          ],
        ),
      ),
    ),
  ),
];
