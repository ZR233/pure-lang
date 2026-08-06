part of 'studio_api.dart';

class DemoStudioApi implements StudioApi {
  DemoStudioApi({this._providerCatalog = demoProviderCatalogFixture});

  final ProviderCatalogView _providerCatalog;
  final _productEvents = StreamController<Object>.broadcast();
  final _threadEvents = StreamController<ThreadStreamFrame>.broadcast();
  final Map<String, ThreadWorkspace> _workspaces = {};
  final Map<String, int> _promptGenerations = {};
  final Map<String, StudioMode> _threadModes = {};
  final Set<String> _archivedProjectIds = {};
  final Set<String> _archivedThreadIds = {};
  final List<StudioThread> _createdRootThreads = [];

  List<ProviderSettingsView>? _providers;
  List<RoleSettingsView>? _roles;
  InstructionsSettingsView _instructions = const InstructionsSettingsView();
  SkillsSettingsView _skills = const SkillsSettingsView();
  GeneralSettingsView _general = const GeneralSettingsView();
  WebSearchSettingsView _webSearch = const WebSearchSettingsView(
    effectiveMode: 'cached',
    availability: 'available',
    providerId: 'openai',
    model: 'gpt-5',
  );
  PermissionMode _permissionMode = PermissionMode.requestApproval;
  int _turnSequence = 0;
  int _threadSequence = 0;
  String? _selectedThreadId;

  Duration get promptStartDelay => const Duration(milliseconds: 120);

  Duration get promptActivityDelay => const Duration(milliseconds: 350);

  Duration get promptToolDelay => const Duration(milliseconds: 500);

  @override
  Future<StudioState> bootstrap() async {
    final fixture = _ensureWorkspaceFixture();
    final project = fixture.project;
    final threads = fixture.threads;
    final preset = _providerCatalog.presets.first;
    final defaultModels = _providerCatalog.modelsFor(preset.modelCatalogId);
    final defaultProvider = preset
        .createProvider(preset.id, defaultModels)
        .copyWith(hasBearerToken: true, status: 'ready', updatedAt: 'Loaded');
    final defaultEffort = defaultModels
        .where((model) => model.slug == preset.suggestedModel)
        .firstOrNull
        ?.defaultReasoningEffort;
    final resolvedDefaultEffort = defaultEffort?.isNotEmpty == true
        ? defaultEffort!
        : defaultModels
                  .where((model) => model.slug == preset.suggestedModel)
                  .firstOrNull
                  ?.reasoningEfforts
                  .firstOrNull ??
              '';
    if (_archivedProjectIds.contains(project.id)) {
      return StudioState(
        projects: const [],
        threads: const [],
        providers: _providers ?? [defaultProvider],
        providerCatalog: _providerCatalog,
        roles: _roles ?? const [],
        mcpServers: const [],
        instructions: _instructions,
        skills: _skills,
        general: _general,
        webSearch: _webSearch,
        selectedProjectId: null,
        selectedThreadId: null,
        permissionMode: _permissionMode,
      );
    }
    final selectedThreadId =
        threads
            .where((thread) => thread.id == _selectedThreadId)
            .firstOrNull
            ?.id ??
        threads.firstOrNull?.id;
    return StudioState(
      projects: [project],
      threads: threads,
      workspacesByThread: Map.unmodifiable(_workspaces),
      workspaceUiByThread: {
        for (final thread in threads)
          thread.id: const WorkspaceUiState(
            syncState: AgentWorkspaceSyncState.ready,
          ),
      },
      providers: _providers ?? [defaultProvider],
      providerCatalog: _providerCatalog,
      roles:
          _roles ??
          [
            for (final key in const [
              'planner',
              'explorer',
              'executor',
              'reviewer',
            ])
              RoleSettingsView(
                key: key,
                providerId: defaultProvider.id,
                model: defaultProvider.defaultModel,
                effort: resolvedDefaultEffort,
              ),
          ],
      mcpServers: const [],
      instructions: _instructions,
      skills: _skills,
      general: _general,
      webSearch: _webSearch,
      selectedProjectId: project.id,
      selectedThreadId: selectedThreadId,
      permissionMode: _permissionMode,
    );
  }

