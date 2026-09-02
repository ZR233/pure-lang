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
  List<SshServer> sshServers = const [];
  SaveSshServerCommand? savedSshServer;
  String? deletedSshServerId;
  String? testedSshServerId;
  String? reconnectedSshServerId;
  ({String serverId, String? path})? browsedRemoteDirectory;
  ({String serverId, String path})? openedRemoteProject;
  String? selectedProjectRequest;
  int activateCallCount = 0;
  String? activatedProjectId;
  String? createdThreadProjectId;
  StudioMode? createdThreadMode;
  String? newThreadPrompt;
  String? archivedThreadId;
  String? renamedThreadId;
  String? renamedThreadTitle;
  Object? renameThreadError;
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
  DeepSeekWebSearchSettingsCommand? savedDeepSeekWebSearchSettings;
  PermissionMode? savedPermissionMode;
  String? resolvedInteractionId;
  Map<String, Object?>? resolvedInteraction;
  Object? resolveInteractionError;
  Completer<PendingInteraction>? blockedInteractionResponse;
  int resolveInteractionCount = 0;
  String? discoverProjectId;
  int discoverCallCount = 0;
  int readSkillsCallCount = 0;
  int searchSkillsCallCount = 0;
  final List<({String projectId, String query, int limit})>
  skillSearchRequests = [];
  final List<Completer<SkillSearchResultView>> blockedSkillSearchResponses = [];
  int _skillsCatalogRevision = 0;
  List<String> _discoveredSkills = const [];
  List<SkillSummaryView> discoveredSkillSummaries = const [];
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
  final List<({String threadId, StudioPromptInput input})> submittedInputs = [];
  final List<
    ({AttachmentAdmissionContext context, List<AttachmentDraftSource> sources})
  >
  attachmentAdmissionRequests = [];
  List<AttachmentDraftView> nextAdmittedDrafts = const [];
  Object? attachmentAdmissionError;
  final Map<String, Uint8List> attachmentDraftBytes = {};
  final Map<({String threadId, String attachmentId}), Uint8List>
  threadAttachmentBytes = {};
  final Map<({String threadId, String attachmentId}), Object>
  threadAttachmentErrors = {};
  final List<String> removedAttachmentDraftIds = [];
  final List<({String threadId, String attachmentId})>
  readThreadAttachmentRequests = [];
  Completer<SubmitPromptReceipt>? blockedPromptSubmit;
  Exception? submitPromptError;
  String? submitReceiptSessionId;
  String submitTurnId = 'turn-1';
  ({String threadId, String turnId})? interruptedTurn;
  int retryPersistenceCallCount = 0;
  Object? retryPersistenceError;
  PersistenceStateSnapshot? retryPersistenceState;
  ({String childId, int expectedLeaseRevision})? cleanedWorktree;

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
  Future<List<AgentProfileView>> readAgentProfiles() async => [
    for (final role in const [
      'explorer',
      'planner',
      'executor',
      'worktree_executor',
      'reviewer',
    ])
      AgentProfileView(
        id: role,
        displayName: role,
        description: 'System $role profile',
        whenToUse: 'Use for $role work',
        systemInstructions: 'Follow the frozen assignment.',
        providerId: _currentState.role(role)?.providerId ?? 'deepseek',
        model: _currentState.role(role)?.model ?? 'deepseek-v4-flash',
        effort: _currentState.role(role)?.effort,
        source: 'studio-builtin',
        revision: 'system-v2',
        contentHash: 'system-$role',
        system: true,
        enabled: true,
        workspaceMode: switch (role) {
          'executor' => AgentWorkspaceMode.directory,
          'worktree_executor' => AgentWorkspaceMode.worktree,
          _ => AgentWorkspaceMode.unrestricted,
        },
      ),
  ];

  @override
  Future<SettingsStateSnapshot> setSystemAgentEnabled({
    required int expectedSettingsRevision,
    required String profileId,
    required bool enabled,
  }) async {
    final snapshot = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
    );
    _currentState = _currentState.copyWith(settingsState: snapshot);
    return snapshot;
  }

  @override
  Future<SettingsStateSnapshot> saveUserAgentProfile(
    int expectedSettingsRevision,
    AgentProfileDraft draft,
  ) async {
    final snapshot = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
    );
    _currentState = _currentState.copyWith(settingsState: snapshot);
    return snapshot;
  }

  @override
  Future<void> cleanupPreservedWorktree({
    required String childId,
    required int expectedLeaseRevision,
  }) async {
    cleanedWorktree = (
      childId: childId,
      expectedLeaseRevision: expectedLeaseRevision,
    );
  }

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
  Future<List<SshServer>> listSshServers() async => List.of(sshServers);

  @override
  Future<SshServer> saveSshServer(SaveSshServerCommand command) async {
    savedSshServer = command;
    final server = SshServer(
      id: command.id ?? 'ssh-created',
      name: command.name,
      host: command.host,
      port: command.port,
      username: command.username,
      authKind: command.authKind,
      identityFile: command.identityFile,
    );
    sshServers = [
      for (final current in sshServers)
        if (current.id != server.id) current,
      server,
    ];
    return server;
  }

  @override
  Future<void> deleteSshServer(String serverId) async {
    deletedSshServerId = serverId;
    sshServers = [
      for (final server in sshServers)
        if (server.id != serverId) server,
    ];
  }

  @override
  Future<SshConnectionView> testSshConnection(String serverId) async {
    testedSshServerId = serverId;
    return SshConnectionView(
      serverId: serverId,
      state: 'ready',
      helperVersion: '0.1.0',
      architecture: 'aarch64',
    );
  }

  @override
  Future<SshConnectionView> reconnectSshServer(String serverId) async {
    reconnectedSshServerId = serverId;
    return SshConnectionView(
      serverId: serverId,
      state: 'ready',
      helperVersion: '0.1.0',
      architecture: 'aarch64',
    );
  }

  @override
  Future<RemoteDirectoryListing> browseRemoteDirectories(
    String serverId, {
    String? path,
  }) async {
    browsedRemoteDirectory = (serverId: serverId, path: path);
    return RemoteDirectoryListing(
      path: path ?? '/workspace',
      parent: path == null ? '/' : null,
      entries: const [
        RemoteDirectoryEntry(name: 'project', path: '/workspace/project'),
      ],
    );
  }

  @override
  Future<StudioProject> openRemoteProject(String serverId, String path) async {
    openedRemoteProject = (serverId: serverId, path: path);
    return StudioProject(
      id: 'remote-project',
      name: 'project',
      path: path,
      sshServerId: serverId,
    );
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
    StudioPromptInput input,
    StudioMode mode,
  ) async {
    createdThreadProjectId = projectId;
    createdThreadMode = mode;
    newThreadPrompt = input.text;
    submitPromptCount += 1;
    submittedPrompts.add((threadId: '<new>', prompt: input.text));
    submittedInputs.add((threadId: '<new>', input: input));
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
        role: 'planner',
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
  Future<StudioThread> renameThread(String threadId, String title) async {
    renamedThreadId = threadId;
    renamedThreadTitle = title;
    if (renameThreadError case final error?) throw error;
    final current = _currentState.threads
        .where((thread) => thread.id == threadId)
        .firstOrNull;
    if (current == null) throw StateError('unknown Thread $threadId');
    final renamed = current.copyWith(
      title: title.trim(),
      updatedAt: DateTime.fromMillisecondsSinceEpoch(2),
    );
    _currentState = _currentState.copyWith(
      threadDirectory: _currentState.threadDirectory.copyWith(
        threads: [
          for (final thread in _currentState.threads)
            thread.id == threadId ? renamed : thread,
        ],
      ),
    );
    return renamed;
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
  Future<PersistenceStateSnapshot> retryPersistence() async {
    retryPersistenceCallCount += 1;
    if (retryPersistenceError case final error?) throw error;
    return retryPersistenceState ?? _currentState.persistenceState;
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
    final workflow =
        _currentState.workspacesByThread[threadId]?.runtime.workflow;
    if (workflow?.isActive ?? false) {
      throw StateError('thread mode cannot change while a workflow is active');
    }
    modeUpdate = (threadId: threadId, mode: mode);
    final updated = thread.copyWith(mode: mode, role: 'planner');
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
    if (blockedInteractionResponse case final blocked?) {
      return blocked.future;
    }
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
    StudioPromptInput input,
  ) => _submitTurn(threadId, input);

  @override
  Future<SubmitPromptReceipt> steerTurn(
    String threadId,
    StudioPromptInput input,
  ) => _submitTurn(threadId, input);

  @override
  Future<List<AttachmentDraftView>> admitAttachmentDrafts(
    AttachmentAdmissionContext context,
    List<AttachmentDraftSource> sources,
  ) async {
    attachmentAdmissionRequests.add((context: context, sources: sources));
    if (attachmentAdmissionError case final error?) throw error;
    return nextAdmittedDrafts;
  }

  @override
  Future<bool> removeAttachmentDraft(String draftId) async {
    removedAttachmentDraftIds.add(draftId);
    return true;
  }

  @override
  Future<Uint8List> readAttachmentDraft(String draftId) async =>
      attachmentDraftBytes[draftId] ?? Uint8List(0);

  @override
  Future<Uint8List> readThreadAttachment(
    String threadId,
    String attachmentId,
  ) async {
    readThreadAttachmentRequests.add((
      threadId: threadId,
      attachmentId: attachmentId,
    ));
    final error =
        threadAttachmentErrors[(
          threadId: threadId,
          attachmentId: attachmentId,
        )];
    if (error != null) throw error;
    return threadAttachmentBytes[(
          threadId: threadId,
          attachmentId: attachmentId,
        )] ??
        Uint8List(0);
  }

  Future<SubmitPromptReceipt> _submitTurn(
    String threadId,
    StudioPromptInput input,
  ) async {
    submitPromptCount += 1;
    submittedPrompts.add((threadId: threadId, prompt: input.text));
    submittedInputs.add((threadId: threadId, input: input));
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
  Future<SkillSearchResultView> searchSkills(
    String projectId,
    String query, {
    int limit = 50,
  }) async {
    searchSkillsCallCount += 1;
    skillSearchRequests.add((projectId: projectId, query: query, limit: limit));
    if (blockedSkillSearchResponses.isNotEmpty) {
      return blockedSkillSearchResponses.removeAt(0).future;
    }
    final normalizedQuery = query.toLowerCase();
    final allMatches = _skillSummaries()
        .where(
          (skill) =>
              skill.name.toLowerCase().contains(normalizedQuery) ||
              skill.description.toLowerCase().contains(normalizedQuery),
        )
        .toList();
    return SkillSearchResultView(
      projectId: projectId,
      catalogRevision: _skillsCatalogRevision,
      matches: allMatches.take(limit).toList(),
      truncated: allMatches.length > limit,
    );
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
  Future<SettingsStateSnapshot> saveDeepSeekWebSearchSettings(
    int expectedSettingsRevision,
    DeepSeekWebSearchSettingsCommand command,
  ) async {
    savedDeepSeekWebSearchSettings = command;
    final snapshot = _settingsSnapshot(
      _currentState.settingsState,
      revision: expectedSettingsRevision + 1,
      deepSeekWebSearch: _currentState.deepSeekWebSearch.withConfiguredEnabled(
        command.enabled,
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
    return SkillsStateSnapshot.fromState(
      projectId: projectId,
      state: ReadyObservedResource(
        revision: revision,
        updatedAt: revision,
        lastCheckedAt: null,
        value: SkillsStateData(
          configFingerprint: 'fake-skills',
          catalogRevision: revision,
          skills: discoveredSkills,
          summaries: _skillSummaries(),
          warnings: const [],
        ),
      ),
    );
  }

  List<SkillSummaryView> _skillSummaries() {
    if (discoveredSkillSummaries.isNotEmpty) {
      return discoveredSkillSummaries;
    }
    return [
      for (final skill in discoveredSkills)
        SkillSummaryView(
          name: skill,
          description: 'Description for $skill',
          source: 'fake',
          providerId: 'fake',
          modelInvocable: true,
          userInvocable: true,
          resourceBase: const SkillResourceBaseView(
            SkillResourceBaseKind.opaque,
            'fake',
          ),
        ),
    ];
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
  DeepSeekWebSearchSettingsView? deepSeekWebSearch,
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
    deepSeekWebSearch: deepSeekWebSearch ?? current.deepSeekWebSearch,
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
          'hostedWebSearchDialect': provider.hostedWebSearchDialect,
          'standaloneWebSearch': provider.standaloneWebSearch,
          'promptCacheDialect': provider.promptCacheDialect,
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
