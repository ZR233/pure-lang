part of 'studio_api.dart';

class DemoStudioApi implements StudioApi {
  DemoStudioApi({
    this.lspActivityLoop = false,
    this._providerCatalog = demoProviderCatalogFixture,
  });

  /// 目录分页窗口的页大小；Driver 模式填充大量历史会话以验收触底加载。
  static const int directoryPageSize = 20;

  final ProviderCatalogView _providerCatalog;

  /// 是否周期推进 demo LSP 索引活动；GUI demo 构建开启，测试默认关闭保持确定性。
  final bool lspActivityLoop;
  final _productEvents = StreamController<Object>.broadcast();
  final _threadEvents = StreamController<ThreadStreamFrame>.broadcast();
  final _shutdownEvents = StreamController<StudioShutdownProgress>.broadcast();
  Timer? _lspActivityTimer;
  final Map<String, ThreadWorkspace> _workspaces = {};
  final Map<String, int> _promptGenerations = {};
  final Map<String, ThreadModeId> _threadModes = {};
  final Map<String, AttachmentDraftView> _attachmentDrafts = {};
  final Set<String> _archivedProjectIds = {};
  final Set<String> _archivedThreadIds = {};
  final List<StudioThread> _createdRootThreads = [];
  final List<StudioThread> _pageFillThreads = [];
  final List<SshServer> _sshServers = [
    const SshServer(
      id: 'demo-ssh',
      name: 'ARM development',
      host: '192.168.100.12',
      port: 22,
      username: 'root',
      authKind: SshAuthKind.agentOrKey,
    ),
  ];

  /// Driver 模式注入的历史会话数量；普通 demo 为 0。
  int get directoryPageFillCount => 0;

  List<ProviderSettingsView>? _providers;
  List<RoleSettingsView>? _roles;
  InstructionsSettingsView _instructions = const InstructionsSettingsView();
  SkillsSettingsView _skills = const SkillsSettingsView();
  GeneralSettingsView _general = const GeneralSettingsView();
  WebSearchSettingsView _webSearch = const WebSearchSettingsView(
    effectiveMode: 'disabled',
    availability: 'available',
    selected: false,
    providerId: 'openai',
    model: 'gpt-5',
  );
  DeepSeekWebSearchSettingsView _deepSeekWebSearch =
      const DeepSeekWebSearchSettingsView(
        effectiveEnabled: true,
        availability: 'available',
        selected: true,
        providerId: 'deepseek-primary',
        model: 'deepseek-v4-flash',
      );
  PermissionMode _permissionMode = PermissionMode.requestApproval;
  int _turnSequence = 0;
  int _threadSequence = 0;
  int _settingsRevision = 1;
  int _attachmentSequence = 0;
  int _skillsRevision = 1;
  int _providerUsageRevision = 1;
  int _mcpRevision = 0;
  int _lspRevision = 0;
  int _projectDirectoryRevision = 1;
  final Set<String> _disabledSystemAgents = <String>{};
  final Map<String, AgentProfileView> _userAgentProfiles = {};
  final List<StudioProject> _openedRemoteProjects = [];
  String? _selectedProjectId;
  String? _selectedThreadId;
  DateTime? _fixtureNow;

  Duration get promptStartDelay => const Duration(milliseconds: 120);

  Duration get promptActivityDelay => const Duration(milliseconds: 350);

  Duration get promptToolDelay => const Duration(milliseconds: 500);

  /// 模拟关机每个阶段的停留时长。
  Duration get shutdownPhaseDelay => const Duration(milliseconds: 80);

  /// demo LSP 索引活动每个步进的停留时长。
  Duration get lspActivityStepDelay => const Duration(seconds: 2);

  @override
  Stream<StudioShutdownProgress> subscribeShutdownProgress() {
    return _shutdownEvents.stream;
  }

  @override
  Future<void> shutdownRuntime() async {
    for (final phase in StudioShutdownPhase.values) {
      await Future<void>.delayed(shutdownPhaseDelay);
      if (phase == StudioShutdownPhase.flushingPersistence) {
        _emitShutdownProgress(
          const FlushingPersistenceProgress(pendingCommits: 3),
        );
        await Future<void>.delayed(shutdownPhaseDelay);
        _emitShutdownProgress(
          const FlushingPersistenceProgress(pendingCommits: 0),
        );
        continue;
      }
      _emitShutdownProgress(_demoShutdownProgress(phase));
    }
    await _shutdownEvents.close();
  }

  void _emitShutdownProgress(StudioShutdownProgress progress) {
    _shutdownEvents.add(progress);
    StudioDriverState.publishShutdownProgress(progress);
  }

  StudioShutdownProgress _demoShutdownProgress(StudioShutdownPhase phase) {
    return switch (phase) {
      StudioShutdownPhase.stoppingSubscriptions =>
        const StoppingSubscriptionsProgress(),
      StudioShutdownPhase.cancellingTurns => const CancellingTurnsProgress(),
      StudioShutdownPhase.flushingPersistence =>
        const FlushingPersistenceProgress(pendingCommits: 0),
      StudioShutdownPhase.stoppingAgents => const StoppingAgentsProgress(),
      StudioShutdownPhase.stoppingMcp => const StoppingMcpProgress(),
      StudioShutdownPhase.stoppingLsp => const StoppingLspProgress(),
      StudioShutdownPhase.stopped => const StoppedProgress(),
    };
  }