  ({StudioProject project, List<StudioThread> threads})
  _ensureWorkspaceFixture() {
    final now = DateTime.now();
    const project = StudioProject(
      id: 'project-local',
      name: 'pure-lang',
      path: r'C:\Users\zhoudongsheng\.codex\worktrees\3bc1\pure-lang',
    );
    final rootMode = _threadModes['thread-main'] ?? StudioMode.simple;
    final reviewerMode = _threadModes['thread-reviewer'] ?? StudioMode.simple;
    final alternateMode = _threadModes['thread-alt'] ?? StudioMode.simple;
    final root = StudioThread(
      id: 'thread-main',
      projectId: project.id,
      title: 'Flutter + FRB 重构',
      mode: rootMode,
      role: rootMode == StudioMode.task ? 'planner' : 'executor',
      createdAt: now.subtract(const Duration(minutes: 10)),
      updatedAt: now,
      agentPath: 'root',
    );
    final reviewer = StudioThread(
      id: 'thread-reviewer',
      projectId: project.id,
      title: 'Driver reviewer',
      mode: reviewerMode,
      createdAt: now.subtract(const Duration(minutes: 7)),
      updatedAt: now.subtract(const Duration(minutes: 7)),
      parentThreadId: root.id,
      rootThreadId: root.id,
      agentPath: 'root/reviewer',
      role: 'reviewer',
      status: 'waiting',
    );
    final alternate = StudioThread(
      id: 'thread-alt',
      projectId: project.id,
      title: 'Riverpod selector audit',
      mode: alternateMode,
      role: alternateMode == StudioMode.task ? 'planner' : 'executor',
      createdAt: now.subtract(const Duration(minutes: 3)),
      updatedAt: now.subtract(const Duration(minutes: 3)),
      agentPath: 'root-alt',
    );
    _workspaces.putIfAbsent(root.id, () => _rootWorkspace(root, now));
    _workspaces.putIfAbsent(
      reviewer.id,
      () => _singleMessageWorkspace(
        reviewer,
        'Driver agent workspace selected.',
        'reviewer/model',
        now.subtract(const Duration(minutes: 6)),
      ),
    );
    _workspaces.putIfAbsent(
      alternate.id,
      () => _singleMessageWorkspace(
        alternate,
        'Riverpod selector boundary is isolated.',
        'future-model',
        now.subtract(const Duration(minutes: 2)),
      ),
    );
    _workspaces[root.id] = _workspaces[root.id]!.copyWith(thread: root);
    _workspaces[reviewer.id] = _workspaces[reviewer.id]!.copyWith(
      thread: reviewer,
    );
    _workspaces[alternate.id] = _workspaces[alternate.id]!.copyWith(
      thread: alternate,
    );
    final threads = [root, reviewer, alternate, ..._createdRootThreads]
        .where(
          (thread) =>
              !_archivedThreadIds.contains(thread.id) &&
              !_archivedThreadIds.contains(thread.effectiveRootThreadId),
        )
        .toList();
    for (final thread in _createdRootThreads) {
      _workspaces.putIfAbsent(
        thread.id,
        () => ThreadWorkspace(
          thread: thread,
          revision: 0,
          items: const [],
          interactions: const [],
          runtime: _emptyRuntimeView(),
        ),
      );
    }
    return (project: project, threads: threads);
  }

  ThreadWorkspace _rootWorkspace(StudioThread thread, DateTime now) {
    final userCreatedAt = now.subtract(const Duration(minutes: 9));
    final agentCreatedAt = now.subtract(const Duration(minutes: 8));
    return ThreadWorkspace(
      thread: thread,
      revision: 1,
      items: [
        _messageItem(
          id: 'turn-demo:user',
          threadId: thread.id,
          turnId: 'turn-demo',
          ordinal: 0,
          kind: ThreadItemKind.userMessage,
          text:
              '用 Flutter 重构 Pure Studio。\n\n'
              '- timeline 要像 Web 版一样即时渲染 Markdown\n'
              '- streaming 中的代码块和表格不要抖动',
          createdAt: userCreatedAt,
        ),
        ThreadItemView(
          id: 'turn-demo:reasoning',
          threadId: thread.id,
          turnId: 'turn-demo',
          ordinal: 1,
          revision: 0,
          status: 'completed',
          createdAt: agentCreatedAt,
          updatedAt: agentCreatedAt,
          completedAt: agentCreatedAt,
          kind: ThreadItemKind.reasoning,
          reasoningSummary: const ['## 判断\n\nUI 只消费当前 Thread 的高频通知。'],
        ),
        ThreadItemView(
          id: 'turn-demo:tool',
          threadId: thread.id,
          turnId: 'turn-demo',
          ordinal: 2,
          revision: 0,
          status: 'completed',
          createdAt: agentCreatedAt,
          updatedAt: agentCreatedAt,
          completedAt: agentCreatedAt,
          kind: ThreadItemKind.toolCall,
          tool: const TimelineToolPart(
            toolCallId: 'turn-demo:tool-call',
            name: 'cargo test -p pl-studio-bridge',
            result: '1 passed; bridge envelope uses typed payload.',
          ),
        ),
        _messageItem(
          id: 'turn-demo:plan',
          threadId: thread.id,
          turnId: 'turn-demo',
          ordinal: 3,
          kind: ThreadItemKind.plan,
          text:
              '## Implementation checklist\n\n'
              '1. Keep the Flutter shell aligned with runtime contracts.\n'
              '2. Subscribe only the selected Thread stream.\n\n'
              '| Area | Status |\n| --- | --- |\n| FRB runtime | ready |',
          createdAt: agentCreatedAt,
        ),
        _messageItem(
          id: 'turn-demo:final',
          threadId: thread.id,
          turnId: 'turn-demo',
          ordinal: 4,
          kind: ThreadItemKind.agentMessage,
          channel: AgentMessageChannel.finalAnswer,
          text:
              '### Streaming Markdown preview\n\n'
              '正文、**加粗**、`inline code` 和链接都按 GFM 渲染。',
          createdAt: agentCreatedAt,
        ),
      ],
      interactions: const [],
      runtime: const ThreadRuntimeView(
        model: 'planner/local-responses',
        contextTokens: 18342,
        contextWindow: 128000,
        totalTokens: 26320,
        costLabel: 'CNY 0.16',
        activeSkills: ['flutter-apply-architecture-best-practices'],
        activeMcpServers: ['dart'],
        activeLspServers: ['rust-analyzer'],
        agentCount: 1,
      ),
    );
  }