  @override
  Future<StudioState> readStudioState() async {
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
        projectDirectory: ProjectDirectoryState(
          revision: _projectDirectoryRevision,
          values: const [],
        ),
        threadDirectory: const ThreadDirectoryWindow(),
        agentDirectory: AgentDirectoryState.fromState(
          state: _demoInitialResource(),
        ),
        settingsState: _settingsSnapshot(
          providers: _providers ?? [defaultProvider],
          roles: _roles ?? const [],
        ),
        recoveryState: RecoveryStateSnapshot.fromState(
          state: _demoInitialResource(),
        ),
        mcpState: McpStateSnapshot.fromState(state: _demoInitialResource()),
        lspState: LspStateSnapshot.fromState(state: _demoInitialResource()),
        skillsByProject: const {},
        providerUsageState: ProviderUsageStateSnapshot.fromState(
          state: _demoInitialResource(),
        ),
        modelPerformance: const ModelPerformanceSnapshotView(),
        updaterState: UpdaterStateSnapshot.idle(
          revision: 0,
          updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
        ),
        providerCatalog: _providerCatalog,
        selectedProjectId: null,
        selectedThreadId: null,
      );
    }
    final projects = [project, ..._openedRemoteProjects];
    final selectedProjectId = _selectedProjectId ?? project.id;
    final selectedThreadId = selectedProjectId == project.id
        ? threads
              .where((thread) => thread.id == _selectedThreadId)
              .firstOrNull
              ?.id
        : null;
    final directory = _sortedDirectoryThreads();
    final firstPage = directory.take(directoryPageSize).toList();
    return StudioState(
      projectDirectory: ProjectDirectoryState.fromState(
        state: _demoReadyResource(_projectDirectoryRevision, projects),
      ),
      threadDirectory: ThreadDirectoryWindow(
        threads: firstPage,
        nextCursor: directory.length > directoryPageSize
            ? _demoDirectoryCursor(directoryPageSize - 1)
            : null,
        hasMore: directory.length > directoryPageSize,
      ),
      agentDirectory: AgentDirectoryState.fromState(
        state: _demoInitialResource(),
      ),
      settingsState: _settingsSnapshot(
        providers: _providers ?? [defaultProvider],
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
      ),
      recoveryState: RecoveryStateSnapshot.fromState(
        state: _demoInitialResource(),
      ),
      mcpState: McpStateSnapshot.fromState(state: _demoInitialResource()),
      lspState: LspStateSnapshot.fromState(state: _demoInitialResource()),
      skillsByProject: {
        project.id: _demoSkillsState(project.id, _skillsRevision),
        for (final remote in _openedRemoteProjects)
          remote.id: _demoSkillsState(remote.id, _skillsRevision),
      },
      providerUsageState: ProviderUsageStateSnapshot.fromState(
        state: _demoReadyResource(
          _providerUsageRevision,
          ProviderUsageStateData(
            configFingerprint: 'demo',
            usages: [
              for (final provider in _providers ?? [defaultProvider])
                _demoProviderUsage(provider),
            ],
          ),
        ),
      ),
      modelPerformance: _demoModelPerformance(_fixtureNow!),
      updaterState: UpdaterStateSnapshot.idle(
        revision: 0,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
      ),
      workspacesByThread: Map.unmodifiable(_workspaces),
      workspaceUiByThread: {
        for (final thread in threads)
          thread.id: const WorkspaceUiState(
            syncState: AgentWorkspaceSyncState.ready,
          ),
      },
      providerCatalog: _providerCatalog,
      selectedProjectId: selectedProjectId,
      selectedThreadId: selectedThreadId,
    );
  }

  SettingsStateSnapshot _settingsSnapshot({
    required List<ProviderSettingsView> providers,
    required List<RoleSettingsView> roles,
  }) {
    return SettingsStateSnapshot.fromState(
      state: _demoReadyResource(
        _settingsRevision,
        SettingsStateData(
          providers: providers,
          defaultProviderId: providers.firstOrNull?.id,
          roles: roles,
          instructions: _instructions,
          skills: _skills,
          general: _general,
          webSearch: _webSearch,
          deepSeekWebSearch: _deepSeekWebSearch,
          permissionMode: _permissionMode,
        ),
      ),
    );
  }

  ModelPerformanceSnapshotView _demoModelPerformance(DateTime now) {
    const deepseek = ModelPerformanceSummaryView(
      providerInstanceId: 'deepseek-primary',
      providerDisplayName: 'DeepSeek',
      model: 'deepseek-v4-flash',
      sampleCount: 2,
      completionTokens: 300,
      totalTtftMillis: 760,
      totalDecodeMillis: 2000,
      totalResponseMillis: 2760,
      tokensPerSecond: 150,
      averageTtftMillis: 380,
      averageResponseMillis: 1380,
    );
    const openai = ModelPerformanceSummaryView(
      providerInstanceId: 'openai-work',
      providerDisplayName: 'OpenAI Work',
      model: 'gpt-5.6-codex',
      sampleCount: 1,
      completionTokens: 216,
      totalTtftMillis: 620,
      totalDecodeMillis: 2500,
      totalResponseMillis: 3120,
      tokensPerSecond: 86.4,
      averageTtftMillis: 620,
      averageResponseMillis: 3120,
    );
    return ModelPerformanceSnapshotView(
      revision: 3,
      updatedAt: now,
      sessionCosts: const [
        SessionCostView(
          rootThreadId: 'thread-main',
          estimatedCosts: [
            RuntimeCostView(currency: 'CNY', amount: 0.14),
            RuntimeCostView(currency: 'USD', amount: 0.02),
          ],
          hasUnpricedUsage: false,
        ),
        SessionCostView(
          rootThreadId: 'thread-alt',
          estimatedCosts: [],
          hasUnpricedUsage: true,
        ),
      ],
      summaries: const [deepseek, openai],
      history: [
        ModelPerformanceSampleView(
          completedAt: now.subtract(const Duration(seconds: 12)),
          providerInstanceId: openai.providerInstanceId,
          providerDisplayName: openai.providerDisplayName,
          model: openai.model,
          completionTokens: 216,
          ttftMillis: 620,
          decodeMillis: 2500,
          totalResponseMillis: 3120,
          tokensPerSecond: 86.4,
        ),
        ModelPerformanceSampleView(
          completedAt: now.subtract(const Duration(seconds: 34)),
          providerInstanceId: deepseek.providerInstanceId,
          providerDisplayName: deepseek.providerDisplayName,
          model: deepseek.model,
          completionTokens: 150,
          ttftMillis: 410,
          decodeMillis: 1000,
          totalResponseMillis: 1410,
          tokensPerSecond: 150,
        ),
        ModelPerformanceSampleView(
          completedAt: now.subtract(const Duration(minutes: 2)),
          providerInstanceId: deepseek.providerInstanceId,
          providerDisplayName: deepseek.providerDisplayName,
          model: deepseek.model,
          completionTokens: 150,
          ttftMillis: 350,
          decodeMillis: 1000,
          totalResponseMillis: 1350,
          tokensPerSecond: 150,
        ),
      ],
    );
  }

  ({StudioProject project, List<StudioThread> threads})
  _ensureWorkspaceFixture() {
    final now = _fixtureNow ??= DateTime.now();
    const project = StudioProject(
      id: 'project-local',
      name: 'pure-lang',
      path: r'C:\Projects\pure-lang',
    );
    final rootMode = _threadModes['thread-main'] ?? ThreadModeId.simple;
    final reviewerMode = _threadModes['thread-reviewer'] ?? ThreadModeId.simple;
    final alternateMode = _threadModes['thread-alt'] ?? ThreadModeId.simple;
    final root = StudioThread(
      id: 'thread-main',
      projectId: project.id,
      title: 'Flutter + FRB 重构',
      mode: rootMode,
      role: 'planner',
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
      status: ThreadStatusView.waitingInteraction,
    );
    final alternate = StudioThread(
      id: 'thread-alt',
      projectId: project.id,
      title: 'Riverpod selector audit',
      mode: alternateMode,
      role: 'planner',
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
    _ensurePageFillThreads(project.id, now);
    return (project: project, threads: threads);
  }

  /// Driver 模式的历史会话填充：保证触底分页有足够数据。
  void _ensurePageFillThreads(String projectId, DateTime now) {
    if (directoryPageFillCount <= 0 || _pageFillThreads.isNotEmpty) {
      return;
    }
    for (var index = 0; index < directoryPageFillCount; index++) {
      final updated = now.subtract(Duration(minutes: 30 + index * 17));
      final thread = StudioThread(
        id: 'thread-page-$index',
        projectId: projectId,
        title: '历史会话 ${index + 1}',
        mode: ThreadModeId.simple,
        role: 'planner',
        createdAt: updated,
        updatedAt: updated,
        agentPath: 'root-page-$index',
      );
      _pageFillThreads.add(thread);
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
  }

  /// 目录完整排序视图（updatedAt 倒序、id 倒序），分页窗口的模拟数据源。
  List<StudioThread> _sortedDirectoryThreads() {
    final fixture = _ensureWorkspaceFixture();
    final threads =
        [
          ...fixture.threads,
          ..._pageFillThreads.where(
            (thread) => !_archivedThreadIds.contains(thread.id),
          ),
        ]..sort((a, b) {
          final byUpdated = b.updatedAt.compareTo(a.updatedAt);
          return byUpdated != 0 ? byUpdated : b.id.compareTo(a.id);
        });
    return threads;
  }

  String _demoDirectoryCursor(int lastIndex) => 'demo:$lastIndex';

  @override
  Future<ThreadDirectoryPage> listThreadsPage({
    String? cursor,
    int limit = 50,
  }) async {
    await Future<void>.delayed(const Duration(milliseconds: 120));
    final directory = _sortedDirectoryThreads();
    final lastIndex = cursor == null
        ? -1
        : int.tryParse(cursor.replaceFirst('demo:', '')) ?? -1;
    final page = directory
        .skip(lastIndex + 1)
        .take(limit.clamp(1, 100))
        .toList();
    final nextLastIndex = lastIndex + page.length;
    return ThreadDirectoryPage(
      threads: page,
      nextCursor: nextLastIndex < directory.length - 1
          ? _demoDirectoryCursor(nextLastIndex)
          : null,
    );
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
          createdAt: agentCreatedAt,
          updatedAt: agentCreatedAt,
          state: ThreadThinkingItemStateView(
            summary: const ['## 判断\n\nUI 只消费当前 Thread 的高频通知。'],
            content: const [],
            lifecycle: CompletedThreadContentView(agentCreatedAt),
          ),
        ),
        ThreadItemView(
          id: 'turn-demo:tool',
          threadId: thread.id,
          turnId: 'turn-demo',
          ordinal: 2,
          revision: 0,
          createdAt: agentCreatedAt,
          updatedAt: agentCreatedAt,
          state: ThreadToolItemStateView(
            invocation: const ThreadToolInvocationView(
              toolCallId: 'turn-demo:tool-call',
              name: 'cargo test -p pl-studio-bridge',
              arguments: '',
            ),
            lifecycle: SucceededThreadToolView(
              agentCreatedAt,
              const ThreadToolOutputView(
                result: '1 passed; bridge envelope uses typed payload.',
                attachments: [],
                outputArtifacts: [],
              ),
            ),
          ),
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
        promptTokens: 22160,
        completionTokens: 4160,
        cachedPromptTokens: 8860,
        cacheWriteTokens: 1240,
        cacheMissTokens: 3100,
        reasoningTokens: 1520,
        inferenceCount: 6,
        cacheHitRate: 0.4,
        estimatedCosts: [
          RuntimeCostView(currency: 'CNY', amount: 0.13),
          RuntimeCostView(currency: 'USD', amount: 0.02),
        ],
        estimatedCacheSavings: [RuntimeCostView(currency: 'CNY', amount: 0.05)],
        costLabel: '￥0.13 + \$0.02',
        activeSkills: ['flutter-apply-architecture-best-practices'],
        activeMcpServers: ['dart'],
        activeLspServers: ['rust-analyzer'],
        agentCount: 1,
        turnCompletionTokens: 150,
        turnDecodeMillis: 1000,
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
        estimatedCosts: const [RuntimeCostView(currency: 'CNY', amount: 0.01)],
        costLabel: '￥0.01',
        activeSkills: const [],
        activeMcpServers: const ['dart'],
        activeLspServers: const [],
        agentCount: 0,
        turnCompletionTokens: 216,
        turnDecodeMillis: 2500,
      ),
    );
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async => _providerCatalog;

  @override
  Future<List<AgentProfileView>> readAgentProfiles() async {
    return [
      AgentProfileView(
        id: 'explorer',
        displayName: 'Explorer',
        description: '只读探索代码、文档和现场事实。',
        whenToUse: '需要定位边界、依赖或验证事实时。',
        systemInstructions: '优先收集事实并引用文件与命令。',
        providerId: 'demo',
        model: 'demo',
        effort: 'medium',
        source: 'studio-builtin',
        revision: 'studio-system-agent-v1',
        contentHash: 'demo-explorer',
        system: true,
        enabled: !_disabledSystemAgents.contains('explorer'),
        workspaceMode: AgentWorkspaceMode.unrestricted,
      ),
      AgentProfileView(
        id: 'planner',
        displayName: 'Planner',
        description: '分析目标并形成可执行方案。',
        whenToUse: '需要梳理复杂方案或阶段设计时。',
        systemInstructions: '输出可验证的分阶段计划。',
        providerId: 'demo',
        model: 'demo',
        effort: 'medium',
        source: 'studio-builtin',
        revision: 'studio-system-agent-v1',
        contentHash: 'demo-planner',
        system: true,
        enabled: !_disabledSystemAgents.contains('planner'),
        workspaceMode: AgentWorkspaceMode.unrestricted,
      ),
      AgentProfileView(
        id: 'executor',
        displayName: 'Executor',
        description: '在项目目录内实施边界明确的修改。',
        whenToUse: '需要限制 Pure 内置文件写工具的修改目录时。',
        systemInstructions: '遵守冻结的 writablePaths。',
        providerId: 'demo',
        model: 'demo',
        effort: 'medium',
        source: 'studio-builtin',
        revision: 'studio-system-agent-v2',
        contentHash: 'demo-executor',
        system: true,
        enabled: !_disabledSystemAgents.contains('executor'),
        workspaceMode: AgentWorkspaceMode.directory,
      ),
      AgentProfileView(
        id: 'worktree_executor',
        displayName: 'Worktree Executor',
        description: '在独立 Git worktree 内实施修改。',
        whenToUse: '需要物理隔离并显式整合 commit 时。',
        systemInstructions: '只修改分配的 worktree，不自动整合。',
        providerId: 'demo',
        model: 'demo',
        effort: 'medium',
        source: 'studio-builtin',
        revision: 'studio-system-agent-v2',
        contentHash: 'demo-worktree-executor',
        system: true,
        enabled: !_disabledSystemAgents.contains('worktree_executor'),
        workspaceMode: AgentWorkspaceMode.worktree,
      ),
      AgentProfileView(
        id: 'reviewer',
        displayName: 'Reviewer',
        description: '独立审查实现和验证证据。',
        whenToUse: '需要复核实现质量时。',
        systemInstructions: '只读审查并报告具体证据。',
        providerId: 'demo',
        model: 'demo',
        effort: 'medium',
        source: 'studio-builtin',
        revision: 'studio-system-agent-v2',
        contentHash: 'demo-reviewer',
        system: true,
        enabled: !_disabledSystemAgents.contains('reviewer'),
        workspaceMode: AgentWorkspaceMode.unrestricted,
      ),
      ..._userAgentProfiles.values,
    ];
  }

  @override
  Future<SettingsStateSnapshot> setSystemAgentEnabled({
    required int expectedSettingsRevision,
    required String profileId,
    required bool enabled,
  }) async {
    _checkSettingsRevision(expectedSettingsRevision);
    if (enabled) {
      _disabledSystemAgents.remove(profileId);
    } else {
      _disabledSystemAgents.add(profileId);
    }
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<SettingsStateSnapshot> saveUserAgentProfile(
    int expectedSettingsRevision,
    AgentProfileDraft draft,
  ) async {
    _checkSettingsRevision(expectedSettingsRevision);
    _userAgentProfiles[draft.id] = AgentProfileView(
      id: draft.id,
      displayName: draft.displayName,
      description: draft.description,
      whenToUse: draft.whenToUse,
      systemInstructions: draft.systemInstructions,
      providerId: draft.providerId,
      model: draft.model,
      effort: draft.effort,
      source: '~/.pure/agents/${draft.id}.toml',
      revision: 'demo-user-${draft.id}',
      contentHash: 'demo-user-${draft.id}',
      system: false,
      enabled: draft.enabled,
      workspaceMode: draft.workspaceMode,
    );
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<void> cleanupPreservedWorktree({
    required String childId,
    required int expectedLeaseRevision,
  }) async {}

  @override
  Future<StudioProject> openProject(String path) async {
    if (_archivedProjectIds.remove('project-local')) {
      _projectDirectoryRevision += 1;
    }
    return (await readStudioState()).projects.first;
  }

  @override
  Future<List<SshServer>> listSshServers() async => List.of(_sshServers);

  @override
  Future<SshServer> saveSshServer(SaveSshServerCommand command) async {
    final server = SshServer(
      id: command.id ?? 'demo-ssh-${_sshServers.length + 1}',
      name: command.name,
      host: command.host,
      port: command.port,
      username: command.username,
      authKind: command.authKind,
      identityFile: command.identityFile,
    );
    _sshServers.removeWhere((item) => item.id == server.id);
    _sshServers.add(server);
    return server;
  }

  @override
  Future<void> deleteSshServer(String serverId) async {
    _sshServers.removeWhere((server) => server.id == serverId);
  }

  @override
  Future<SshConnectionView> testSshConnection(String serverId) async {
    return SshConnectionView(
      serverId: serverId,
      state: 'ready',
      helperVersion: '0.1.0',
      architecture: 'aarch64',
    );
  }

  @override
  Future<SshConnectionView> reconnectSshServer(String serverId) =>
      testSshConnection(serverId);

  @override
  Future<RemoteDirectoryListing> browseRemoteDirectories(
    String serverId, {
    String? path,
  }) async {
    final current = path ?? '/home';
    return RemoteDirectoryListing(
      path: current,
      parent: current == '/' ? null : '/',
      entries: const [
        RemoteDirectoryEntry(name: 'projects', path: '/home/projects'),
        RemoteDirectoryEntry(name: 'workspace', path: '/workspace'),
      ],
    );
  }

  @override
  Future<StudioProject> openRemoteProject(String serverId, String path) async {
    final project = StudioProject(
      id: 'project-remote',
      name: path.split('/').last,
      path: path,
      sshServerId: serverId,
    );
    final existingIndex = _openedRemoteProjects.indexWhere(
      (candidate) => candidate.id == project.id,
    );
    if (existingIndex < 0) {
      _openedRemoteProjects.add(project);
      _projectDirectoryRevision += 1;
    } else {
      final existing = _openedRemoteProjects[existingIndex];
      if (existing.name != project.name ||
          existing.path != project.path ||
          existing.sshServerId != project.sshServerId) {
        _openedRemoteProjects[existingIndex] = project;
        _projectDirectoryRevision += 1;
      }
    }
    return project;
  }

  @override
  Future<void> activateProject(String projectId) async {
    if (!(await readStudioState()).projects.any(
      (project) => project.id == projectId,
    )) {
      throw StateError('unknown demo project $projectId');
    }
    _selectedProjectId = projectId;
  }

  @override
  Future<StartNewThreadResult> startNewThread(
    String projectId,
    StudioPromptInput input,
    ThreadModeId mode,
  ) async {
    final current = await readStudioState();
    if (!current.projects.any((project) => project.id == projectId)) {
      throw StateError('unknown demo project $projectId');
    }
    if (input.text.trim().isEmpty && input.attachmentDraftIds.isEmpty) {
      throw ArgumentError.value(input.text, 'text', 'empty');
    }
    final now = DateTime.now();
    final provisionalTitle = _demoProvisionalThreadTitle(input.text);
    final thread = StudioThread(
      id: 'thread-created-${++_threadSequence}',
      projectId: projectId,
      title: provisionalTitle,
      mode: mode,
      role: 'planner',
      createdAt: now,
      updatedAt: now,
    );
    _createdRootThreads.add(thread);
    _ensureWorkspaceFixture();
    _selectedThreadId = thread.id;
    final receipt = await _submitPrompt(thread.id, input);
    if (input.text.trim().isNotEmpty) {
      unawaited(_completeDemoThreadTitle(thread.id, provisionalTitle));
    }
    return StartNewThreadResult(thread: thread, receipt: receipt);
  }

  @override
  Future<StudioThread> renameThread(String threadId, String title) async {
    final normalized = title.trim();
    if (normalized.isEmpty || normalized.runes.length > 80) {
      throw ArgumentError.value(title, 'title', 'must be 1–80 characters');
    }
    final index = _createdRootThreads.indexWhere(
      (thread) => thread.id == threadId,
    );
    if (index < 0) throw StateError('unknown demo root Thread $threadId');
    final renamed = _createdRootThreads[index].copyWith(
      title: normalized,
      updatedAt: DateTime.now(),
    );
    _createdRootThreads[index] = renamed;
    if (_workspaces[threadId] case final workspace?) {
      _workspaces[threadId] = workspace.copyWith(thread: renamed);
    }
    _emitThreadDirectoryUpdate(renamed);
    return renamed;
  }

  Future<void> _completeDemoThreadTitle(
    String threadId,
    String provisionalTitle,
  ) async {
    await Future<void>.delayed(const Duration(milliseconds: 650));
    final index = _createdRootThreads.indexWhere(
      (thread) => thread.id == threadId,
    );
    if (index < 0 || _archivedThreadIds.contains(threadId)) return;
    final current = _createdRootThreads[index];
    if (current.title != provisionalTitle) return;
    final renamed = current.copyWith(
      title: 'Demo generated session',
      updatedAt: DateTime.now(),
    );
    _createdRootThreads[index] = renamed;
    if (_workspaces[threadId] case final workspace?) {
      _workspaces[threadId] = workspace.copyWith(thread: renamed);
    }
    _emitThreadDirectoryUpdate(renamed);
  }

  void _emitThreadDirectoryUpdate(StudioThread thread) {
    _productEvents.add(
      StudioBridgeEvent(
        payload: ThreadDirectoryChangedPayload(
          upserted: [thread],
          removed: const [],
        ),
      ),
    );
  }

  @override
  Future<ArchiveThreadResult> archiveThread(String threadId) async {
    final current = await readStudioState();
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
    final roots = _sortedDirectoryThreads()
        .where((candidate) => candidate.isRoot)
        .toList();
    final index = roots.indexWhere((candidate) => candidate.id == threadId);
    if (index < 0) throw StateError('unknown demo root Thread $threadId');
    final nextRoot = index + 1 < roots.length
        ? roots[index + 1]
        : index > 0
        ? roots[index - 1]
        : null;
    final removedThreadIds = <String>{
      threadId,
      for (final candidate in _ensureWorkspaceFixture().threads)
        if (candidate.effectiveRootThreadId == threadId) candidate.id,
    }.toList();
    _archivedThreadIds.add(threadId);
    if (_selectedThreadId == threadId) _selectedThreadId = nextRoot?.id;
    return ArchiveThreadResult(
      archivedRootId: threadId,
      removedThreadIds: removedThreadIds,
      nextRoot: nextRoot,
    );
  }

  @override
  Future<void> archiveProject(String projectId) async {
    if (projectId == 'project-local' && _archivedProjectIds.add(projectId)) {
      _projectDirectoryRevision += 1;
    }
  }

  @override
  Future<PersistenceStateSnapshot> retryPersistence() async =>
      const PersistenceStateSnapshot.ready();

  @override
  Future<SettingsStateSnapshot> setModelRole({
    required int expectedSettingsRevision,
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    _checkSettingsRevision(expectedSettingsRevision);
    final current = await readStudioState();
    final selected = current.providers
        .firstWhere((provider) => provider.id == providerId)
        .allModels
        .firstWhere((candidate) => candidate.slug == model);
    final selectedEffort = selected.reasoningEfforts.contains(effort)
        ? effort
        : selected.reasoningEfforts.firstOrNull;
    _roles = [
      for (final role in current.roles)
        role.key == roleKey
            ? RoleSettingsView(
                key: role.key,
                providerId: providerId,
                model: model,
                effort: selectedEffort ?? '',
              )
            : role,
    ];
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<void> setThreadMode({
    required String threadId,
    required ThreadModeId mode,
  }) async {
    final current = await readStudioState();
    final thread = current.threads
        .where((candidate) => candidate.id == threadId)
        .firstOrNull;
    if (thread == null) {
      throw StateError('unknown demo thread $threadId');
    }
    if (!thread.isRoot) {
      throw StateError('only a root Thread can change mode');
    }
    if (thread.status != ThreadStatusView.idle) {
      throw StateError(
        'thread mode cannot change while the Thread is running or has pending input',
      );
    }
    final workflow = current.workspacesByThread[threadId]?.runtime.workflow;
    if (workflow?.isActive ?? false) {
      throw StateError('thread mode cannot change while a workflow is active');
    }
    _threadModes[threadId] = mode;
    final workspace = _workspaces[threadId];
    if (workspace != null) {
      final thread = workspace.thread;
      _workspaces[threadId] = workspace.copyWith(
        thread: thread.copyWith(
          mode: mode,
          role: thread.isRoot ? 'planner' : thread.role,
        ),
      );
    }
  }

  @override
  Stream<Object> subscribeProductEvents() {
    if (lspActivityLoop) {
      _startLspActivityLoop();
    }
    return _productEvents.stream;
  }

  /// 周期发布 LSP 索引活动状态：40→55→70→85→100 后回到 idle，再循环。
  /// 每步递增 revision，保证 reducer 的 meta.isNewerThan 校验通过。
  void _startLspActivityLoop() {
    if (_lspActivityTimer != null) return;
    const cycle = [40, 55, 70, 85, 100, null];
    var index = 0;
    _lspActivityTimer = Timer.periodic(lspActivityStepDelay, (_) {
      _lspRevision += 1;
      final percentage = cycle[index];
      _productEvents.add(
        StudioBridgeEvent(
          payload: LspStateChangedPayload(
            percentage == null
                ? LspStateSnapshot.fromState(
                    state: _demoReadyResource(
                      _lspRevision,
                      const LspStateData(),
                    ),
                  )
                : _demoLspState(percentage: percentage),
          ),
        ),
      );
      index = (index + 1) % cycle.length;
    });
  }

  @override
  Stream<ThreadStreamFrame> subscribeThread(String threadId) async* {
    final snapshot = (await readStudioState()).workspacesByThread[threadId];
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
  Future<({ThreadWorkspace workspace, String? historyCursor})>
  readThreadSnapshot(String threadId) async {
    final workspace = (await readStudioState()).workspacesByThread[threadId];
    if (workspace == null) throw StateError('unknown demo thread $threadId');
    return (workspace: workspace, historyCursor: null);
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
    StudioPromptInput input,
  ) => _submitPrompt(threadId, input);

  @override
  Future<SubmitPromptReceipt> steerTurn(
    String threadId,
    StudioPromptInput input,
  ) => _submitPrompt(threadId, input);

  @override
  Future<List<AttachmentDraftView>> admitAttachmentDrafts(
    AttachmentAdmissionContext context,
    List<AttachmentDraftSource> sources,
  ) async {
    return [
      for (final source in sources)
        _demoAttachmentDraft(source, ++_attachmentSequence),
    ].map((draft) {
      _attachmentDrafts[draft.id] = draft;
      return draft;
    }).toList();
  }

  @override
  Future<bool> removeAttachmentDraft(String draftId) async =>
      _attachmentDrafts.remove(draftId) != null;

  @override
  Future<Uint8List> readAttachmentDraft(String draftId) async {
    if (!_attachmentDrafts.containsKey(draftId)) {
      throw StateError('unknown demo attachment draft');
    }
    return Uint8List(0);
  }

  @override
  Future<Uint8List> readThreadAttachment(
    String threadId,
    String attachmentId,
  ) async => Uint8List(0);

  Future<SubmitPromptReceipt> _submitPrompt(
    String threadId,
    StudioPromptInput input,
  ) async {
    final trimmed = input.text.trim();
    if (trimmed.isEmpty && input.attachmentDraftIds.isEmpty) {
      throw ArgumentError.value(input.text, 'text', 'empty');
    }
    final workspace = _workspaces[threadId];
    if (workspace == null) throw StateError('unknown demo thread $threadId');
    final settings = await readStudioState();
    final route = settings.roles
        .where((role) => role.key == workspace.thread.role)
        .firstOrNull;
    if (route != null) {
      _emitThreadUpdate(
        threadId,
        ThreadRuntimeUpdate(
          runtime: workspace.runtime.copyWith(model: route.model),
          todo: null,
        ),
      );
    }
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
          attachments: [
            for (final id in input.attachmentDraftIds)
              if (_attachmentDrafts[id] case final draft?)
                ThreadAttachmentView(
                  id: draft.id,
                  modality: draft.modality,
                  mediaType: draft.mediaType,
                  filename: draft.filename,
                  width: draft.width,
                  height: draft.height,
                  byteSize: draft.byteSize,
                ),
          ],
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadTurnUpdate(
        StudioTurnView(
          turnId: turnId,
          threadId: threadId,
          revision: 0,
          state: RunningStudioTurnState(
            startedAt: now.millisecondsSinceEpoch ~/ 1000,
            activity: StudioTurnActivity.thinking,
          ),
          updatedAt: now,
        ),
      ),
    );
    unawaited(
      _completePrompt(
        threadId: threadId,
        turnId: turnId,
        trimmedPrompt: trimmed,
        generation: generation,
        startedAt: now,
      ),
    );
    return receipt;
  }

  Future<void> _completePrompt({
    required String threadId,
    required String turnId,
    required String trimmedPrompt,
    required int generation,
    required DateTime startedAt,
  }) async {
    await Future<void>.delayed(promptStartDelay);
    if (_promptGenerations[threadId] != generation) return;
    final reasoningId = '$turnId:reasoning';
    const reasoningSummary =
        '## Inspecting the request\n\nChecking the live ThreadItem projection.';
    const reasoningContent =
        'The provider is folding summary and raw reasoning independently.';
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        ThreadItemView(
          id: reasoningId,
          threadId: threadId,
          turnId: turnId,
          ordinal: _nextOrdinal(threadId),
          revision: 0,
          createdAt: startedAt,
          updatedAt: startedAt,
          state: const ThreadThinkingItemStateView(
            summary: [],
            content: [],
            lifecycle: StreamingThreadContentView(),
          ),
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: reasoningId,
          revision: 1,
          state: const ThreadThinkingSummaryDeltaView(0, '## Inspecting'),
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: reasoningId,
          revision: 2,
          state: const ThreadThinkingSummaryDeltaView(
            0,
            ' the request\n\nChecking the live ThreadItem projection.',
          ),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: reasoningId,
          revision: 3,
          state: const ThreadThinkingContentDeltaView(0, reasoningContent),
        ),
      ),
    );
    final liveReasoning = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == reasoningId,
    );
    final reasoningCompletedAt = DateTime.now();
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        liveReasoning.copyWith(
          revision: 4,
          updatedAt: reasoningCompletedAt,
          state: ThreadThinkingItemStateView(
            summary: const [reasoningSummary],
            content: const [reasoningContent],
            lifecycle: CompletedThreadContentView(reasoningCompletedAt),
          ),
        ),
      ),
    );
    final commentaryId = '$turnId:commentary';
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        ThreadItemView(
          id: commentaryId,
          threadId: threadId,
          turnId: turnId,
          ordinal: _nextOrdinal(threadId),
          revision: 0,
          createdAt: startedAt,
          updatedAt: startedAt,
          state: const ThreadTextItemStateView(
            channel: ThreadTextChannel.commentary,
            text: '',
            attachments: [],
            lifecycle: StreamingThreadContentView(),
          ),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: commentaryId,
          revision: 1,
          state: const ThreadTextDeltaView('正在逐段核对 Timeline'),
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: commentaryId,
          revision: 2,
          state: const ThreadTextDeltaView(' 的 typed delta。'),
        ),
      ),
    );
    final commentaryCompletedAt = DateTime.now();
    final liveCommentary = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == commentaryId,
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        liveCommentary.copyWith(
          revision: 3,
          updatedAt: commentaryCompletedAt,
          state: ThreadTextItemStateView(
            channel: ThreadTextChannel.commentary,
            text: '正在逐段核对 Timeline 的 typed delta。',
            attachments: const [],
            lifecycle: CompletedThreadContentView(commentaryCompletedAt),
          ),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadTurnUpdate(
        StudioTurnView(
          turnId: turnId,
          threadId: threadId,
          revision: 1,
          state: RunningStudioTurnState(
            startedAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
            activity: StudioTurnActivity.runningTool,
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
          createdAt: startedAt,
          updatedAt: startedAt,
          state: ThreadToolItemStateView(
            invocation: ThreadToolInvocationView(
              toolCallId: '$turnId:tool-call',
              name: 'exec',
              arguments: '',
            ),
            lifecycle: const StartedThreadToolView(),
          ),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: toolId,
          revision: 1,
          state: const ThreadToolArgumentsDeltaView(
            '{"command":"flutter test ',
          ),
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: toolId,
          revision: 2,
          state: const ThreadToolArgumentsDeltaView('test/widget_test.dart"}'),
        ),
      ),
    );
    final streamedTool = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == toolId,
    );
    final streamedToolState = streamedTool.state as ThreadToolItemStateView;
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        streamedTool.copyWith(
          revision: 3,
          updatedAt: DateTime.now(),
          state: ThreadToolItemStateView(
            invocation: streamedToolState.invocation,
            lifecycle: const RunningThreadToolView(''),
          ),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: toolId,
          revision: 4,
          state: const ThreadToolResultDeltaView('running widget tests...\n'),
        ),
      ),
    );
    await Future<void>.delayed(promptToolDelay);
    if (_promptGenerations[threadId] != generation) return;
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: toolId,
          revision: 5,
          state: const ThreadToolResultDeltaView(
            '[stderr] analyzer warnings: 0\n',
          ),
        ),
      ),
    );
    final runningTool = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == toolId,
    );
    final runningToolState = runningTool.state as ThreadToolItemStateView;
    final toolCompletedAt = DateTime.now();
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        runningTool.copyWith(
          revision: 6,
          updatedAt: toolCompletedAt,
          state: ThreadToolItemStateView(
            invocation: runningToolState.invocation,
            lifecycle: SucceededThreadToolView(
              toolCompletedAt,
              const ThreadToolOutputView(
                result:
                    'running widget tests...\n'
                    '[stderr] analyzer warnings: 0\n'
                    'All widget tests passed.',
                attachments: [],
                outputArtifacts: [],
              ),
            ),
          ),
        ),
      ),
    );
    final finalId = '$turnId:final';
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        ThreadItemView(
          id: finalId,
          threadId: threadId,
          turnId: turnId,
          ordinal: _nextOrdinal(threadId),
          revision: 0,
          createdAt: startedAt,
          updatedAt: startedAt,
          state: const ThreadTextItemStateView(
            channel: ThreadTextChannel.finalAnswer,
            text: '',
            attachments: [],
            lifecycle: StreamingThreadContentView(),
          ),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: finalId,
          revision: 1,
          state: ThreadTextDeltaView('Demo response for: **$trimmedPrompt**'),
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    const finalSuffix = '\n\n- reasoning、tool output 与 Plan 都直接来自 typed delta';
    _emitThreadUpdate(
      threadId,
      ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: finalId,
          revision: 2,
          state: const ThreadTextDeltaView(finalSuffix),
        ),
      ),
    );
    final finalCompletedAt = DateTime.now();
    final liveFinal = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == finalId,
    );
    _emitThreadUpdate(
      threadId,
      ThreadItemUpsert(
        liveFinal.copyWith(
          revision: 3,
          updatedAt: finalCompletedAt,
          state: ThreadTextItemStateView(
            channel: ThreadTextChannel.finalAnswer,
            text: 'Demo response for: **$trimmedPrompt**$finalSuffix',
            attachments: const [],
            lifecycle: CompletedThreadContentView(finalCompletedAt),
          ),
        ),
      ),
    );
    _emitThreadUpdate(
      threadId,
      ThreadTurnUpdate(
        StudioTurnView(
          turnId: turnId,
          threadId: threadId,
          revision: 2,
          state: CompletedStudioTurnState(
            startedAt: null,
            completedAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
            completion: StudioTurnCompletion.normal,
          ),
          updatedAt: DateTime.now(),
        ),
      ),
    );
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
          revision: 2,
          state: CancelledStudioTurnState(
            startedAt: null,
            requestedAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
            completedAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
            cause: const UserRequestedTurnCancellation(),
          ),
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
          turnId: 'driver-origin-turn',
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
  Future<SettingsStateSnapshot> saveRuntimePermissionMode(
    int expectedSettingsRevision,
    PermissionMode mode,
  ) async {
    _checkSettingsRevision(expectedSettingsRevision);
    _permissionMode = mode;
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<SettingsStateSnapshot> saveProviderSettings(
    int expectedSettingsRevision,
    ProviderSettingsCommand command,
  ) async {
    _checkSettingsRevision(expectedSettingsRevision);
    final current = await readStudioState();
    _providers = _providersFromSettingsCommand(
      command,
      previous: current.providers,
      catalog: _providerCatalog,
    );
    _roles = _rolesFromSettingsCommand(command);
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<SettingsStateSnapshot> saveInstructionsSettings(
    int expectedSettingsRevision,
    InstructionsSettingsCommand command,
  ) async {
    _checkSettingsRevision(expectedSettingsRevision);
    _instructions = _instructionsFromSettingsCommand(command);
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<SettingsStateSnapshot> saveSkillsSettings(
    int expectedSettingsRevision,
    SkillsSettingsCommand command,
  ) async {
    _checkSettingsRevision(expectedSettingsRevision);
    _skills = _skillsFromSettingsCommand(command);
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<SettingsStateSnapshot> saveMcpSettings(
    int expectedSettingsRevision,
    McpSettingsCommand command,
  ) async {
    _checkSettingsRevision(expectedSettingsRevision);
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<SettingsStateSnapshot> saveGeneralSettings(
    int expectedSettingsRevision,
    GeneralSettingsCommand command,
  ) async {
    _checkSettingsRevision(expectedSettingsRevision);
    _general = GeneralSettingsView(
      followSystemTheme: command.followSystemTheme,
      followActiveTurn: command.followActiveTurn,
      compactTimeline: command.compactTimeline,
    );
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<SettingsStateSnapshot> saveWebSearchSettings(
    int expectedSettingsRevision,
    WebSearchSettingsCommand command,
  ) async {
    _checkSettingsRevision(expectedSettingsRevision);
    _webSearch = WebSearchSettingsView(
      configuredMode: command.mode,
      effectiveMode: command.mode,
      availability: command.mode == 'disabled' ? 'disabled' : 'available',
      selected: command.mode != 'disabled' && !_deepSeekWebSearch.selected,
      contextSize: command.contextSize,
      allowedDomains: command.allowedDomains,
      country: command.country,
      region: command.region,
      city: command.city,
      timezone: command.timezone,
      providerId: 'openai',
      model: 'gpt-5',
    );
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<SettingsStateSnapshot> saveDeepSeekWebSearchSettings(
    int expectedSettingsRevision,
    DeepSeekWebSearchSettingsCommand command,
  ) async {
    _checkSettingsRevision(expectedSettingsRevision);
    _deepSeekWebSearch = _deepSeekWebSearch.withConfiguredEnabled(
      command.enabled,
    );
    if (!command.enabled && _webSearch.isAvailable) {
      _webSearch = WebSearchSettingsView(
        configuredMode: _webSearch.configuredMode,
        effectiveMode: _webSearch.configuredMode,
        availability: _webSearch.availability,
        selected: true,
        contextSize: _webSearch.contextSize,
        allowedDomains: _webSearch.allowedDomains,
        country: _webSearch.country,
        region: _webSearch.region,
        city: _webSearch.city,
        timezone: _webSearch.timezone,
        providerId: _webSearch.providerId,
        model: _webSearch.model,
      );
    }
    _settingsRevision += 1;
    return (await readStudioState()).settingsState;
  }

  @override
  Future<ProviderUsageStateSnapshot> checkProviderUsage() async {
    _providerUsageRevision += 1;
    return (await readStudioState()).providerUsageState;
  }

  @override
  Future<SkillsStateSnapshot> readSkillsState(String projectId) async {
    return _archivedProjectIds.contains(projectId)
        ? _demoSkillsState(projectId, 0, skills: const [])
        : _demoSkillsState(projectId, _skillsRevision);
  }

  @override
  Future<SkillsStateSnapshot> discoverSkills(String projectId) async {
    _skillsRevision += 1;
    return readSkillsState(projectId);
  }

  @override
  Future<SkillSearchResultView> searchSkills(
    String projectId,
    String query, {
    int limit = 50,
  }) async {
    final snapshot = await readSkillsState(projectId);
    final normalizedQuery = query.trim().toLowerCase();
    final allMatches = snapshot.summaries
        .where(
          (skill) =>
              skill.name.toLowerCase().contains(normalizedQuery) ||
              skill.description.toLowerCase().contains(normalizedQuery),
        )
        .toList();
    return SkillSearchResultView(
      projectId: projectId,
      catalogRevision: snapshot.catalogRevision,
      matches: allMatches.take(limit).toList(),
      truncated: allMatches.length > limit,
    );
  }

  @override
  Future<McpStateSnapshot> readMcpState() async => _demoMcpState();

  @override
  Future<McpStateSnapshot> resetMcpServer(String serverId) async {
    _mcpRevision += 1;
    return _demoMcpState();
  }

  @override
  Future<McpStateSnapshot> resetAllMcp() async {
    _mcpRevision += 1;
    return _demoMcpState();
  }

  @override
  Future<LspStateSnapshot> readLspState() async => _demoLspState();

  @override
  Future<LspStateSnapshot> probeLspServer(String projectId) async {
    _lspRevision += 1;
    return _demoLspState();
  }

  @override
  Future<LspStateSnapshot> repairLspServer(
    String projectId,
    String serverId,
  ) async {
    _lspRevision += 1;
    return _demoLspState();
  }

  @override
  Future<LspStateSnapshot> resetLspServer(
    String projectId,
    String serverId,
  ) async {
    _lspRevision += 1;
    return _demoLspState();
  }

  @override
  Future<LspStateSnapshot> resetLspWorkspace(String projectId) async {
    _lspRevision += 1;
    return _demoLspState();
  }

  McpStateSnapshot _demoMcpState() {
    return McpStateSnapshot.fromState(
      state: _demoReadyResource(_mcpRevision, const McpStateData()),
    );
  }

  LspStateSnapshot _demoLspState({int percentage = 40}) {
    return LspStateSnapshot.fromState(
      state: _demoReadyResource(
        _lspRevision,
        LspStateData(
          activeServers: const ['rust-analyzer'],
          servers: [
            LspServerStateView(
              id: 'rust-analyzer',
              displayName: 'rust-analyzer',
              state: LspAvailableState(
                checkedAt: 0,
                diagnosticCount: 0,
                activity: LspIndexingActivity(
                  title: 'Roots Scanned',
                  message: '${(408 * percentage / 100).round()}/408',
                  percentage: percentage,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  void _checkSettingsRevision(int expected) {
    if (expected != _settingsRevision) {
      throw StateError(
        'settings revision conflict: expected $expected, actual $_settingsRevision',
      );
    }
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
  List<ThreadAttachmentView> attachments = const [],
}) {
  return ThreadItemView(
    id: id,
    threadId: threadId,
    turnId: turnId,
    ordinal: ordinal,
    revision: 0,
    createdAt: createdAt,
    updatedAt: createdAt,
    state: switch (kind) {
      ThreadItemKind.userMessage => ThreadTextItemStateView(
        channel: ThreadTextChannel.user,
        text: text,
        attachments: attachments,
        lifecycle: CompletedThreadContentView(createdAt),
      ),
      ThreadItemKind.parentAgentMessage => ThreadTextItemStateView(
        channel: ThreadTextChannel.parentAgent,
        text: text,
        attachments: attachments,
        lifecycle: CompletedThreadContentView(createdAt),
      ),
      ThreadItemKind.agentMessage => ThreadTextItemStateView(
        channel: channel == AgentMessageChannel.commentary
            ? ThreadTextChannel.commentary
            : ThreadTextChannel.finalAnswer,
        text: text,
        attachments: const [],
        lifecycle: CompletedThreadContentView(createdAt),
      ),
      ThreadItemKind.reasoning ||
      ThreadItemKind.toolCall ||
      ThreadItemKind.agent ||
      ThreadItemKind.turn ||
      ThreadItemKind.inference ||
      ThreadItemKind.skill ||
      ThreadItemKind.file ||
      ThreadItemKind.contextCompaction => throw ArgumentError.value(
        kind,
        'kind',
        'demo message helper only accepts text and plan items',
      ),
    },
  );
}

AttachmentDraftView _demoAttachmentDraft(
  AttachmentDraftSource source,
  int sequence,
) {
  final filename = switch (source) {
    LocalFileAttachmentDraftSource(:final path) =>
      path.replaceAll('\\', '/').split('/').last,
    RemoteUrlAttachmentDraftSource(:final url, :final filename) =>
      filename ?? Uri.parse(url).pathSegments.last,
  };
  final extension = filename.split('.').last.toLowerCase();
  final modality = switch (extension) {
    'png' || 'jpg' || 'jpeg' || 'webp' => AttachmentModalityView.image,
    'mp4' || 'mov' || 'webm' || 'mkv' => AttachmentModalityView.video,
    _ => AttachmentModalityView.file,
  };
  return AttachmentDraftView(
    id: 'demo-attachment-$sequence',
    modality: modality,
    mediaType: switch (modality) {
      AttachmentModalityView.image => 'image/png',
      AttachmentModalityView.video => 'video/mp4',
      AttachmentModalityView.file => 'application/octet-stream',
    },
    filename: filename,
    byteSize: 0,
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
    final nextItem = items[index].appendDelta(
      delta: delta.state,
      nextRevision: delta.revision,
    );
    if (nextItem != null) items[index] = nextItem;
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
  DriverDemoStudioApi({super.lspActivityLoop});

  bool _sessionLifecycleScenario = false;
  bool _persistenceFailureScenario = false;
  bool _failNextTurn = false;

  void preparePersistenceFailureScenario() {
    prepareSessionLifecycleScenario();
    _persistenceFailureScenario = true;
    _failNextTurn = true;
    _selectedThreadId = 'thread-main';
  }

  @override
  Future<void> _completePrompt({
    required String threadId,
    required String turnId,
    required String trimmedPrompt,
    required int generation,
    required DateTime startedAt,
  }) async {
    if (!_failNextTurn) {
      return super._completePrompt(
        threadId: threadId,
        turnId: turnId,
        trimmedPrompt: trimmedPrompt,
        generation: generation,
        startedAt: startedAt,
      );
    }
    _failNextTurn = false;
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    _emitThreadUpdate(
      threadId,
      ThreadTurnUpdate(
        StudioTurnView(
          turnId: turnId,
          threadId: threadId,
          revision: 1,
          updatedAt: DateTime.now(),
          state: FailedStudioTurnState(
            startedAt: startedAt.millisecondsSinceEpoch ~/ 1000,
            completedAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
            failure: const StudioTurnFailureView(
              category: 'protocol',
              providerKind: null,
              code: 'turnProtocolProjectionFailed',
              httpStatus: null,
              message: 'Injected turn protocol failure',
              retryable: false,
              retryAfterMs: null,
            ),
          ),
        ),
      ),
    );
  }

  @override
  Future<PersistenceStateSnapshot> retryPersistence() async {
    _persistenceFailureScenario = false;
    return const PersistenceStateSnapshot(
      revision: 2,
      state: ReadyPersistenceState(pendingCommits: 0),
    );
  }

  bool _fallbackInputScenario = false;
  int _pagingFixtureCount = 42;

  void preparePagingScenario(int count) {
    _sessionLifecycleScenario = false;
    _fallbackInputScenario = false;
    _pagingFixtureCount = count;
    _pageFillThreads.clear();
    _selectedThreadId = null;
  }

  void prepareSessionLifecycleScenario() {
    _sessionLifecycleScenario = true;
    _fallbackInputScenario = false;
    _pageFillThreads.clear();
    _selectedThreadId = null;
  }

  void prepareFallbackInputScenario() {
    _sessionLifecycleScenario = false;
    _fallbackInputScenario = true;
    _pageFillThreads.clear();
    _selectedThreadId = 'thread-main';
  }

  @override
  Duration get promptActivityDelay => const Duration(seconds: 3);

  @override
  Duration get promptToolDelay => const Duration(seconds: 3);

  @override
  Duration get shutdownPhaseDelay => const Duration(milliseconds: 400);

  @override
  Duration get lspActivityStepDelay => const Duration(seconds: 3);

  /// Driver 目录分页验收需要大量历史会话。
  @override
  int get directoryPageFillCount =>
      _sessionLifecycleScenario ? 0 : _pagingFixtureCount;

  @override
  Future<StudioState> readStudioState() async {
    final state = await super.readStudioState();
    if (_persistenceFailureScenario) {
      return state.copyWith(
        persistenceState: const PersistenceStateSnapshot(
          revision: 1,
          state: DegradedPersistenceState(
            pendingCommits: 3,
            oldestPendingRevision: 8,
            firstFailedAt: 1,
            error: ObservedResourceError(
              code: 'storageUnavailable',
              message: 'Injected save failure',
              retryable: true,
            ),
          ),
        ),
      );
    }
    if (_sessionLifecycleScenario) return state;
    const threadId = 'thread-main';
    final workspace = state.workspacesByThread[threadId];
    if (workspace == null) return state;
    StudioDriverState.publishTurn(
      StudioTurnView(
        turnId: 'driver-origin-turn',
        threadId: threadId,
        revision: 1,
        state: CompletedStudioTurnState(
          startedAt: null,
          completedAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
          completion: StudioTurnCompletion.normal,
        ),
        updatedAt: DateTime.now(),
      ),
    );
    return state.copyWith(
      workspacesByThread: {
        ...state.workspacesByThread,
        threadId: workspace.copyWith(
          interactions: _fallbackInputScenario
              ? const [
                  PendingInteraction(
                    id: 'driver-fallback-input',
                    threadId: threadId,
                    turnId: 'driver-origin-turn',
                    kind: InteractionKind.userInput,
                    title: 'Continue task',
                    body: '请输入“继续”以创建新的任务轮次。',
                    payload: UserInputInteractionPayload(questions: []),
                  ),
                ]
              : const [
                  PendingInteraction(
                    id: 'driver-tool',
                    threadId: threadId,
                    turnId: 'driver-origin-turn',
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
                    turnId: 'driver-origin-turn',
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
                ],
        ),
      },
    );
  }
}

String _demoProvisionalThreadTitle(String prompt) {
  final normalized = prompt.trim().replaceAll(RegExp(r'\s+'), ' ');
  if (normalized.isEmpty) return 'New Session';
  return normalized.runes.take(80).map(String.fromCharCode).join();
}

ObservedResource<T> _demoReadyResource<T>(int revision, T value) {
  return ReadyObservedResource(
    revision: revision,
    updatedAt: revision,
    lastCheckedAt: null,
    value: value,
  );
}

ObservedResource<T> _demoInitialResource<T>() =>
    UninitializedObservedResource(updatedAt: 0);

SkillsStateSnapshot _demoSkillsState(
  String projectId,
  int revision, {
  List<String> skills = const [
    'flutter-ui-polish',
    'runtime-review',
    'studio-settings',
  ],
}) {
  return SkillsStateSnapshot.fromState(
    projectId: projectId,
    state: _demoReadyResource(
      revision,
      SkillsStateData(
        configFingerprint: 'demo',
        catalogRevision: revision,
        skills: skills,
        summaries: [
          for (final skill in skills)
            SkillSummaryView(
              name: skill,
              description: switch (skill) {
                'flutter-ui-polish' =>
                  'Polish Flutter layouts, interactions, and visual details.',
                'runtime-review' =>
                  'Review runtime behavior, lifecycle, and concurrency.',
                'studio-settings' =>
                  'Manage Studio providers, skills, and application settings.',
                _ => 'Installed Studio skill.',
              },
              source: 'demo',
              providerId: 'demo',
              modelInvocable: true,
              userInvocable: true,
              resourceBase: const SkillResourceBaseView(
                SkillResourceBaseKind.opaque,
                'demo',
              ),
            ),
        ],
        warnings: const [],
      ),
    ),
  );
}