  ThreadWorkspace _singleMessageWorkspace(
    StudioThread thread,
    String text,
    String model,
    DateTime createdAt,
  ) {
    return ThreadWorkspace(
      thread: thread,
      revision: 1,
      items: [
        _messageItem(
          id: '${thread.id}:final',
          threadId: thread.id,
          turnId: '${thread.id}:turn',
          ordinal: 0,
          kind: ThreadItemKind.agentMessage,
          channel: AgentMessageChannel.finalAnswer,
          text: text,
          createdAt: createdAt,
        ),
      ],
      interactions: const [],
      runtime: ThreadRuntimeView(
        model: model,
        contextTokens: 320,
        contextWindow: 128000,
        totalTokens: 512,
        costLabel: 'CNY 0.01',
        activeSkills: const [],
        activeMcpServers: const ['dart'],
        activeLspServers: const [],
        agentCount: 0,
      ),
    );
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async => _providerCatalog;

  @override
  Future<StudioState> openProject(String path) {
    _archivedProjectIds.remove('project-local');
    return bootstrap();
  }

  @override
  Future<StudioState> selectProject(String projectId) => bootstrap();

  @override
  Future<StudioState> createThread(String projectId, {String? title}) async {
    final current = await bootstrap();
    if (!current.projects.any((project) => project.id == projectId)) {
      throw StateError('unknown demo project $projectId');
    }
    final now = DateTime.now();
    final thread = StudioThread(
      id: 'thread-created-${++_threadSequence}',
      projectId: projectId,
      title: title?.trim().isNotEmpty == true ? title!.trim() : 'New Session',
      mode: StudioMode.simple,
      role: 'executor',
      createdAt: now,
      updatedAt: now,
    );
    _createdRootThreads.add(thread);
    _selectedThreadId = thread.id;
    return bootstrap();
  }

  @override
  Future<StudioState> archiveThread(
    String threadId, {
    String? selectedThreadId,
  }) async {
    final current = await bootstrap();
    final thread = current.threads
        .where((candidate) => candidate.id == threadId)
        .firstOrNull;
    if (thread == null || !thread.isRoot) {
      throw StateError('only a root Thread can be archived');
    }
    final workspace = current.workspacesByThread[threadId];
    if (workspace?.activeTurn?.state.isBusy ?? false) {
      throw StateError('thread tree has an active turn or pending input');
    }
    _archivedThreadIds.add(threadId);
    _selectedThreadId = selectedThreadId;
    return bootstrap();
  }

  @override
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  }) {
    _archivedProjectIds.add(projectId);
    return bootstrap();
  }

  @override
  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId) async {
    return RecoveryCleanupPreview(
      issueId: 'project-cleanup-$projectId',
      expectedRevision: 'demo-project',
      scope: RecoveryIssueScope.project,
      projectId: projectId,
      detail: 'Remove the project and its Pure-owned task worktrees.',
      resources: const [],
    );
  }

  @override
  Future<StudioState> cleanupProject(
    String projectId,
    String expectedRevision, {
    String? selectedProjectId,
  }) {
    _archivedProjectIds.add(projectId);
    return bootstrap();
  }

  @override
  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(
    String issueId,
  ) async {
    return RecoveryCleanupPreview(
      issueId: issueId,
      expectedRevision: 'demo',
      scope: RecoveryIssueScope.thread,
      detail: 'Demo recovery issue',
      resources: const [],
    );
  }

  @override
  Future<StudioState> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision, {
    String? selectedProjectId,
    String? selectedThreadId,
  }) => bootstrap();

  @override
  Future<StudioState> retryRecoveryIssue(
    String issueId, {
    String? selectedProjectId,
    String? selectedThreadId,
  }) => bootstrap();

  @override
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedThreadId,
  }) async {
    final current = await bootstrap();
    _roles = [
      for (final role in current.roles)
        role.key == roleKey
            ? RoleSettingsView(
                key: role.key,
                providerId: providerId,
                model: model,
                effort: effort ?? role.effort,
              )
            : role,
    ];
    return bootstrap();
  }

  @override
  Future<StudioState> setThreadMode({
    required String threadId,
    required StudioMode mode,
  }) async {
    final current = await bootstrap();
    final thread = current.threads
        .where((candidate) => candidate.id == threadId)
        .firstOrNull;
    if (thread == null) {
      throw StateError('unknown demo thread $threadId');
    }
    if (!thread.isRoot) {
      throw StateError('only a root Thread can change mode');
    }
    if (current.tasksByRootThread[threadId]?.isActive ?? false) {
      throw StateError('thread mode cannot change while a task is active');
    }
    _threadModes[threadId] = mode;
    final workspace = _workspaces[threadId];
    if (workspace != null) {
      final thread = workspace.thread;
      _workspaces[threadId] = workspace.copyWith(
        thread: thread.copyWith(
          mode: mode,
          role: thread.isRoot
              ? (mode == StudioMode.task ? 'planner' : 'executor')
              : thread.role,
        ),
      );
    }
    return bootstrap();
  }

  @override
  Stream<Object> subscribeProductEvents() => _productEvents.stream;

  @override
  Stream<ThreadStreamFrame> subscribeThread(String threadId) async* {
    final snapshot = (await bootstrap()).workspacesByThread[threadId];
    if (snapshot == null) {
      throw StateError('unknown demo thread $threadId');
    }
    yield ThreadSnapshotFrame(workspace: snapshot);
    yield* _threadEvents.stream.where(
      (frame) => switch (frame) {
        ThreadSnapshotFrame(:final workspace) =>
          workspace.thread.id == threadId,
        ThreadNotificationFrame(threadId: final id) => id == threadId,
        ThreadResyncRequiredFrame(threadId: final id) => id == threadId,
      },
    );
  }

  @override
  Future<ThreadHistoryPage> listThreadTurns(
    String threadId, {
    String? cursor,
    int limit = 50,
  }) async {
    return const ThreadHistoryPage(items: [], nextCursor: null);
  }

  @override
  Future<SubmitPromptReceipt> startTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  ) => _submitPrompt(threadId, prompt);

  @override
  Future<SubmitPromptReceipt> steerTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  ) => _submitPrompt(threadId, prompt);

  Future<SubmitPromptReceipt> _submitPrompt(
    String threadId,
    String prompt,
  ) async {
    final trimmed = prompt.trim();
    if (trimmed.isEmpty) throw ArgumentError.value(prompt, 'prompt', 'empty');
    final workspace = _workspaces[threadId];
    if (workspace == null) throw StateError('unknown demo thread $threadId');
    final generation = _promptGenerations.update(
      threadId,
      (value) => value + 1,
      ifAbsent: () => 1,
    );
    final turnId = 'demo-turn-${++_turnSequence}';
    final now = DateTime.now();
    final receipt = SubmitPromptReceipt(
      threadId: threadId,
      turnId: turnId,
      cursor: workspace.revision + 1,
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        _messageItem(
          id: '$turnId:user',
          threadId: threadId,
          turnId: turnId,
          ordinal: _nextOrdinal(threadId),
          kind: ThreadItemKind.userMessage,
          text: trimmed,
          createdAt: now,
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadTurnUpdate(
        StudioTurnView(
          turnId: turnId,
          threadId: threadId,
          state: const StudioTurnState.inProgress(StudioTurnActivity.thinking),
          updatedAt: now,
        ),
      ),
    );
    await Future<void>.delayed(promptStartDelay);
    if (_promptGenerations[threadId] != generation) return receipt;
    final reasoningId = '$turnId:reasoning';
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        ThreadItemView(
          id: reasoningId,
          threadId: threadId,
          turnId: turnId,
          ordinal: _nextOrdinal(threadId),
          revision: 0,
          status: 'streaming',
          createdAt: now,
          updatedAt: now,
          kind: ThreadItemKind.reasoning,
          reasoningSummary: const ['## Inspecting the request'],
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return receipt;
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: reasoningId,
          revision: 1,
          field: 'reasoning.summary',
          delta: '\n\nChecking the live ThreadItem projection.',
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return receipt;
    final liveReasoning = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == reasoningId,
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        liveReasoning.copyWith(
          revision: 2,
          status: 'completed',
          updatedAt: DateTime.now(),
          completedAt: DateTime.now(),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadTurnUpdate(
        StudioTurnView(
          turnId: turnId,
          threadId: threadId,
          state: const StudioTurnState.inProgress(
            StudioTurnActivity.runningTool,
          ),
          updatedAt: DateTime.now(),
        ),
      ),
    );
    final toolId = '$turnId:tool';
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        ThreadItemView(
          id: toolId,
          threadId: threadId,
          turnId: turnId,
          ordinal: _nextOrdinal(threadId),
          revision: 0,
          status: 'running',
          createdAt: now,
          updatedAt: now,
          kind: ThreadItemKind.toolCall,
          tool: TimelineToolPart(
            toolCallId: '$turnId:tool-call',
            name: 'exec',
            arguments: jsonEncode({
              'command': 'flutter test test/widget_test.dart',
            }),
          ),
        ),
      ),
    );
    await Future<void>.delayed(promptToolDelay);
    if (_promptGenerations[threadId] != generation) return receipt;
    final runningTool = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == toolId,
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        runningTool.copyWith(
          revision: 1,
          status: 'completed',
          updatedAt: DateTime.now(),
          completedAt: DateTime.now(),
          tool: runningTool.tool?.copyWith(result: 'All widget tests passed.'),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        _messageItem(
          id: '$turnId:final',
          threadId: threadId,
          turnId: turnId,
          ordinal: _nextOrdinal(threadId),
          kind: ThreadItemKind.agentMessage,
          channel: AgentMessageChannel.finalAnswer,
          text:
              'Demo response for: **$trimmed**\n\n'
              '- reasoning 与 tool 都直接来自 ThreadItem',
          createdAt: DateTime.now(),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadTurnUpdate(
        StudioTurnView(
          turnId: turnId,
          threadId: threadId,
          state: const StudioTurnState.completed(),
          updatedAt: DateTime.now(),
        ),
      ),
    );
    return receipt;
  }

  @override
  Future<void> interruptTurn(String threadId, String turnId) async {
    _promptGenerations.update(
      threadId,
      (value) => value + 1,
      ifAbsent: () => 1,
    );
    _emitThreadUpdate(
      threadId,
      ThreadTurnUpdate(
        StudioTurnView(
          turnId: turnId,
          threadId: threadId,
          state: const StudioTurnState.cancelled('Stopped in demo mode'),
          updatedAt: DateTime.now(),
        ),
      ),
    );
  }

  @override
  Future<PendingInteraction> respondInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  ) async {
    const threadId = 'thread-main';
    final workspace = _workspaces[threadId];
    final interaction = workspace?.interactions
        .where((item) => item.id == interactionId)
        .firstOrNull;
    final resolved =
        interaction ??
        PendingInteraction(
          id: interactionId,
          threadId: threadId,
          kind: InteractionKind.userInput,
          title: '',
          body: '',
        );
    _emitThreadUpdate(
      threadId,
      ThreadInteractionUpdate(interaction: resolved, pending: false),
    );
    return resolved;
  }

  @override
  Future<StudioState> saveRuntimePermissionMode(PermissionMode mode) async {
    _permissionMode = mode;
    return bootstrap();
  }

  @override
  Future<StudioState> saveProviderSettings(
    ProviderSettingsCommand command,
  ) async {
    final current = await bootstrap();
    _providers = _providersFromSettingsCommand(
      command,
      previous: current.providers,
      catalog: _providerCatalog,
    );
    _roles = _rolesFromSettingsCommand(command);
    return bootstrap();
  }

  @override
  Future<StudioState> saveInstructionsSettings(
    InstructionsSettingsCommand command,
  ) async {
    _instructions = _instructionsFromSettingsCommand(command);
    return bootstrap();
  }

  @override
  Future<StudioState> saveSkillsSettings(SkillsSettingsCommand command) async {
    _skills = _skillsFromSettingsCommand(command);
    return bootstrap();
  }

  @override
  Future<StudioState> saveMcpSettings(McpSettingsCommand command) =>
      bootstrap();

  @override
  Future<StudioState> saveGeneralSettings(
    GeneralSettingsCommand command,
  ) async {
    _general = GeneralSettingsView(
      followSystemTheme: command.followSystemTheme,
      followActiveTurn: command.followActiveTurn,
      compactTimeline: command.compactTimeline,
    );
    return bootstrap();
  }

  @override
  Future<StudioState> saveWebSearchSettings(
    WebSearchSettingsCommand command,
  ) async {
    _webSearch = WebSearchSettingsView(
      configuredMode: command.mode,
      effectiveMode: command.mode,
      availability: command.mode == 'disabled' ? 'disabled' : 'available',
      contextSize: command.contextSize,
      allowedDomains: command.allowedDomains,
      country: command.country,
      region: command.region,
      city: command.city,
      timezone: command.timezone,
      providerId: 'openai',
      model: 'gpt-5',
    );
    return bootstrap();
  }

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async {
    final current = await bootstrap();
    return [
      for (final provider in current.providers) _demoProviderUsage(provider),
    ];
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    return _archivedProjectIds.contains(projectId)
        ? const []
        : const ['flutter-ui-polish', 'runtime-review', 'studio-settings'];
  }

  void _emitThreadUpdate(String threadId, ThreadWorkspaceUpdate update) {
    final workspace = _workspaces[threadId];
    if (workspace == null) return;
    final revision = workspace.revision + 1;
    final updated = switch (update) {
      ThreadTurnUpdate(:final turn) => workspace.copyWith(
        revision: revision,
        activeTurn: turn.state.isBusy ? turn : null,
      ),
      ThreadItemUpsert(:final item) => _demoUpsertItem(
        workspace,
        revision,
        item,
      ),
      ThreadItemDeltaUpdate(:final delta) => _demoAppendDelta(
        workspace,
        revision,
        delta,
      ),
      ThreadInteractionUpdate(:final interaction, :final pending) =>
        _demoUpdateInteraction(workspace, revision, interaction, pending),
      ThreadRuntimeUpdate(:final runtime, :final todo) => workspace.copyWith(
        revision: revision,
        runtime: runtime,
        todo: todo,
      ),
    };
    _workspaces[threadId] = updated;
    _threadEvents.add(
      ThreadNotificationFrame(
        threadId: threadId,
        revision: revision,
        update: update,
      ),
    );
  }

  int _nextOrdinal(String threadId) {
    final items = _workspaces[threadId]?.items ?? const <ThreadItemView>[];
    return items.isEmpty
        ? 0
        : items
                  .map((item) => item.ordinal)
                  .reduce((left, right) => left > right ? left : right) +
              1;
  }
}

ThreadItemView _messageItem({
  required String id,
  required String threadId,
  required String turnId,
  required int ordinal,
  required ThreadItemKind kind,
  required String text,
  required DateTime createdAt,
  AgentMessageChannel? channel,
}) {
  return ThreadItemView(
    id: id,
    threadId: threadId,
    turnId: turnId,
    ordinal: ordinal,
    revision: 0,
    status: 'completed',
    createdAt: createdAt,
    updatedAt: createdAt,
    completedAt: createdAt,
    kind: kind,
    text: text,
    channel: channel,
  );
}

ThreadWorkspace _demoUpsertItem(
  ThreadWorkspace workspace,
  int revision,
  ThreadItemView incoming,
) {
  final items = [...workspace.items];
  final index = items.indexWhere((item) => item.id == incoming.id);
  if (index < 0) {
    items.add(incoming);
  } else {
    items[index] = incoming;
  }
  items.sort(_compareThreadItems);
  return workspace.copyWith(revision: revision, items: items);
}

ThreadWorkspace _demoAppendDelta(
  ThreadWorkspace workspace,
  int revision,
  ThreadItemDeltaView delta,
) {
  final items = [...workspace.items];
  final index = items.indexWhere((item) => item.id == delta.itemId);
  if (index >= 0) {
    items[index] = items[index].appendDelta(
      field: delta.field,
      delta: delta.delta,
      nextRevision: delta.revision,
    );
  }
  return workspace.copyWith(revision: revision, items: items);
}

ThreadWorkspace _demoUpdateInteraction(
  ThreadWorkspace workspace,
  int revision,
  PendingInteraction interaction,
  bool pending,
) {
  final interactions = [...workspace.interactions];
  final index = interactions.indexWhere((item) => item.id == interaction.id);
  if (!pending) {
    if (index >= 0) interactions.removeAt(index);
  } else if (index < 0) {
    interactions.add(interaction);
  } else {
    interactions[index] = interaction;
  }
  return workspace.copyWith(revision: revision, interactions: interactions);
}

/// Deterministic demo fixture exposed only by the dedicated Driver build.
class DriverDemoStudioApi extends DemoStudioApi {
  @override
  Duration get promptActivityDelay => const Duration(seconds: 3);

  @override
  Duration get promptToolDelay => const Duration(seconds: 3);

  @override
  Future<StudioState> bootstrap() async {
    final state = await super.bootstrap();
    const threadId = 'thread-main';
    final workspace = state.workspacesByThread[threadId];
    if (workspace == null) return state;
    return state.copyWith(
      workspacesByThread: {
        ...state.workspacesByThread,
        threadId: workspace.copyWith(
          interactions: const [
            PendingInteraction(
              id: 'driver-tool',
              threadId: threadId,
              kind: InteractionKind.toolApproval,
              title: 'Approve demo tool',
              body: 'Run a deterministic demo command.',
              payload: ToolApprovalInteractionPayload(
                toolName: 'demo_tool',
                workingDirectory: r'C:\demo',
              ),
            ),
            PendingInteraction(
              id: 'driver-input',
              threadId: threadId,
              kind: InteractionKind.userInput,
              title: 'Demo question',
              body: 'Choose a deterministic answer.',
              payload: UserInputInteractionPayload(
                questions: [
                  UserQuestionView(
                    id: 'driver-question',
                    header: 'Driver',
                    question: 'Continue?',
                    isOther: false,
                    isSecret: false,
                    options: [],
                  ),
                ],
              ),
            ),
            PendingInteraction(
              id: 'driver-plan',
              threadId: threadId,
              kind: InteractionKind.planConfirmation,
              title: 'Confirm demo plan',
              body: 'Implement the deterministic demo plan.',
              payload: PlanConfirmationInteractionPayload(
                planId: 'driver-plan',
                content: '1. Verify stable Driver keys.',
              ),
            ),
          ],
        ),
      },
    );
  }
}
