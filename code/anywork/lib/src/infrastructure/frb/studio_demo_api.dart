part of 'studio_api.dart';

class DemoStudioTransport extends StudioTransport
    implements PersistenceQueueReader, ChatWindowReader {
  @override
  Future<RecoveryStateSnapshot> retryRecovery() async => _demoRecoveryState();

  DemoStudioTransport({
    this.lspActivityLoop = false,
    this._providerCatalog = demoProviderCatalogFixture,
  });

  /// 目录分页窗口的页大小；Driver 模式填充大量历史会话以验收触底加载。
  static const int directoryPageSize = 20;

  final ProviderCatalogView _providerCatalog;

  /// 是否周期推进 demo LSP 索引活动；GUI demo 构建开启，测试默认关闭保持确定性。
  final bool lspActivityLoop;

  /// 每 topic 的 typed fanout 接收者：receiver 先登记其 sink，再取得该 topic 基线；
  /// 无关领域不进入该 receiver 的缓冲。发布只走已登记 sink，不做全局 mixed bus。
  final Map<ProductTopic, Set<void Function(ProductTopicEventPayload)>>
  _topicSinks = {};
  final String _ownerEpoch = 'demo:${newStudioCorrelationId()}';
  int _productEventSequence = 0;
  int _threadDirectoryRevision = 0;
  int _agentProfilesRevision = 0;
  final _threadEvents = StreamController<ThreadStreamFrame>.broadcast();
  final _shutdownEvents = StreamController<StudioShutdownProgress>.broadcast();

  /// 内容窗口的变更信号：条目变化时唤醒在途的 `next()`，正文只经窗口交付。
  final Map<String, int> _chatChangeSeq = {};
  final Map<String, List<Completer<void>>> _chatWaiters = {};
  Timer? _lspActivityTimer;
  final Map<String, ThreadWorkspace> _workspaces = {};
  final Map<String, int> _promptGenerations = {};
  final Map<String, ThreadModeId> _threadModes = {};
  final Map<String, AttachmentDraftView> _attachmentDrafts = {};
  final Set<String> _archivedProjectIds = {};
  final Map<String, String> _projectNames = {};
  final Set<String> _archivedThreadIds = {};
  final Map<String, ({PendingInteraction interaction, Object resolution})>
  _resolvedInteractions = {};
  final List<StudioThread> _createdRootThreads = [];
  final List<StudioThread> _pageFillThreads = [];
  final List<SshServer> _sshServers = [
    const SshServer(
      alias: 'arm-dev',
      hostName: '192.168.100.12',
      port: 22,
      username: 'root',
      managed: true,
    ),
    const SshServer(
      alias: 'handwritten',
      hostName: '203.0.113.10',
      port: 2222,
      username: 'deploy',
      managed: false,
    ),
  ];

  /// Driver 模式注入的历史会话数量；普通 demo 为 0。
  int get directoryPageFillCount => 0;

  List<ProviderSettingsView>? _providers;
  String? _defaultProviderId;
  List<ModeModelRouteView>? _modeRoutes;
  List<RoleSettingsView>? _roles;
  InstructionsSettingsView _instructions = const InstructionsSettingsView();
  SkillsSettingsView _skills = const SkillsSettingsView();
  GeneralSettingsView _general = const GeneralSettingsView();
  WebSearchSettingsView _webSearch = const WebSearchSettingsView(
    effectiveMode: 'disabled',
    availability: 'available',
    providerId: 'openai',
    model: 'gpt-5',
  );
  DeepSeekWebSearchSettingsView _deepSeekWebSearch =
      const DeepSeekWebSearchSettingsView(
        effectiveEnabled: true,
        configuredFetchEnabled: true,
        effectiveFetchEnabled: true,
        availability: 'available',
        providerId: 'deepseek-primary',
      );
  PermissionMode _permissionMode = PermissionMode.requestApproval;
  int _turnSequence = 0;
  int _threadSequence = 0;
  int _settingsRevision = 1;
  int _attachmentSequence = 0;
  int _skillsRevision = 1;
  int _providerUsageRevision = 1;
  final Map<String, ProviderUsageView> _providerUsages = {};
  int _mcpRevision = 0;
  int _lspRevision = 0;
  int? _lspActivityPercentage = 40;
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
  Future<void> beginRuntimeExit({required int remainingMs}) async {
    // demo 无 native runtime owner：与 public StudioTransport 契约一致，早期封闭是无副作用的
    // no-op。真正的早期封闭由 FrbStudioTransport 调 native `begin_runtime_exit`；这里不声称任何
    // raw demo 结果验证过 Rust。
  }

  @override
  Future<StudioShutdownReport> shutdownRuntime({
    required int remainingMs,
    List<StudioShutdownIssue> externalIssues = const [],
  }) async {
    for (final phase in StudioShutdownPhase.values) {
      if (shutdownPhaseDelay > Duration.zero) {
        await Future<void>.delayed(shutdownPhaseDelay);
      }
      if (phase == StudioShutdownPhase.flushingPersistence) {
        _emitShutdownProgress(
          const FlushingPersistenceProgress(pendingCommits: 3),
        );
        if (shutdownPhaseDelay > Duration.zero) {
          await Future<void>.delayed(shutdownPhaseDelay);
        }
        _emitShutdownProgress(
          const FlushingPersistenceProgress(pendingCommits: 0),
        );
        continue;
      }
      _emitShutdownProgress(_demoShutdownProgress(phase));
    }
    await _shutdownEvents.close();
    // Demo 的确定性结局：所有阶段成功且持久化已排空。外部（Dart）取消 issue 是本次关闭
    // 的最终成功前置条件，只要存在就如实为 Degraded，绝不伪 Clean。
    if (externalIssues.isEmpty) {
      return const StudioShutdownReport(
        outcome: StudioShutdownOutcome.clean,
        issues: [],
        persistence: DrainedStudioPendingPersistence(),
      );
    }
    return StudioShutdownReport(
      outcome: StudioShutdownOutcome.degraded,
      issues: externalIssues,
      persistence: const DrainedStudioPendingPersistence(),
    );
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
  Future<StudioBootstrap> readBootstrap() async {
    // Bootstrap only assembles independently versioned owner snapshots. Normal
    // commands and topic baselines query their own domain directly.
    final projects = _demoProjectDirectory();
    final threads = _demoThreadDirectory();
    final selectedProjectId =
        projects.values
            .where((project) => project.id == _selectedProjectId)
            .firstOrNull
            ?.id ??
        projects.values.firstOrNull?.id;
    final selectedThreadId = _sortedDirectoryThreads()
        .where(
          (thread) =>
              thread.id == _selectedThreadId &&
              thread.projectId == selectedProjectId,
        )
        .firstOrNull
        ?.id;
    return StudioBootstrap(
      ownerEpoch: _ownerEpoch,
      projectDirectory: projects,
      threadDirectory: threads,
      agentDirectory: _demoAgentDirectory(),
      settingsState: _demoSettingsState(),
      recoveryState: _demoRecoveryState(),
      mcpState: _demoMcpState(),
      lspState: _demoLspState(),
      skillsByProject: {
        for (final project in projects.values)
          project.id: _demoSkillsState(project.id, _skillsRevision),
      },
      threadModeCatalog: _demoThreadModeCatalog(),
      providerUsageState: _demoProviderUsageState(),
      modelPerformance: _demoModelPerformance(_fixtureTime),
      updaterState: _demoUpdaterState(),
      persistenceState: _demoPersistenceState(),
      persistenceQueueState: _demoPersistenceQueueState(),
      agentProfilesState: readAgentProfilesStateSync(),
      sessionCostsByRoot: {
        for (final root in _sortedDirectoryThreads().where(
          (thread) => thread.isRoot,
        ))
          root.id: _demoSessionCosts(root.id),
      },
      selectedProjectId: selectedProjectId,
      selectedThreadId: selectedThreadId,
    );
  }

  DateTime get _fixtureTime => _fixtureNow ??= DateTime.now();

  ThreadModeCatalogView _demoThreadModeCatalog() => const ThreadModeCatalogView(
    revision: 1,
    modes: [
      ThreadModeDescriptorView(
        id: 'mode.simple',
        displayName: '简洁',
        description: '直接对话',
        order: 0,
        hasWorkflow: false,
      ),
      ThreadModeDescriptorView(
        id: 'mode.task',
        displayName: '任务',
        description: '通过工作流完成任务',
        order: 1,
        hasWorkflow: true,
      ),
    ],
  );

  ProjectDirectoryState _demoProjectDirectory() {
    final fixture = _ensureWorkspaceFixture();
    final projects = [fixture.project, ..._openedRemoteProjects]
        .where((project) => !_archivedProjectIds.contains(project.id))
        .map(
          (project) => StudioProject(
            id: project.id,
            name: _projectNames[project.id] ?? project.name,
            path: project.path,
            sshAlias: project.sshAlias,
          ),
        )
        .toList();
    return ProjectDirectoryState.fromState(
      state: _demoReadyResource(_projectDirectoryRevision, projects),
    );
  }

  ThreadDirectoryWindow _demoThreadDirectory() {
    final directory = _sortedDirectoryThreads();
    return ThreadDirectoryWindow(
      threads: directory.take(directoryPageSize).toList(),
      nextCursor: directory.length > directoryPageSize
          ? _demoDirectoryCursor(directoryPageSize - 1)
          : null,
      hasMore: directory.length > directoryPageSize,
      revision: _threadDirectoryRevision,
    );
  }

  ThreadDirectoryPage _demoDirectoryPage() {
    final window = _demoThreadDirectory();
    return ThreadDirectoryPage(
      threads: window.threads,
      nextCursor: window.nextCursor,
      revision: window.revision,
    );
  }

  AgentDirectoryState _demoAgentDirectory() =>
      AgentDirectoryState.fromState(state: _demoInitialResource());

  RecoveryStateSnapshot _demoRecoveryState() =>
      RecoveryStateSnapshot.fromState(state: _demoInitialResource());

  ProviderSettingsView _demoDefaultProvider() {
    final preset = _providerCatalog.presets.first;
    return preset
        .createProvider(
          preset.id,
          _providerCatalog.modelsFor(preset.modelCatalogId),
        )
        .copyWith(hasBearerToken: true, status: 'ready', updatedAt: 'Loaded');
  }

  SettingsStateSnapshot _demoSettingsState() {
    final defaultProvider = _demoDefaultProvider();
    final defaultModel = defaultProvider.models
        .where((model) => model.slug == defaultProvider.defaultModel)
        .firstOrNull;
    final effort = defaultModel?.defaultReasoningEffort;
    final resolvedEffort = effort?.isNotEmpty == true
        ? effort!
        : defaultModel?.reasoningEfforts.firstOrNull ?? '';
    return _settingsSnapshot(
      providers: _providers ?? [defaultProvider],
      modeRoutes:
          _modeRoutes ??
          [
            for (final mode in [ThreadModeId.simple, ThreadModeId.task])
              ModeModelRouteView(
                modeId: mode,
                providerId: defaultProvider.id,
                model: defaultProvider.defaultModel,
                effort: resolvedEffort,
              ),
          ],
      roles:
          _roles ??
          [
            for (final role in const [
              'explorer',
              'executor',
              'worktree_executor',
              'reviewer',
            ])
              RoleSettingsView(
                key: role,
                providerId: defaultProvider.id,
                model: defaultProvider.defaultModel,
                effort: resolvedEffort,
              ),
          ],
    );
  }

  ProviderUsageStateSnapshot _demoProviderUsageState() {
    final providers = _providers ?? [_demoDefaultProvider()];
    final usages = [
      for (final provider in providers)
        _providerUsages[provider.id] ?? _demoProviderUsage(provider),
    ];
    return ProviderUsageStateSnapshot.fromState(
      state: _demoReadyResource(
        _providerUsageRevision,
        ProviderUsageStateData(configFingerprint: 'demo', usages: usages),
      ),
    );
  }

  SessionCostsStateView _demoSessionCosts(String rootThreadId) =>
      SessionCostsStateView(
        rootThreadId: rootThreadId,
        revision: rootThreadId == 'thread-main' || rootThreadId == 'thread-alt'
            ? 1
            : 0,
        updatedAt: _fixtureTime,
        cost: switch (rootThreadId) {
          'thread-main' => const SessionCostView(
            rootThreadId: 'thread-main',
            estimatedCosts: [
              RuntimeCostView(currency: 'CNY', amount: 0.14),
              RuntimeCostView(currency: 'USD', amount: 0.02),
            ],
            hasUnpricedUsage: false,
          ),
          'thread-alt' => const SessionCostView(
            rootThreadId: 'thread-alt',
            estimatedCosts: [],
            hasUnpricedUsage: true,
          ),
          _ => null,
        },
      );

  UpdaterStateSnapshot _demoUpdaterState() => UpdaterStateSnapshot.idle(
    revision: 0,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );

  PersistenceStateSnapshot _demoPersistenceState() =>
      const PersistenceStateSnapshot.ready();

  PersistenceQueueStateView _demoPersistenceQueueState() =>
      const PersistenceQueueStateView(
        revision: 0,
        queue: PersistenceQueueSnapshot.empty(),
      );

  ThreadWorkspace _demoWorkspace(String threadId) {
    _ensureWorkspaceFixture();
    final workspace = _workspaces[threadId];
    if (workspace == null) throw StateError('unknown demo thread $threadId');
    return workspace;
  }

  SettingsStateSnapshot _settingsSnapshot({
    required List<ProviderSettingsView> providers,
    required List<ModeModelRouteView> modeRoutes,
    required List<RoleSettingsView> roles,
  }) {
    return SettingsStateSnapshot.fromState(
      state: _demoReadyResource(
        _settingsRevision,
        SettingsStateData(
          providers: providers,
          defaultProviderId: _defaultProviderId ?? providers.firstOrNull?.id,
          modeModelRoutes: modeRoutes,
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

  ModeModelRouteView _demoModeRoute(ThreadModeId mode) {
    final saved =
        _modeRoutes?.where((route) => route.modeId == mode).firstOrNull ??
        _modeRoutes
            ?.where((route) => route.modeId == ThreadModeId.simple)
            .firstOrNull;
    if (saved != null) return saved;
    final preset = _providerCatalog.presets.first;
    final models = _providerCatalog.modelsFor(preset.modelCatalogId);
    final model =
        models
            .where((candidate) => candidate.slug == preset.suggestedModel)
            .firstOrNull ??
        models.first;
    return ModeModelRouteView(
      modeId: mode,
      providerId: preset.id,
      model: model.slug,
      effort: model.defaultReasoningEffort.isNotEmpty
          ? model.defaultReasoningEffort
          : model.reasoningEfforts.firstOrNull ?? '',
    );
  }

  ModelPerformanceSnapshotView _demoModelPerformance(DateTime now) {
    const deepseekHigh = ModelPerformanceSummaryView(
      providerInstanceId: 'deepseek-primary',
      providerDisplayName: 'DeepSeek',
      model: 'deepseek-flash',
      reasoningEffort: 'high',
      sampleCount: 1,
      completionTokens: 150,
      totalTtftMillis: 410,
      totalDecodeMillis: 1000,
      totalResponseMillis: 1410,
      tokensPerSecond: 150,
      averageTtftMillis: 410,
      averageResponseMillis: 1410,
    );
    const deepseekNone = ModelPerformanceSummaryView(
      providerInstanceId: 'deepseek-primary',
      providerDisplayName: 'DeepSeek',
      model: 'deepseek-flash',
      reasoningEffort: 'none',
      sampleCount: 1,
      completionTokens: 150,
      totalTtftMillis: 350,
      totalDecodeMillis: 1000,
      totalResponseMillis: 1350,
      tokensPerSecond: 150,
      averageTtftMillis: 350,
      averageResponseMillis: 1350,
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
      summaries: const [deepseekHigh, deepseekNone, openai],
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
          providerInstanceId: deepseekHigh.providerInstanceId,
          providerDisplayName: deepseekHigh.providerDisplayName,
          model: deepseekHigh.model,
          reasoningEffort: deepseekHigh.reasoningEffort,
          completionTokens: 150,
          ttftMillis: 410,
          decodeMillis: 1000,
          totalResponseMillis: 1410,
          tokensPerSecond: 150,
        ),
        ModelPerformanceSampleView(
          completedAt: now.subtract(const Duration(minutes: 2)),
          providerInstanceId: deepseekNone.providerInstanceId,
          providerDisplayName: deepseekNone.providerDisplayName,
          model: deepseekNone.model,
          reasoningEffort: deepseekNone.reasoningEffort,
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
      lastUserMessageAt: now,
      agentPath: 'root',
      workspacePath: project.path,
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
      workspacePath: project.path,
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
      workspacePath: project.path,
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
    _ensurePageFillThreads(project.id, project.path, now);
    return (project: project, threads: threads);
  }

  /// Driver 模式的历史会话填充：保证触底分页有足够数据。
  void _ensurePageFillThreads(
    String projectId,
    String projectPath,
    DateTime now,
  ) {
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
        workspacePath: projectPath,
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

  /// 目录完整排序视图（最近用户消息时间倒序、id 倒序），分页窗口的模拟数据源。
  List<StudioThread> _sortedDirectoryThreads() {
    final fixture = _ensureWorkspaceFixture();
    final threads =
        [
              ...fixture.threads,
              ..._pageFillThreads.where(
                (thread) => !_archivedThreadIds.contains(thread.id),
              ),
            ]
            .where((thread) => !_archivedProjectIds.contains(thread.projectId))
            .toList()
          ..sort(StudioThread.compareDirectoryOrder);
    return threads;
  }

  @override
  Future<ThreadDirectoryPage> queryThreads(
    DirectoryQuery query, {
    String? cursor,
    int limit = 50,
  }) async {
    final projects = _demoProjectDirectory().values;
    final all = <String, StudioThread>{
      for (final thread in [
        ..._workspaces.values.map((w) => w.thread),
        ..._createdRootThreads,
        ..._pageFillThreads,
      ])
        thread.id: thread.copyWith(
          archived: _archivedThreadIds.contains(thread.effectiveRootThreadId),
        ),
    };
    final rows =
        all.values.where((thread) => query.matches(thread, projects)).toList()
          ..sort(StudioThread.compareDirectoryOrder);
    final offset = cursor == null ? 0 : int.parse(cursor);
    return ThreadDirectoryPage(
      threads: rows.skip(offset).take(limit).toList(),
      nextCursor: offset + limit < rows.length ? '${offset + limit}' : null,
      revision: _threadDirectoryRevision,
    );
  }

  @override
  Future<StudioThread> restoreThread(String threadId) async {
    _archivedThreadIds.remove(threadId);
    final restored = _sortedDirectoryThreads().firstWhere(
      (thread) => thread.id == threadId,
    );
    _emitThreadDirectoryUpdate(restored);
    return restored;
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
      revision: _threadDirectoryRevision,
    );
  }

  ThreadWorkspace _rootWorkspace(StudioThread thread, DateTime now) {
    final route = _demoModeRoute(thread.mode);
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
              '用 Flutter 重构 anywork。\n\n'
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
      runtime: ThreadRuntimeView(
        model: 'planner/local-responses',
        modelRoute: ThreadModelRouteView(
          providerId: route.providerId,
          model: route.model,
          effort: route.effort,
          revision: 1,
          available: true,
        ),
        contextTokens: 18342,
        contextWindow: 128000,
        totalTokens: 26320,
        promptTokens: 22160,
        completionTokens: 4160,
        cachedPromptTokens: 8860,
        cacheWriteTokens: 1240,
        reasoningTokens: 1520,
        inferenceCount: 6,
        cacheUsage: CacheUsageView(
          inputTokens: 22160,
          cacheReadTokens: 8860,
          hitRate: 0.4,
        ),
        estimatedCosts: [
          RuntimeCostView(currency: 'CNY', amount: 0.13),
          RuntimeCostView(currency: 'USD', amount: 0.02),
        ],
        estimatedCacheSavings: [RuntimeCostView(currency: 'CNY', amount: 0.05)],
        costLabel: '￥0.13 + \$0.02',
        activeSkills: ['flutter-apply-architecture-best-practices'],
        activeMcpServers: ['dart'],
        activeLspServers: ['rust-analyzer'],
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
        modelRoute: ThreadModelRouteView(
          providerId: _demoModeRoute(thread.mode).providerId,
          model: model,
          effort: null,
          revision: 1,
          available: true,
        ),
        contextTokens: 320,
        contextWindow: 128000,
        totalTokens: 512,
        estimatedCosts: const [RuntimeCostView(currency: 'CNY', amount: 0.01)],
        costLabel: '￥0.01',
        activeSkills: const [],
        activeMcpServers: const ['dart'],
        activeLspServers: const [],
        turnCompletionTokens: 216,
        turnDecodeMillis: 2500,
      ),
    );
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async => _providerCatalog;

  @override
  Future<AgentProfilesStateView> readAgentProfilesState() async =>
      readAgentProfilesStateSync();

  @override
  Future<List<AgentProfileView>> readAgentProfiles() async =>
      _demoAgentProfilesSync();

  @override
  Future<SettingsCommandReply> setSystemAgentEnabled({
    required OperationId operationId,
    required int expectedSettingsRevision,
    required String profileId,
    required bool enabled,
  }) async {
    if (![
      'explorer',
      'executor',
      'worktree_executor',
      'reviewer',
    ].contains(profileId)) {
      throw ArgumentError.value(
        profileId,
        'profileId',
        'not a system subagent',
      );
    }
    final before = _demoSettingsState();
    _checkSettingsRevision(expectedSettingsRevision);
    final changed = _disabledSystemAgents.contains(profileId) == enabled;
    if (enabled) {
      _disabledSystemAgents.remove(profileId);
    } else {
      _disabledSystemAgents.add(profileId);
    }
    _settingsRevision += 1;
    _emitAgentProfilesUpdate();
    return _demoSettingsReply(operationId, before, changed: changed);
  }

  @override
  Future<SettingsCommandReply> saveUserAgentProfile(
    int expectedSettingsRevision,
    AgentProfileDraft draft, {
    required OperationId operationId,
  }) async {
    if ([
      'planner',
      'explorer',
      'executor',
      'worktree_executor',
      'reviewer',
    ].contains(draft.id)) {
      throw ArgumentError.value(draft.id, 'id', 'reserved agent identifier');
    }
    final before = _demoSettingsState();
    _checkSettingsRevision(expectedSettingsRevision);
    final previous = _userAgentProfiles[draft.id];
    _userAgentProfiles[draft.id] = AgentProfileView(
      id: draft.id,
      displayName: draft.displayName,
      description: draft.description,
      whenToUse: draft.whenToUse,
      systemInstructions: draft.systemInstructions,
      providerId: draft.providerId,
      model: draft.model,
      effort: draft.effort,
      source: '~/.anywork/agents/${draft.id}.toml',
      revision: 'demo-user-${draft.id}',
      contentHash: 'demo-user-${draft.id}',
      system: false,
      enabled: draft.enabled,
      workspaceMode: draft.workspaceMode,
    );
    _settingsRevision += 1;
    _emitAgentProfilesUpdate();
    return _demoSettingsReply(
      operationId,
      before,
      changed: previous != _userAgentProfiles[draft.id],
    );
  }

  @override
  Future<void> cleanupPreservedWorktree({
    required String ownerKind,
    required String ownerThreadId,
    required int expectedLeaseRevision,
  }) async {
    if (ownerKind != WorktreeOwnerKind.session.id &&
        ownerKind != WorktreeOwnerKind.child.id) {
      throw ArgumentError.value(
        ownerKind,
        'ownerKind',
        'must be session or child',
      );
    }
  }

  @override
  Future<StudioProject> renameProject(String projectId, String name) async {
    final project = _demoProjectDirectory().values.firstWhere(
      (project) => project.id == projectId,
    );
    _projectNames[projectId] = name;
    _projectDirectoryRevision++;
    _publishProductPayload(
      ProjectDirectoryChangedPayload(_demoProjectDirectory()),
    );
    return StudioProject(
      id: project.id,
      name: name,
      path: project.path,
      sshAlias: project.sshAlias,
    );
  }

  @override
  Future<StudioProject> openProject(String path) async {
    if (_archivedProjectIds.remove('project-local')) {
      _projectDirectoryRevision += 1;
      _publishProductPayload(
        ProjectDirectoryChangedPayload(_demoProjectDirectory()),
      );
    }
    return _demoProjectDirectory().values.first;
  }

  @override
  Future<List<SshServer>> listSshServers() async => List.of(_sshServers);

  @override
  Future<SshServer> saveSshServer(SaveSshServerCommand command) async {
    final existing = _sshServers
        .where((item) => item.alias == command.alias)
        .firstOrNull;
    final hasPassword = switch (command.password.action) {
      SshPasswordAction.preserve => existing?.hasPassword ?? false,
      SshPasswordAction.replace => true,
      SshPasswordAction.clear => false,
    };
    final server = SshServer(
      alias: command.alias,
      hostName: command.hostName,
      port: command.port,
      username: command.username,
      identityFile: command.identityFile,
      managed: true,
      hasPassword: hasPassword,
    );
    _sshServers.removeWhere((item) => item.alias == server.alias);
    _sshServers.add(server);
    return server;
  }

  @override
  Future<void> deleteSshServer(String alias) async {
    _sshServers.removeWhere((server) => server.alias == alias);
  }

  @override
  Future<SshConnectionView> authenticateSshServer(
    AuthenticateSshServerCommand command,
  ) async {
    final index = _sshServers.indexWhere((item) => item.alias == command.alias);
    if (index != -1 && command.rememberPassword) {
      final current = _sshServers[index];
      _sshServers[index] = SshServer(
        alias: current.alias,
        hostName: current.hostName,
        port: current.port,
        username: current.username,
        identityFile: current.identityFile,
        managed: current.managed,
        hasPassword: true,
      );
    }
    return testSshConnection(command.alias);
  }

  @override
  Future<SshConnectionView> testSshConnection(String alias) async {
    return SshConnectionView(
      alias: alias,
      state: 'ready',
      helperVersion: '0.1.0',
      architecture: 'aarch64',
    );
  }

  @override
  Future<SshConnectionView> reconnectSshServer(String alias) =>
      testSshConnection(alias);

  @override
  Future<RemoteDirectoryListing> browseRemoteDirectories(
    String alias, {
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
  Future<StudioProject> openRemoteProject(String alias, String path) async {
    final project = StudioProject(
      id: 'project-remote',
      name: path.split('/').last,
      path: path,
      sshAlias: alias,
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
          existing.sshAlias != project.sshAlias) {
        _openedRemoteProjects[existingIndex] = project;
        _projectDirectoryRevision += 1;
      }
    }
    _publishProductPayload(
      ProjectDirectoryChangedPayload(_demoProjectDirectory()),
    );
    return project;
  }

  @override
  Future<void> activateProject(String projectId) async {
    if (!_demoProjectDirectory().values.any(
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
    ThreadModeId mode, {
    required ModeModelRouteView route,
    String? workspaceMode,
  }) async {
    final projects = _demoProjectDirectory().values;
    if (!projects.any((project) => project.id == projectId)) {
      throw StateError('unknown demo project $projectId');
    }
    if (input.text.trim().isEmpty && input.attachmentDraftIds.isEmpty) {
      throw ArgumentError.value(input.text, 'text', 'empty');
    }
    final requestedWorkspaceMode = switch (workspaceMode?.trim()) {
      null || '' || 'local' => ThreadWorkspaceMode.local,
      'worktree' => ThreadWorkspaceMode.worktree,
      final other => throw ArgumentError.value(
        other,
        'workspaceMode',
        'must be local or worktree',
      ),
    };
    final now = DateTime.now();
    final provisionalTitle = _demoProvisionalThreadTitle(input.text);
    final threadId = 'thread-created-${++_threadSequence}';
    final projectPath = projects
        .firstWhere((project) => project.id == projectId)
        .path;
    final thread = StudioThread(
      id: threadId,
      projectId: projectId,
      title: provisionalTitle,
      mode: mode,
      role: 'planner',
      createdAt: now,
      updatedAt: now,
      workspaceMode: requestedWorkspaceMode,
      workspacePath: requestedWorkspaceMode == ThreadWorkspaceMode.worktree
          ? _demoWorktreePath(projectPath, threadId)
          : projectPath,
    );
    _createdRootThreads.add(thread);
    _ensureWorkspaceFixture();
    _selectedThreadId = thread.id;
    final workspace = _workspaces[thread.id]!;
    _workspaces[thread.id] = workspace.copyWith(
      runtime: workspace.runtime.copyWith(
        model: route.model,
        modelRoute: ThreadModelRouteView(
          providerId: route.providerId,
          model: route.model,
          effort: route.effort,
          revision: 1,
          available: true,
        ),
      ),
    );
    final receipt = await _submitPrompt(thread.id, input);
    if (input.text.trim().isNotEmpty) {
      unawaited(_completeDemoThreadTitle(thread.id, provisionalTitle));
    }
    return StartNewThreadResult(thread: thread, receipt: receipt);
  }

  @override
  Future<RenameThreadCommandReply> renameThread(
    String threadId,
    String title, {
    required OperationId operationId,
  }) async {
    final normalized = title.trim();
    if (normalized.isEmpty || normalized.runes.length > 80) {
      throw ArgumentError.value(title, 'title', 'must be 1–80 characters');
    }
    final index = _createdRootThreads.indexWhere(
      (thread) => thread.id == threadId,
    );
    if (index < 0) throw StateError('unknown demo root Thread $threadId');
    final changed = _createdRootThreads[index].title != normalized;
    final renamed = _createdRootThreads[index].copyWith(
      title: normalized,
      updatedAt: DateTime.now(),
    );
    _createdRootThreads[index] = renamed;
    if (_workspaces[threadId] case final workspace?) {
      _workspaces[threadId] = workspace.copyWith(thread: renamed);
    }
    _emitThreadDirectoryUpdate(renamed);
    return RenameThreadCommandReply(
      thread: renamed,
      receipt: _demoReceipt(
        operationId,
        changed: changed,
        targets: [_demoStamp('thread-directory', _threadDirectoryRevision)],
        effect: EffectPoint.immediate,
      ),
    );
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

  /// 记录一次已受理的模拟用户消息：推进目录的最近用户消息时间并广播增量。
  void _advanceDemoLastUserMessage(String threadId, DateTime acceptedAt) {
    StudioThread? updated;
    for (var index = 0; index < _createdRootThreads.length; index++) {
      if (_createdRootThreads[index].id == threadId) {
        updated = _createdRootThreads[index] = _createdRootThreads[index]
            .copyWith(lastUserMessageAt: acceptedAt, updatedAt: acceptedAt);
      }
    }
    for (var index = 0; index < _pageFillThreads.length; index++) {
      if (_pageFillThreads[index].id == threadId) {
        updated = _pageFillThreads[index] = _pageFillThreads[index].copyWith(
          lastUserMessageAt: acceptedAt,
          updatedAt: acceptedAt,
        );
      }
    }
    final workspace = _workspaces[threadId];
    if (workspace != null && workspace.thread.id == threadId) {
      final thread = workspace.thread.copyWith(
        lastUserMessageAt: acceptedAt,
        updatedAt: acceptedAt,
      );
      _workspaces[threadId] = workspace.copyWith(thread: thread);
      updated = thread;
    }
    if (updated != null) {
      _emitThreadDirectoryUpdate(updated);
    }
  }

  /// settings 写入的 canonical 事实同时经 topic 交付。
  ///
  /// 真实侧每次 `publish_settings_state` 都发布 `SettingsConfigStateChanged` 与
  /// `ModelCatalogStateChanged`，只让命令返回快照会让仅依赖 topic 的观察者停在旧值，
  /// demo 因此与真实 bridge 语义分叉（design/25 §25.10）。
  SettingsStateSnapshot _settingsWriteResult() {
    final state = _demoSettingsState();
    _publishProductPayload(SettingsConfigStateChangedPayload(state));
    _publishProductPayload(ModelCatalogStateChangedPayload(state));
    return state;
  }

  ResourceStamp _demoStamp(String key, int revision) => ResourceStamp(
    resourceKey: key,
    ownerEpoch: _ownerEpoch,
    revision: revision,
  );

  StudioCommandReceipt _demoReceipt(
    OperationId operationId, {
    required bool changed,
    required List<ResourceStamp> targets,
    required EffectPoint effect,
    bool? effective,
    bool requiresIdentityObservation = false,
  }) => StudioCommandReceipt(
    operationId: operationId,
    disposition: changed
        ? CommitDisposition.applied
        : CommitDisposition.noChange,
    targets: targets,
    effect: effect,
    effective: effective ?? effect == EffectPoint.immediate,
    requiresIdentityObservation: requiresIdentityObservation,
    // Demo has no native persistence and cannot acknowledge durability.
    durable: false,
  );

  SettingsCommandReply _demoSettingsReply(
    OperationId operationId,
    SettingsStateSnapshot before, {
    bool? changed,
    EffectPoint effect = EffectPoint.futureSession,
  }) {
    final snapshot = _settingsWriteResult();
    return SettingsCommandReply(
      snapshot: snapshot,
      receipt: _demoReceipt(
        operationId,
        changed:
            changed ??
            !_sameDemoFacts(
              _demoSettingsFacts(before),
              _demoSettingsFacts(snapshot),
            ),
        targets: [_demoStamp('settings', snapshot.revision)],
        effect: effect,
      ),
    );
  }

  EffectPoint _demoFieldEffect(SettingsFieldCommand command) =>
      switch (command) {
        GeneralFollowActiveTurnCommand() ||
        GeneralCompactTimelineCommand() ||
        GeneralSidebarWidthCommand() ||
        GeneralPinnedThreadIdsCommand() ||
        GeneralPinnedProjectIdsCommand() => EffectPoint.immediate,
        _ => EffectPoint.futureSession,
      };

  // Compare canonical business values, excluding resource clocks and diagnostics.
  // This is a value comparison of the one demo owner, not a second state store.
  List<Object?> _demoSettingsFacts(SettingsStateSnapshot s) => [
    s.defaultProviderId,
    s.permissionMode,
    {
      for (final p in s.providers)
        p.id: [
          p.name,
          p.templateKind,
          p.baseUrl,
          p.bearerToken,
          p.hasBearerToken,
          p.defaultModel,
          p.pricingEnabled,
          [
            for (final m in p.customModels)
              [
                m.slug,
                m.displayName,
                m.wireProtocol,
                m.contextWindow,
                m.maxOutputTokens,
              ],
          ],
          p.modelConnectionModes,
          {
            for (final e in p.autoCompactLimits.entries)
              e.key: e.value.overrideLimit,
          },
        ],
    },
    {
      for (final route in s.modeModelRoutes)
        route.modeId.id: [route.providerId, route.model, route.effort],
    },
    {
      for (final role in s.roles)
        role.key: [role.providerId, role.model, role.effort],
    },
    {
      for (final server in s.mcpServers)
        server.id: [server.transport, server.endpoint],
    },
    [
      s.instructions.baseOverride,
      s.instructions.developer,
      s.instructions.user,
      s.instructions.projectDocMaxBytes,
      s.instructions.projectDocFallbackFilenames,
    ],
    [
      s.skills.enabled,
      s.skills.autoLearn,
      s.skills.systemEnabled,
      s.skills.projectDir,
      s.skills.userDir,
      s.skills.externalDirs,
      s.skills.disabled,
      s.skills.autoLearnMinToolCalls,
    ],
    [
      s.general.followActiveTurn,
      s.general.compactTimeline,
      s.general.sidebarWidth,
      s.general.pinnedThreadIds,
      s.general.pinnedProjectIds,
    ],
    [
      s.webSearch.configuredMode,
      s.webSearch.contextSize,
      s.webSearch.allowedDomains,
      s.webSearch.country,
      s.webSearch.region,
      s.webSearch.city,
      s.webSearch.timezone,
    ],
    [
      s.deepSeekWebSearch.configuredEnabled,
      s.deepSeekWebSearch.configuredFetchEnabled,
    ],
  ];

  bool _sameDemoFacts(Object? left, Object? right) {
    if (left is List && right is List) {
      return left.length == right.length &&
          [
            for (var i = 0; i < left.length; i++)
              _sameDemoFacts(left[i], right[i]),
          ].every((value) => value);
    }
    if (left is Map && right is Map) {
      return left.length == right.length &&
          left.keys.every(
            (key) =>
                right.containsKey(key) && _sameDemoFacts(left[key], right[key]),
          );
    }
    return left == right;
  }

  void _emitThreadDirectoryUpdate(StudioThread thread) {
    _threadDirectoryRevision += 1;
    _publishProductPayload(
      ThreadDirectoryChangedPayload(
        revision: _threadDirectoryRevision,
        upserted: [thread],
        removed: const [],
      ),
    );
  }

  /// 配置级 Profiles 变化同样发布 typed topic 事实（保存命令返回 Settings snapshot，
  /// 但 Profiles 目录自身是独立资源；订阅者按 revision 前进）。
  void _emitAgentProfilesUpdate() {
    _agentProfilesRevision += 1;
    _publishProductPayload(
      AgentProfilesStateChangedPayload(readAgentProfilesStateSync()),
    );
  }

  AgentProfilesStateView readAgentProfilesStateSync() {
    return AgentProfilesStateView(
      state: ReadyObservedResource(
        revision: _agentProfilesRevision,
        updatedAt: 0,
        lastCheckedAt: null,
        value: AgentProfilesDataView(
          profiles: _demoAgentProfilesSync(),
          diagnostics: const [],
        ),
      ),
    );
  }

  List<AgentProfileView> _demoAgentProfilesSync() {
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
        id: 'executor',
        displayName: 'Executor',
        description: '在项目目录内实施边界明确的修改。',
        whenToUse: '需要限制 糊来帮内置文件写工具的修改目录时。',
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
  Future<ArchiveThreadCommandReply> archiveThread(
    String threadId, {
    required OperationId operationId,
  }) async {
    final thread = _sortedDirectoryThreads()
        .where((candidate) => candidate.id == threadId)
        .firstOrNull;
    if (thread == null || !thread.isRoot) {
      throw StateError('only a root Thread can be archived');
    }
    // 归档前先结束会话树的活动工作：中断正在运行的 Turn，再归档。
    final activeTurn = _workspaces[threadId]?.activeTurn;
    if (activeTurn != null && activeTurn.state.isBusy) {
      _interruptDemoTurn(threadId, activeTurn.turnId);
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
    _threadDirectoryRevision += 1;
    _publishProductPayload(
      ThreadDirectoryChangedPayload(
        revision: _threadDirectoryRevision,
        upserted: const [],
        removed: removedThreadIds,
      ),
    );
    return ArchiveThreadCommandReply(
      result: ArchiveThreadResult(
        archivedRootId: threadId,
        removedThreadIds: removedThreadIds,
        nextRoot: nextRoot,
      ),
      receipt: _demoReceipt(
        operationId,
        changed: true,
        targets: [_demoStamp('thread-directory', _threadDirectoryRevision)],
        effect: EffectPoint.immediate,
      ),
    );
  }

  @override
  Future<void> archiveProject(String projectId) async {
    if (projectId == 'project-local' && _archivedProjectIds.add(projectId)) {
      _projectDirectoryRevision += 1;
      final removed = _workspaces.values
          .where((workspace) => workspace.thread.projectId == projectId)
          .map((workspace) => workspace.thread.id)
          .toList();
      _threadDirectoryRevision += 1;
      _publishProductPayload(
        ProjectDirectoryChangedPayload(_demoProjectDirectory()),
      );
      _publishProductPayload(
        ThreadDirectoryChangedPayload(
          revision: _threadDirectoryRevision,
          upserted: const [],
          removed: removed,
        ),
      );
    }
  }

  @override
  Future<PersistenceStateSnapshot> retryPersistence() async =>
      const PersistenceStateSnapshot.ready();

  @override
  Future<PersistenceQueueSnapshot> retryThreadHistory(
    String threadId,
    int faultGeneration,
  ) async => const PersistenceQueueSnapshot.empty();

  /// demo 是纯内存实现，没有存储故障闩：显式继续同样是空的 canonical 观测。
  @override
  Future<PersistenceQueueSnapshot> resumeThreadHistory(
    String threadId,
    int faultGeneration,
  ) async => const PersistenceQueueSnapshot.empty();

  /// demo 是纯内存实现，没有异步持久化队列，因此队列压力恒为空的 canonical 观测。
  @override
  Future<PersistenceQueueStateView> readPersistenceQueueState() async =>
      const PersistenceQueueStateView(
        revision: 0,
        queue: PersistenceQueueSnapshot.empty(),
      );

  @override
  Future<PersistenceQueueSnapshot> readPersistenceQueue() async =>
      const PersistenceQueueSnapshot.empty();

  /// Demo counterpart of the production field mutation endpoint.
  ///
  /// The demo must obey the same contract as the Bridge: one command changes
  /// one canonical field/resource and never reconstructs a sibling settings
  /// object from a stale page snapshot.
  @override
  Future<SettingsCommandReply> applySettingsField(
    int expectedSettingsRevision,
    SettingsFieldCommand command, {
    required OperationId operationId,
  }) async {
    final before = _demoSettingsState();
    _checkSettingsRevision(expectedSettingsRevision);
    final settings = before;
    switch (command) {
      case InstructionBaseOverrideCommand(:final value):
        _instructions = _instructions.copyWith(baseOverride: value);
      case InstructionDeveloperCommand(:final value):
        _instructions = _instructions.copyWith(developer: value);
      case InstructionUserCommand(:final value):
        _instructions = _instructions.copyWith(user: value);
      case ProjectDocMaxBytesCommand(:final value):
        _instructions = _instructions.copyWith(projectDocMaxBytes: value);
      case ProjectDocFallbackFilenamesCommand(:final value):
        _instructions = _instructions.copyWith(
          projectDocFallbackFilenames: value,
        );
      case SkillsEnabledCommand(:final value):
        _skills = _skills.copyWith(enabled: value);
      case SkillsAutoLearnCommand(:final value):
        _skills = _skills.copyWith(autoLearn: value);
      case SkillsSystemEnabledCommand(:final value):
        _skills = _skills.copyWith(systemEnabled: value);
      case SkillsProjectDirCommand(:final value):
        _skills = _skills.copyWith(projectDir: value);
      case SkillsUserDirCommand(:final value):
        _skills = _skills.copyWith(userDir: value);
      case SkillsExternalDirsCommand(:final value):
        _skills = _skills.copyWith(externalDirs: value);
      case SkillsDisabledCommand(:final value):
        _skills = _skills.copyWith(disabled: value);
      case SkillsAutoLearnMinToolCallsCommand(:final value):
        _skills = _skills.copyWith(autoLearnMinToolCalls: value);
      case McpServerEnabledCommand():
      case McpServerTransportCommand():
      case McpServerEndpointCommand():
        // The demo fixture has no configurable MCP entries. Keep the command
        // observable through the same revision/resource boundary as Bridge.
        _mcpRevision += 1;
        _publishProductPayload(McpStateChangedPayload(_demoMcpState()));
      case GeneralFollowActiveTurnCommand(:final value):
        _general = _general.copyWith(followActiveTurn: value);
      case GeneralCompactTimelineCommand(:final value):
        _general = _general.copyWith(compactTimeline: value);
      case GeneralSidebarWidthCommand(:final value):
        _general = _general.copyWith(sidebarWidth: value ?? 336);
      case GeneralPinnedThreadIdsCommand(:final value):
        _general = _general.copyWith(pinnedThreadIds: value);
      case GeneralPinnedProjectIdsCommand(:final value):
        _general = _general.copyWith(pinnedProjectIds: value);
      case WebSearchModeCommand(:final value):
        _webSearch = _webSearch.withConfiguredValues(
          configuredMode: value,
          contextSize: _webSearch.contextSize,
          allowedDomains: _webSearch.allowedDomains,
          country: _webSearch.country,
          region: _webSearch.region,
          city: _webSearch.city,
          timezone: _webSearch.timezone,
        );
      case WebSearchContextSizeCommand(:final value):
        _webSearch = _webSearch.withConfiguredValues(
          configuredMode: _webSearch.configuredMode,
          contextSize: value,
          allowedDomains: _webSearch.allowedDomains,
          country: _webSearch.country,
          region: _webSearch.region,
          city: _webSearch.city,
          timezone: _webSearch.timezone,
        );
      case WebSearchAllowedDomainsCommand(:final value):
        _webSearch = _webSearch.withConfiguredValues(
          configuredMode: _webSearch.configuredMode,
          contextSize: _webSearch.contextSize,
          allowedDomains: value,
          country: _webSearch.country,
          region: _webSearch.region,
          city: _webSearch.city,
          timezone: _webSearch.timezone,
        );
      case WebSearchCountryCommand(:final value):
        _webSearch = _webSearch.withConfiguredValues(
          configuredMode: _webSearch.configuredMode,
          contextSize: _webSearch.contextSize,
          allowedDomains: _webSearch.allowedDomains,
          country: value,
          region: _webSearch.region,
          city: _webSearch.city,
          timezone: _webSearch.timezone,
        );
      case WebSearchRegionCommand(:final value):
        _webSearch = _webSearch.withConfiguredValues(
          configuredMode: _webSearch.configuredMode,
          contextSize: _webSearch.contextSize,
          allowedDomains: _webSearch.allowedDomains,
          country: _webSearch.country,
          region: value,
          city: _webSearch.city,
          timezone: _webSearch.timezone,
        );
      case WebSearchCityCommand(:final value):
        _webSearch = _webSearch.withConfiguredValues(
          configuredMode: _webSearch.configuredMode,
          contextSize: _webSearch.contextSize,
          allowedDomains: _webSearch.allowedDomains,
          country: _webSearch.country,
          region: _webSearch.region,
          city: value,
          timezone: _webSearch.timezone,
        );
      case WebSearchTimezoneCommand(:final value):
        _webSearch = _webSearch.withConfiguredValues(
          configuredMode: _webSearch.configuredMode,
          contextSize: _webSearch.contextSize,
          allowedDomains: _webSearch.allowedDomains,
          country: _webSearch.country,
          region: _webSearch.region,
          city: _webSearch.city,
          timezone: value,
        );
      case DeepSeekWebSearchEnabledCommand(:final value):
        _deepSeekWebSearch = _deepSeekWebSearch.withConfiguredEnabled(value);
      case DeepSeekWebFetchEnabledCommand(:final value):
        _deepSeekWebSearch = _deepSeekWebSearch.withConfiguredFetchEnabled(
          value,
        );
      case ModeModelCommand(:final modeId, :final providerId, :final model):
        final mode = ThreadModeId.fromId(modeId);
        final selected = _findDemoModel(settings.providers, providerId, model);
        final previous = settings.modeModelRoutes
            .where((route) => route.modeId == mode)
            .firstOrNull;
        final effort =
            previous != null &&
                previous.providerId == providerId &&
                previous.model == model
            ? previous.effort
            : selected.defaultReasoningEffort.isNotEmpty
            ? selected.defaultReasoningEffort
            : selected.reasoningEfforts.firstOrNull ?? '';
        _modeRoutes = [
          for (final route in settings.modeModelRoutes)
            if (route.modeId != mode) route,
          ModeModelRouteView(
            modeId: mode,
            providerId: providerId,
            model: model,
            effort: effort,
          ),
        ];
      case ModeReasoningEffortCommand(:final modeId, :final effort):
        final mode = ThreadModeId.fromId(modeId);
        final previous = settings.modeModelRoutes
            .where((route) => route.modeId == mode)
            .firstOrNull;
        final route = previous ?? _demoModeRoute(mode);
        final selected = _findDemoModel(
          settings.providers,
          route.providerId,
          route.model,
        );
        final resolvedEffort =
            effort != null && selected.reasoningEfforts.contains(effort)
            ? effort
            : selected.defaultReasoningEffort.isNotEmpty
            ? selected.defaultReasoningEffort
            : selected.reasoningEfforts.firstOrNull ?? '';
        _modeRoutes = [
          for (final candidate in settings.modeModelRoutes)
            if (candidate.modeId != mode) candidate,
          ModeModelRouteView(
            modeId: mode,
            providerId: route.providerId,
            model: route.model,
            effort: resolvedEffort,
          ),
        ];
      case RoleModelCommand(:final role, :final providerId, :final model):
        final selected = _findDemoModel(settings.providers, providerId, model);
        final previous = settings.roles
            .where((candidate) => candidate.key == role)
            .firstOrNull;
        final effort =
            previous != null &&
                previous.providerId == providerId &&
                previous.model == model
            ? previous.effort
            : selected.defaultReasoningEffort.isNotEmpty
            ? selected.defaultReasoningEffort
            : selected.reasoningEfforts.firstOrNull ?? '';
        final next = RoleSettingsView(
          key: role,
          providerId: providerId,
          model: model,
          effort: effort,
        );
        _roles = [
          for (final candidate in settings.roles)
            if (candidate.key != role) candidate,
          next,
        ];
      case RoleReasoningEffortCommand(:final role, :final effort):
        final previous = settings.roles
            .where((candidate) => candidate.key == role)
            .firstOrNull;
        if (previous == null) {
          throw StateError('unknown demo Agent role: $role');
        }
        final selected = _findDemoModel(
          settings.providers,
          previous.providerId,
          previous.model,
        );
        final resolvedEffort =
            effort != null && selected.reasoningEfforts.contains(effort)
            ? effort
            : selected.defaultReasoningEffort.isNotEmpty
            ? selected.defaultReasoningEffort
            : selected.reasoningEfforts.firstOrNull ?? '';
        _roles = [
          for (final candidate in settings.roles)
            candidate.key == role
                ? RoleSettingsView(
                    key: candidate.key,
                    providerId: candidate.providerId,
                    model: candidate.model,
                    effort: resolvedEffort,
                  )
                : candidate,
        ];
    }
    _settingsRevision += 1;
    return _demoSettingsReply(
      operationId,
      before,
      effect: _demoFieldEffect(command),
    );
  }

  @override
  Future<ThreadModelRouteCommandReply> setThreadModelRoute({
    required OperationId operationId,
    required String threadId,
    required int expectedModelRouteRevision,
    required int expectedSettingsRevision,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    final current = _demoSettingsState();
    _checkSettingsRevision(expectedSettingsRevision);
    final thread = _sortedDirectoryThreads()
        .where((candidate) => candidate.id == threadId)
        .firstOrNull;
    final workspace = _workspaces[threadId];
    if (thread == null || !thread.isRoot || workspace == null) {
      throw StateError('unknown root demo thread $threadId');
    }
    final previousRoute = workspace.runtime.modelRoute;
    if (previousRoute == null) {
      throw StateError('demo thread has no saved model route');
    }
    if (previousRoute.revision != expectedModelRouteRevision) {
      throw StateError('model route revision conflict');
    }
    final selected = current.providers
        .firstWhere((provider) => provider.id == providerId)
        .models
        .firstWhere((candidate) => candidate.slug == model);
    final selectedEffort = selected.reasoningEfforts.contains(effort)
        ? effort
        : selected.reasoningEfforts.firstOrNull ?? '';
    _modeRoutes = [
      for (final route in current.modeModelRoutes)
        if (route.modeId != thread.mode) route,
      ModeModelRouteView(
        modeId: thread.mode,
        providerId: providerId,
        model: model,
        effort: selectedEffort ?? '',
      ),
    ];
    _settingsRevision += 1;
    final runtime = workspace.runtime.copyWith(
      modelRoute: ThreadModelRouteView(
        providerId: providerId,
        model: model,
        effort: selectedEffort,
        revision: previousRoute.revision + 1,
        available: true,
      ),
    );
    _workspaces[threadId] = workspace.copyWith(runtime: runtime);
    // 与生产一致：route 变更作为 Thread 运行时状态流事实交付（revision 单调）。
    // 只写内存 workspace 会让选择器在乐观值被清除后回落到旧 canonical route。
    _emitThreadUpdate(
      threadId,
      ThreadRuntimeUpdate(runtime: runtime, todo: workspace.todo),
    );
    final settings = _settingsWriteResult();
    return ThreadModelRouteCommandReply(
      receipt: _demoReceipt(
        operationId,
        changed:
            previousRoute.providerId != providerId ||
            previousRoute.model != model ||
            previousRoute.effort != selectedEffort ||
            !_sameDemoFacts(
              _demoSettingsFacts(current),
              _demoSettingsFacts(settings),
            ),
        targets: [
          _demoStamp('thread-route:$threadId', runtime.modelRoute!.revision),
          _demoStamp('settings', settings.revision),
        ],
        effect: EffectPoint.nextTurn,
      ),
      runtime: runtime,
      settings: settings,
      modeDefaultSaved: true,
      warning: null,
    );
  }

  @override
  Future<ThreadModeCommandReply> setThreadMode({
    required OperationId operationId,
    required String threadId,
    required ThreadModeId mode,
  }) async {
    final thread = _sortedDirectoryThreads()
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
    final workflow = _workspaces[threadId]?.runtime.workflow;
    if (workflow?.isActive ?? false) {
      throw StateError('thread mode cannot change while a workflow is active');
    }
    _threadModes[threadId] = mode;
    final workspace = _workspaces[threadId];
    if (workspace != null) {
      final settings = _demoSettingsState();
      final route =
          settings.modeModelRoutes
              .where((candidate) => candidate.modeId == mode)
              .firstOrNull ??
          settings.modeModelRoutes
              .where((candidate) => candidate.modeId == ThreadModeId.simple)
              .firstOrNull;
      final thread = workspace.thread;
      final updatedThread = thread.copyWith(
        mode: mode,
        role: thread.isRoot ? 'planner' : thread.role,
      );
      // 与生产一致：mode 变更同时交付两条 canonical 事实。
      //
      // 其一，运行时事实：native `set_thread_mode` 经 reconfigure 改写 mode 默认
      // route 扩展并发布 Thread 帧。demo 若只改内存 workspace 而不出这一帧，客户端
      // 的 threadSession 就只能在显式重读下看到新 route，验收无法发现帧丢失。
      _emitThreadUpdate(
        threadId,
        ThreadRuntimeUpdate(
          runtime: route == null
              ? workspace.runtime
              : workspace.runtime.copyWith(
                  modelRoute: ThreadModelRouteView(
                    providerId: route.providerId,
                    model: route.model,
                    effort: route.effort,
                    revision: (workspace.runtime.modelRoute?.revision ?? 0) + 1,
                    available: true,
                  ),
                ),
          todo: workspace.todo,
        ),
      );
      _workspaces[threadId] = _workspaces[threadId]!.copyWith(
        thread: updatedThread,
      );
      // 其二，目录事实：mode 由目录增量交付（目录 revision 前进）。不推进目录水位
      // 会让客户端的目录基线门与目录事件都看不到这次变更。
      _emitThreadDirectoryUpdate(updatedThread);
    }
    return ThreadModeCommandReply(
      receipt: _demoReceipt(
        operationId,
        changed: true,
        targets: [
          _demoStamp('thread-state:$threadId', _workspaces[threadId]!.revision),
          _demoStamp('thread-directory', _threadDirectoryRevision),
        ],
        effect: EffectPoint.immediate,
      ),
    );
  }

  @override
  Stream<ProductTopicFrame> subscribeProductTopic(ProductTopic topic) {
    if (lspActivityLoop && topic == const LspTopic()) {
      _startLspActivityLoop();
    }
    var detached = false;
    var baselineEmitted = false;
    final pending = <ProductTopicEventPayload>[];
    late final StreamController<ProductTopicFrame> controller;
    late final void Function(ProductTopicEventPayload) sink;

    void detach() {
      if (detached) return;
      detached = true;
      final sinks = _topicSinks[topic];
      if (sinks != null) {
        sinks.remove(sink);
        if (sinks.isEmpty) _topicSinks.remove(topic);
      }
    }

    void receive(ProductTopicEventPayload payload) {
      if (detached) return;
      if (!baselineEmitted) {
        // 基线尚未交付：本 topic 事件先缓冲，保证先基线后增量（无关领域不进入本 receiver）。
        pending.add(payload);
        return;
      }
      _emitProductFrame(controller, topic, payload);
    }

    sink = receive;

    controller = StreamController<ProductTopicFrame>(
      onListen: () async {
        // receiver 先登记，再取得 canonical 基线：期间本 topic 的事件不丢。
        (_topicSinks[topic] ??= <void Function(ProductTopicEventPayload)>{})
            .add(receive);
        try {
          final baseline = await _demoTopicBaseline(topic);
          if (detached) return;
          baselineEmitted = true;
          controller.add(baseline);
          for (final payload in pending) {
            _emitProductFrame(controller, topic, payload);
          }
          pending.clear();
        } catch (error, stackTrace) {
          if (!detached) {
            controller.addError(error, stackTrace);
          }
        }
      },
      onCancel: () async {
        detach();
      },
    );
    return controller.stream;
  }

  /// 把一个 owner 事实发布给该 topic 的已登记 receiver；无接收者时不缓冲。
  void _publishProductPayload(ProductTopicEventPayload payload) {
    final sinks = _topicSinks[payload.topic];
    if (sinks == null || sinks.isEmpty) return;
    for (final sink in List.of(sinks)) {
      sink(payload);
    }
  }

  /// 把 payload 包装成 typed Data 帧交付给 receiver。
  void _emitProductFrame(
    StreamController<ProductTopicFrame> controller,
    ProductTopic topic,
    ProductTopicEventPayload payload,
  ) {
    _productEventSequence += 1;
    controller.add(
      ProductTopicDataFrame(
        topic: topic,
        event: ProductTopicEventEnvelope(
          eventId: 'demo-${_productEventSequence.toString().padLeft(6, '0')}',
          sequence: BigInt.from(_productEventSequence),
          createdAt: DateTime.now(),
          payload: payload,
        ),
      ),
    );
  }

  /// Demo 基线从当前内存事实派生；与生产路径一致按领域 revision 表达。
  Future<ProductTopicBaselineFrame> _demoTopicBaseline(
    ProductTopic topic,
  ) async {
    final ProductTopicBaselineState baseline = switch (topic) {
      ProjectDirectoryTopic() => ProjectDirectoryBaseline(
        _demoProjectDirectory(),
      ),
      ThreadDirectoryTopic() => ThreadDirectoryBaseline(_demoDirectoryPage()),
      AgentDirectoryTopic() => AgentDirectoryBaseline(_demoAgentDirectory()),
      SettingsConfigTopic() => SettingsConfigBaseline(_demoSettingsState()),
      ModelCatalogTopic() => ModelCatalogBaseline(_demoSettingsState()),
      RecoveryTopic() => RecoveryBaseline(_demoRecoveryState()),
      McpTopic() => McpBaseline(_demoMcpState()),
      LspTopic() => LspBaseline(_demoLspState()),
      SkillsTopic(:final projectId) => SkillsBaseline(
        _demoSkillsState(projectId, _skillsRevision),
      ),
      ThreadModeCatalogTopic() => ThreadModeCatalogBaseline(
        _demoThreadModeCatalog(),
      ),
      ProviderUsageTopic() => ProviderUsageBaseline(_demoProviderUsageState()),
      ModelPerformanceTopic() => ModelPerformanceBaseline(
        _demoModelPerformance(_fixtureTime),
      ),
      SessionCostsTopic(:final rootThreadId) => SessionCostsBaseline(
        _demoSessionCosts(rootThreadId),
      ),
      UpdaterTopic() => UpdaterBaseline(_demoUpdaterState()),
      PersistenceTopic() => PersistenceBaseline(_demoPersistenceState()),
      PersistenceQueueTopic() => PersistenceQueueBaseline(
        _demoPersistenceQueueState(),
      ),
      AgentProfilesTopic() => AgentProfilesBaseline(
        readAgentProfilesStateSync(),
      ),
    };
    final revision = switch (baseline) {
      ProjectDirectoryBaseline(:final state) => state.revision,
      ThreadDirectoryBaseline(:final page) => page.revision,
      AgentDirectoryBaseline(:final state) => state.revision,
      SettingsConfigBaseline(:final state) => state.revision,
      ModelCatalogBaseline(:final state) => state.modelCatalogRevision,
      RecoveryBaseline(:final state) => state.revision,
      McpBaseline(:final state) => state.revision,
      LspBaseline(:final state) => state.revision,
      SkillsBaseline(:final state) => state.revision,
      ThreadModeCatalogBaseline(:final state) => state.revision,
      ProviderUsageBaseline(:final state) => state.revision,
      ModelPerformanceBaseline(:final state) => state.revision,
      SessionCostsBaseline(:final state) => state.revision,
      UpdaterBaseline(:final state) => state.revision,
      PersistenceBaseline(:final state) => state.revision,
      PersistenceQueueBaseline(:final state) => state.revision,
      AgentProfilesBaseline(:final state) => state.revision,
    };
    return ProductTopicBaselineFrame(
      topic: topic,
      revision: revision,
      state: baseline,
    );
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
      _lspActivityPercentage = percentage;
      _publishProductPayload(LspStateChangedPayload(_demoLspState()));
      index = (index + 1) % cycle.length;
    });
  }

  @override
  Stream<ThreadStreamFrame> subscribeThread(String threadId) {
    StreamSubscription<ThreadStreamFrame>? observation;
    late final StreamController<ThreadStreamFrame> controller;
    controller = StreamController<ThreadStreamFrame>(
      onListen: () {
        // Register before taking the current value, so no update can fall
        // between the baseline and observing future owner notifications.
        observation = _threadEvents.stream
            .where(
              (frame) => switch (frame) {
                ThreadSnapshotFrame(:final workspace) =>
                  workspace.thread.id == threadId,
                ThreadNotificationFrame(threadId: final id) => id == threadId,
                ThreadResyncRequiredFrame(threadId: final id) => id == threadId,
              },
            )
            .listen(controller.add, onError: controller.addError);
        try {
          controller.add(
            ThreadSnapshotFrame(
              workspace: _snapshotOf(_demoWorkspace(threadId)),
            ),
          );
        } catch (error, stack) {
          controller.addError(error, stack);
        }
      },
      onCancel: () => observation?.cancel(),
    );
    return controller.stream;
  }

  /// Thread 首帧只暴露当前状态：已完成条目由分页历史提供，
  /// 未终态条目由内容窗口提供。
  ThreadWorkspace _snapshotOf(ThreadWorkspace workspace) {
    return workspace.copyWith(items: const [], latestTurn: null);
  }

  /// 内容窗口：正文（含流式）只经它交给 UI，与状态流相互独立。
  @override
  Future<StudioChatWindow> openChatWindow(String threadId) async {
    if (!_workspaces.containsKey(threadId)) {
      throw StateError('unknown demo thread $threadId');
    }
    final window = _DemoChatWindow(this, threadId);
    final factory = _chatWindowFactory;
    // 工厂拿到的是真实窗口：注入者只包装故障点，不会再次回到 openChatWindow。
    return factory == null ? window : factory(threadId, window);
  }

  /// 测试专用：包装内容窗口，用于故障注入（首帧失败、增量失败、EOF、慢打开）。
  ///
  /// 收到的是 demo 的真实窗口，只替换故障点；状态流、目录与正文事实仍是 demo 的实现，
  /// 因此注入的是生命周期交错而不是另一套业务状态。
  Future<StudioChatWindow> Function(String threadId, StudioChatWindow window)?
  _chatWindowFactory;

  @visibleForTesting
  set chatWindowFactory(
    Future<StudioChatWindow> Function(String threadId, StudioChatWindow window)?
    factory,
  ) {
    _chatWindowFactory = factory;
  }

  /// 按活动身份读取完整详情。demo 没有独立的正文投影，活动本身即为可用事实。
  @override
  Future<ThreadActivityDetail> readThreadActivityDetail(
    String threadId,
    String activityId,
  ) async {
    final workspace = _workspaces[threadId];
    if (workspace == null) {
      throw StateError('unknown demo thread $threadId');
    }
    final activity = workspace.activity;
    if (activity == null) {
      return EndedThreadActivityDetail(
        threadId: threadId,
        activityId: activityId,
      );
    }
    if (activity.identity != activityId) {
      return SupersededThreadActivityDetail(
        activity: activity,
        requestedActivityId: activityId,
      );
    }
    return CurrentThreadActivityDetail(activity: activity);
  }

  @override
  Future<SubmitPromptReceipt> submitPrompt(
    String threadId,
    StudioPromptInput input, {
    required int expectedModelRouteRevision,
  }) => _submitPrompt(
    threadId,
    input,
    expectedModelRouteRevision: expectedModelRouteRevision,
  );

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
    StudioPromptInput input, {
    int? expectedModelRouteRevision,
  }) async {
    final trimmed = input.text.trim();
    if (trimmed.isEmpty && input.attachmentDraftIds.isEmpty) {
      throw ArgumentError.value(input.text, 'text', 'empty');
    }
    final workspace = _workspaces[threadId];
    if (workspace == null) throw StateError('unknown demo thread $threadId');
    final route = workspace.runtime.modelRoute;
    if (expectedModelRouteRevision != null &&
        route?.revision != expectedModelRouteRevision) {
      throw const StudioFailure(
        code: StudioFailureCode.staleRevision,
        message: 'The Thread model route changed before input admission',
        retryable: false,
        correlationId: 'demo-prompt-route-conflict',
      );
    }
    if (route == null || !route.available) {
      throw const StudioFailure(
        code: StudioFailureCode.invalidArgument,
        message: 'The Thread model route is unavailable',
        retryable: false,
        correlationId: 'demo-prompt-route-unavailable',
      );
    }
    final activeTurn = workspace.activeTurn;
    if (activeTurn != null && activeTurn.state.isBusy) {
      _interruptDemoTurn(threadId, activeTurn.turnId);
    }
    // No await separates the route CAS and admission. Demo follows the Thread
    // route, rather than re-reading role defaults after the user's intent.
    _emitThreadUpdate(
      threadId,
      ThreadRuntimeUpdate(
        runtime: workspace.runtime.copyWith(model: route.model),
        todo: null,
      ),
    );
    final generation = _promptGenerations.update(
      threadId,
      (value) => value + 1,
      ifAbsent: () => 1,
    );
    final turnId = 'demo-turn-${++_turnSequence}';
    final now = DateTime.now();
    final receipt = SubmitPromptReceipt(
      threadId: threadId,
      inputId: input.inputId,
      cursor: workspace.revision + 1,
    );
    // 模拟服务同样只在成功受理后推进最近用户消息时间；拒绝路径在上方抛出、不推进。
    _advanceDemoLastUserMessage(threadId, now);
    _upsertChatItem(
      threadId,
      _messageItem(
        id: input.inputId,
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
    );
    _emitThreadUpdate(
      threadId,
      ThreadTurnUpdate(
        StudioTurnView(
          turnId: turnId,
          inputId: input.inputId,
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
    // 脚本化活动：提交后进入“思考中”（等待 API / 开始 reasoning）。
    _emitActivity(
      threadId,
      turnId: turnId,
      inputId: input.inputId,
      kind: ThreadActivityKind.thinking,
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
    _upsertChatItem(
      threadId,
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
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    _appendChatContent(
      threadId,
      itemId: reasoningId,
      revision: 1,
      field: const ThreadThinkingSummaryFieldView(0),
      text: '## Inspecting',
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    _appendChatContent(
      threadId,
      itemId: reasoningId,
      revision: 2,
      field: const ThreadThinkingSummaryFieldView(0),
      text: ' the request\n\nChecking the live ThreadItem projection.',
    );
    _appendChatContent(
      threadId,
      itemId: reasoningId,
      revision: 3,
      field: const ThreadThinkingContentFieldView(0),
      text: reasoningContent,
    );
    // 脚本化活动：reasoning 已有内容时显示“思考中”+最新非空逻辑行。
    _emitActivity(
      threadId,
      turnId: turnId,
      inputId: null,
      kind: ThreadActivityKind.thinking,
      summary: 'Checking the live ThreadItem projection.',
    );
    final liveReasoning = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == reasoningId,
    );
    final reasoningCompletedAt = DateTime.now();
    _upsertChatItem(
      threadId,
      liveReasoning.copyWith(
        revision: 4,
        updatedAt: reasoningCompletedAt,
        state: ThreadThinkingItemStateView(
          summary: const [reasoningSummary],
          content: const [reasoningContent],
          lifecycle: CompletedThreadContentView(reasoningCompletedAt),
        ),
      ),
    );
    final commentaryId = '$turnId:commentary';
    _upsertChatItem(
      threadId,
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
    );
    _appendChatContent(
      threadId,
      itemId: commentaryId,
      revision: 1,
      field: const ThreadTextFieldView(),
      text: '正在逐段核对 Timeline',
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    _appendChatContent(
      threadId,
      itemId: commentaryId,
      revision: 2,
      field: const ThreadTextFieldView(),
      text: ' 的 typed delta。',
    );
    final commentaryCompletedAt = DateTime.now();
    final liveCommentary = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == commentaryId,
    );
    _upsertChatItem(
      threadId,
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
    // 脚本化活动：工具前台显示“执行中”+完整命令行预览。
    _emitActivity(
      threadId,
      turnId: turnId,
      inputId: null,
      kind: ThreadActivityKind.runningTool,
      summary: 'cargo test -p pl-model --test provider_wire',
    );
    final toolId = '$turnId:tool';
    _upsertChatItem(
      threadId,
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
    );
    _appendChatContent(
      threadId,
      itemId: toolId,
      revision: 1,
      field: const ThreadToolArgumentsFieldView(),
      text: '{"command":"cargo test -p pl-model ',
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    _appendChatContent(
      threadId,
      itemId: toolId,
      revision: 2,
      field: const ThreadToolArgumentsFieldView(),
      text: '--test provider_wire"}',
    );
    final streamedTool = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == toolId,
    );
    final streamedToolState = streamedTool.state as ThreadToolItemStateView;
    _upsertChatItem(
      threadId,
      streamedTool.copyWith(
        revision: 3,
        updatedAt: DateTime.now(),
        state: ThreadToolItemStateView(
          invocation: streamedToolState.invocation,
          lifecycle: const RunningThreadToolView(''),
        ),
      ),
    );
    _appendChatContent(
      threadId,
      itemId: toolId,
      revision: 4,
      field: const ThreadToolResultFieldView(),
      text: 'running widget tests...\n',
    );
    await Future<void>.delayed(promptToolDelay);
    if (_promptGenerations[threadId] != generation) return;
    _appendChatContent(
      threadId,
      itemId: toolId,
      revision: 5,
      field: const ThreadToolResultFieldView(),
      text: '[stderr] analyzer warnings: 0\n',
    );
    final runningTool = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == toolId,
    );
    final runningToolState = runningTool.state as ThreadToolItemStateView;
    final toolCompletedAt = DateTime.now();
    _upsertChatItem(
      threadId,
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
    );
    final finalId = '$turnId:final';
    _upsertChatItem(
      threadId,
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
    );
    _appendChatContent(
      threadId,
      itemId: finalId,
      revision: 1,
      field: const ThreadTextFieldView(),
      text: 'Demo response for: **$trimmedPrompt**',
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[threadId] != generation) return;
    const finalSuffix = '\n\n- reasoning、tool output 与 Plan 都直接来自 typed delta';
    _appendChatContent(
      threadId,
      itemId: finalId,
      revision: 2,
      field: const ThreadTextFieldView(),
      text: finalSuffix,
    );
    final finalCompletedAt = DateTime.now();
    final liveFinal = _workspaces[threadId]!.items.firstWhere(
      (item) => item.id == finalId,
    );
    _upsertChatItem(
      threadId,
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
    // Turn 结束：清空当前活动。
    _clearActivity(threadId);
  }

  @override
  Future<InterruptTurnCommandReply> interruptTurn(
    String threadId,
    String turnId, {
    required OperationId operationId,
  }) async {
    final interrupted = _interruptDemoTurn(threadId, turnId);
    return InterruptTurnCommandReply(
      threadId: threadId,
      turnId: turnId,
      interrupted: interrupted,
      receipt: _demoReceipt(
        operationId,
        changed: interrupted,
        targets: const [],
        effect: EffectPoint.immediate,
        effective: false,
        requiresIdentityObservation: true,
      ),
    );
  }

  bool _interruptDemoTurn(String threadId, String turnId) {
    final active = _workspaces[threadId]?.activeTurn;
    if (active == null || !active.state.isBusy) return false;
    if (active.turnId != turnId) {
      throw const StudioFailure(
        code: StudioFailureCode.invalidArgument,
        message: 'Expected Turn does not match the active Turn',
        retryable: false,
        correlationId: 'demo-interrupt-turn-identity',
      );
    }
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
    // 打断结束 Turn：清空当前活动。
    _clearActivity(threadId);
    return true;
  }

  @override
  Future<InteractionCommandReply> respondInteraction(
    String interactionId,
    InteractionResolutionCommand resolution, {
    required OperationId operationId,
  }) async {
    _ensureWorkspaceFixture();
    final facts = switch (resolution) {
      UserInputResolutionCommand(:final answers) => <Object?>[
        'user-input',
        {
          for (final answer in answers)
            answer.questionId: List<String>.of(answer.answers),
        },
      ],
      ToolApprovalResolutionCommand(:final decision, :final reason) =>
        <Object?>['tool-approval', decision, reason],
    };
    final previous = _resolvedInteractions[interactionId];
    if (previous != null) {
      if (!_sameDemoFacts(previous.resolution, facts)) {
        throw const StudioFailure(
          code: StudioFailureCode.conflict,
          message: 'Interaction already has a different terminal resolution',
          retryable: false,
          correlationId: 'demo-interaction-conflict',
        );
      }
      return InteractionCommandReply(
        interaction: previous.interaction,
        receipt: _demoReceipt(
          operationId,
          changed: false,
          targets: const [],
          effect: EffectPoint.immediate,
          requiresIdentityObservation: true,
        ),
      );
    }
    final interaction = [
      for (final workspace in _workspaces.values) ...workspace.interactions,
    ].where((item) => item.id == interactionId).firstOrNull;
    if (interaction == null) {
      throw const StudioFailure(
        code: StudioFailureCode.notFound,
        message: 'Interaction is unavailable',
        retryable: false,
        correlationId: 'demo-interaction-not-found',
      );
    }
    _resolvedInteractions[interactionId] = (
      interaction: interaction,
      resolution: facts,
    );
    // Bound terminal receipts without inventing a new active interaction owner.
    while (_resolvedInteractions.length > 128) {
      _resolvedInteractions.remove(_resolvedInteractions.keys.first);
    }
    _emitThreadUpdate(
      interaction.threadId,
      ThreadInteractionUpdate(interaction: interaction, pending: false),
    );
    if (interaction.kind == InteractionKind.userInput) {
      _advanceDemoLastUserMessage(interaction.threadId, DateTime.now());
    }
    return InteractionCommandReply(
      interaction: interaction,
      receipt: _demoReceipt(
        operationId,
        changed: true,
        targets: const [],
        effect: EffectPoint.immediate,
        requiresIdentityObservation: true,
      ),
    );
  }

  @override
  Future<SettingsCommandReply> saveRuntimePermissionMode(
    int expectedSettingsRevision,
    PermissionMode mode, {
    required OperationId operationId,
  }) async {
    final before = _demoSettingsState();
    _checkSettingsRevision(expectedSettingsRevision);
    _permissionMode = mode;
    _settingsRevision += 1;
    return _demoSettingsReply(operationId, before);
  }

  @override
  Future<SettingsCommandReply> saveProvider(
    int expectedSettingsRevision,
    ProviderCommand command, {
    required OperationId operationId,
  }) async {
    final before = _demoSettingsState();
    _checkSettingsRevision(expectedSettingsRevision);
    final current = before;
    final converted = _providersFromCommands(
      [command],
      previous: current.providers,
      catalog: _providerCatalog,
    );
    final originalId = command.originalId ?? command.id;
    _providers = [
      for (final provider in current.providers)
        if (provider.id != originalId && provider.id != command.id) provider,
      ...converted,
    ];
    _settingsRevision += 1;
    return _demoSettingsReply(operationId, before);
  }

  @override
  Future<SettingsCommandReply> setDefaultProvider(
    int expectedSettingsRevision,
    String providerId, {
    required OperationId operationId,
  }) async {
    final before = _demoSettingsState();
    _checkSettingsRevision(expectedSettingsRevision);
    if (!before.providers.any((provider) => provider.id == providerId)) {
      throw StateError('Provider is unavailable: $providerId');
    }
    _defaultProviderId = providerId;
    _settingsRevision += 1;
    return _demoSettingsReply(operationId, before);
  }

  @override
  Future<SettingsCommandReply> removeProviderModel(
    int expectedSettingsRevision,
    String providerId,
    String slug,
  ) async {
    final before = _demoSettingsState();
    _checkSettingsRevision(expectedSettingsRevision);
    if (slug.trim().isEmpty) {
      throw StateError('Model slug must not be empty');
    }
    final providers = [
      for (final provider in before.providers)
        if (provider.id == providerId)
          _demoRemoveManualModel(provider, slug)
        else
          provider,
    ];
    if (!providers.any((provider) => provider.id == providerId)) {
      throw StateError('unknown demo provider: $providerId');
    }
    _providers = providers;
    _settingsRevision += 1;
    return _demoSettingsReply(
      OperationId('remove-provider-model:$providerId:$slug'),
      before,
    );
  }

  @override
  Future<SettingsCommandReply> removeProvider(
    int expectedSettingsRevision,
    String providerId, {
    required OperationId operationId,
    String? replacementProviderId,
  }) async {
    final before = _demoSettingsState();
    _checkSettingsRevision(expectedSettingsRevision);
    final current = before;
    if (current.providers.length <= 1) {
      throw StateError('At least one provider is required');
    }
    final replacement =
        replacementProviderId ??
        current.providers
            .firstWhere((provider) => provider.id != providerId)
            .id;
    if (!current.providers.any((provider) => provider.id == replacement)) {
      throw StateError('Replacement provider is unavailable: $replacement');
    }
    _providers = [
      for (final provider in current.providers)
        if (provider.id != providerId) provider,
    ];
    _defaultProviderId = _defaultProviderId == providerId
        ? replacement
        : _defaultProviderId;
    _settingsRevision += 1;
    return _demoSettingsReply(operationId, before);
  }

  @override
  Future<SettingsStateSnapshot> readSettingsState() async =>
      _demoSettingsState();

  @override
  Future<ProviderUsageStateSnapshot> checkProviderUsage(
    String providerId,
  ) async {
    final providers = _providers ?? [_demoDefaultProvider()];
    final provider = providers
        .where((item) => item.id == providerId)
        .firstOrNull;
    if (provider != null) {
      _providerUsages[providerId] = _demoProviderUsage(provider);
    } else {
      _providerUsages.remove(providerId);
    }
    _providerUsageRevision += 1;
    final state = _demoProviderUsageState();
    _publishProductPayload(ProviderUsageStateChangedPayload(state));
    return state;
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
    final state = await readSkillsState(projectId);
    _publishProductPayload(SkillsStateChangedPayload(state));
    return state;
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
    return _mcpWriteResult();
  }

  @override
  Future<McpStateSnapshot> resetAllMcp() async {
    _mcpRevision += 1;
    return _mcpWriteResult();
  }

  @override
  Future<LspStateSnapshot> readLspState() async => _demoLspState();

  @override
  Future<LspStateSnapshot> probeLspServer(String projectId) async {
    _lspRevision += 1;
    return _lspWriteResult();
  }

  @override
  Future<LspStateSnapshot> repairLspServer(
    String projectId,
    String serverId,
  ) async {
    _lspRevision += 1;
    return _lspWriteResult();
  }

  @override
  Future<LspStateSnapshot> resetLspServer(
    String projectId,
    String serverId,
  ) async {
    _lspRevision += 1;
    return _lspWriteResult();
  }

  @override
  Future<LspStateSnapshot> resetLspWorkspace(String projectId) async {
    _lspRevision += 1;
    return _lspWriteResult();
  }

  McpStateSnapshot _mcpWriteResult() {
    final state = _demoMcpState();
    _publishProductPayload(McpStateChangedPayload(state));
    return state;
  }

  LspStateSnapshot _lspWriteResult() {
    _lspActivityPercentage = 40;
    final state = _demoLspState();
    _publishProductPayload(LspStateChangedPayload(state));
    return state;
  }

  McpStateSnapshot _demoMcpState() {
    return McpStateSnapshot.fromState(
      state: _demoReadyResource(_mcpRevision, const McpStateData()),
    );
  }

  LspStateSnapshot _demoLspState() {
    final percentage = _lspActivityPercentage;
    if (percentage == null) {
      return LspStateSnapshot.fromState(
        state: _demoReadyResource(_lspRevision, const LspStateData()),
      );
    }
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

  ProviderSettingsView _demoRemoveManualModel(
    ProviderSettingsView provider,
    String slug,
  ) {
    final target = provider.models
        .where((model) => model.slug == slug)
        .firstOrNull;
    if (target == null || target.origin != ProviderModelOrigin.manual) {
      throw StateError('unknown demo manual model: ${provider.id} / $slug');
    }
    final models = [
      for (final model in provider.models)
        if (model.slug != slug) model,
    ];
    final connections = {...provider.modelConnectionModes}..remove(slug);
    final limits = {...provider.autoCompactLimits}..remove(slug);
    return provider.copyWith(
      models: models,
      modelConnectionModes: connections,
      autoCompactLimits: limits,
      modelCount: '${models.length}',
      usageLabel: models.isEmpty
          ? provider.defaultModel
          : '${models.length} models',
    );
  }

  ProviderModelView _findDemoModel(
    List<ProviderSettingsView> providers,
    String providerId,
    String model,
  ) {
    final provider = providers
        .where((item) => item.id == providerId)
        .firstOrNull;
    if (provider == null) {
      throw StateError('unknown demo provider: $providerId');
    }
    final selected = provider.models
        .where((candidate) => candidate.slug == model)
        .firstOrNull;
    if (selected == null) {
      throw StateError('unknown demo model: $providerId / $model');
    }
    return selected;
  }

  /// 通知某 Thread 的内容窗口：条目发生真实变化（正文只由窗口交给 UI）。
  void _notifyItemsChanged(String threadId) {
    _chatChangeSeq[threadId] = (_chatChangeSeq[threadId] ?? 0) + 1;
    final waiters = _chatWaiters.remove(threadId);
    if (waiters == null) return;
    for (final waiter in waiters) {
      if (!waiter.isCompleted) waiter.complete();
    }
  }

  Future<void> _awaitItemsChanged(String threadId, int from) async {
    while ((_chatChangeSeq[threadId] ?? 0) == from) {
      final waiter = Completer<void>();
      (_chatWaiters[threadId] ??= []).add(waiter);
      await waiter.future;
    }
  }

  /// demo 模拟一次 Thread 状态更新。
  ///
  /// 状态流只承载 Turn / 活动 / 交互 / 运行时——与生产投影同一契约；**条目与正文不在
  /// 这里**，它们写进内存条目并由内容窗口（[openChatWindow]）按同一窗口契约交付。
  void _emitThreadUpdate(String threadId, ThreadWorkspaceUpdate update) {
    final workspace = _workspaces[threadId];
    if (workspace == null) return;
    final revision = workspace.revision + 1;
    // Turn revisions share the Thread event sequence, just like the native projector.
    if (update case ThreadTurnUpdate(:final turn)) {
      update = ThreadTurnUpdate(
        StudioTurnView(
          turnId: turn.turnId,
          inputId: turn.inputId ?? workspace.activeTurn?.inputId,
          threadId: threadId,
          revision: revision,
          state: turn.state,
          updatedAt: turn.updatedAt,
        ),
      );
    }
    final ThreadWorkspace updated = switch (update) {
      ThreadTurnUpdate(:final turn) => workspace.copyWith(
        revision: revision,
        activeTurn: turn.state.isBusy ? turn : null,
      ),
      // 活动与生产一样只从 typed 活动事实进入状态。
      ThreadActivityUpdate(:final activity) => workspace.copyWith(
        revision: revision,
        activity: activity,
      ),
      ThreadRuntimeUpdate(:final runtime, :final todo) => workspace.copyWith(
        revision: revision,
        runtime: runtime,
        todo: todo,
      ),
      // 存储状态与生产一样只从 typed 事实进入状态。
      ThreadStorageUpdate(:final storage) => workspace.copyWith(
        revision: revision,
        storage: storage,
      ),
      ThreadInteractionUpdate(:final interaction, :final pending) =>
        _demoUpdateInteraction(workspace, revision, interaction, pending),
    };
    _workspaces[threadId] = updated;
    _threadEvents.add(
      ThreadNotificationFrame(
        threadId: threadId,
        revision: revision,
        update: update,
      ),
    );
    if (update case ThreadTurnUpdate(:final turn)) {
      final id = 'turn:${turn.turnId.length}:${turn.turnId}';
      final previous = workspace.items
          .where((item) => item.id == id)
          .firstOrNull;
      _upsertChatItem(
        threadId,
        ThreadItemView(
          id: id,
          threadId: threadId,
          turnId: turn.turnId,
          ordinal: previous?.ordinal ?? _nextOrdinal(threadId),
          revision: turn.revision,
          createdAt: previous?.createdAt ?? turn.updatedAt,
          updatedAt: turn.updatedAt,
          state: ThreadTurnItemStateView(turn.state, inputId: turn.inputId),
        ),
      );
    }
  }

  /// 把一条条目写入内存窗口：条目只属于内容窗口，不推进线程状态水位。
  void _upsertChatItem(String threadId, ThreadItemView item) {
    final workspace = _workspaces[threadId];
    if (workspace == null) return;
    // demo 的内存条目没有独立的执行终态事件：状态进入终态即视为窗口终态。
    final entry = item.executionTerminal
        ? item
        : item.copyWith(executionTerminal: item.isTerminal);
    final items = [...workspace.items];
    final index = items.indexWhere((existing) => existing.id == entry.id);
    if (index < 0) {
      items.add(entry);
    } else {
      items[index] = entry;
    }
    items.sort(_compareThreadItems);
    _workspaces[threadId] = workspace.copyWith(items: items);
    _notifyItemsChanged(threadId);
  }

  /// 应用一帧 typed `UpdateItem` 字段变化，与 `BridgeViewChange::UpdateItem` 同形状：
  /// 按字段身份落到条目的 domain 字段，本地基线必须等于当前 revision；条目缺失、
  /// revision 不连续或已终态接受流式变化时丢弃（真实链路会重建权威窗口）。
  void _applyChatFieldUpdates(
    String threadId, {
    required String itemId,
    required int revision,
    required List<ThreadFieldUpdateView> fields,
  }) {
    final workspace = _workspaces[threadId];
    if (workspace == null) return;
    final items = [...workspace.items];
    final index = items.indexWhere((item) => item.id == itemId);
    if (index < 0) return;
    final current = items[index];
    final next = current.applyFieldUpdates(
      fields: fields,
      expectedRevision: current.revision,
      revision: revision,
      omittedUnits: current.bodyOmittedUnits,
      saved: current.saved,
      terminal: current.executionTerminal,
    );
    if (next == null) return;
    items[index] = next;
    _workspaces[threadId] = workspace.copyWith(items: items);
    _notifyItemsChanged(threadId);
  }

  /// 单字段 append 的便捷入口（demo 脚本只追加正文）。
  void _appendChatContent(
    String threadId, {
    required String itemId,
    required int revision,
    required ThreadContentFieldView field,
    required String text,
  }) {
    _applyChatFieldUpdates(
      threadId,
      itemId: itemId,
      revision: revision,
      fields: [
        ThreadFieldUpdateView(
          field: field,
          change: AppendThreadFieldChangeView(text),
        ),
      ],
    );
  }

  /// demo 脚本化的当前活动：作为 typed `ActivityChanged` 状态走状态流。
  void _emitActivity(
    String threadId, {
    required String turnId,
    required String? inputId,
    required ThreadActivityKind kind,
    String summary = '',
  }) {
    _emitThreadUpdate(
      threadId,
      ThreadActivityUpdate(
        activity: ThreadActivityView(
          threadId: threadId,
          identity: 'activity:$turnId:${kind.name}',
          revision: 0,
          turnId: turnId,
          inputId: inputId,
          kind: kind,
          summary: summary,
        ),
      ),
    );
  }

  /// Turn 结束或打断：清空当前活动（`activity == null`）。
  void _clearActivity(String threadId) {
    _emitThreadUpdate(threadId, const ThreadActivityUpdate(activity: null));
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

/// 内容窗口的内存实现：快照直接来自 demo 的工作区条目，变化由 [_DemoChatWindow.next]
/// 唤醒；不参与状态流，也没有第二份业务状态。
///
/// 交付形状与原生窗口一致：首帧与控制操作后的窗口移动是权威 `Reset`，条目**原地变化**按
/// typed 差量交付 `Patch`（`UpdateItem` 的字段身份、`expected_revision`、`omitted`/`saved`
/// 与 splice 索引都按同一契约）。只交付 `Reset` 会让 Dart 的差量 reducer 在 demo 预览与
/// 人工验收中完全不被走到——`from/to` 防线、字段原子提交和终态拒绝都只在单元测试里成立。
/// demo 仍然没有真实字节预览预算，也不证明原生差量的性能表现。
class _DemoChatWindow implements StudioChatWindow {
  _DemoChatWindow(this._api, this._threadId);

  final DemoStudioTransport _api;
  final String _threadId;
  String? _focusId;
  int _operations = 0;
  bool _closed = false;

  /// 上一次交付给消费者的基线：Reset 或 Patch 之后的窗口文本与版本。
  ///
  /// `Patch.from` 必须等于它，与原生窗口「对外交付的 Reset 版本等于之后差量的 from」同一条
  /// 规则。
  int? _baselineVersion;
  List<ThreadItemView> _baselineItems = const [];

  /// 每个推理条目已经交付过的块数，用于把追加块映射回**逻辑** chunk 下标。
  ///
  /// 只在有界窗口内的条目上增长；条目离开窗口时随基线一起清理。
  final Map<String, int> _deliveredChunks = {};

  List<ThreadItemView> get _items =>
      _api._workspaces[_threadId]?.items ?? const <ThreadItemView>[];

  StudioChatSnapshot _snapshot() {
    return StudioChatSnapshot(
      focusedItemId: _focusId,
      version: _api._workspaces[_threadId]?.revision ?? 0,
      items: List.unmodifiable([
        for (final item in _items)
          StudioChatItem(
            item,
            saved: item.saved,
            lifecycle: item.executionTerminal
                ? StudioChatLifecycle.terminal
                : StudioChatLifecycle.streaming,
          ),
      ]),
      hasOlder: false,
      hasNewer: false,
    );
  }

  StudioChatReceipt _receipt({
    StudioChatDisposition disposition = StudioChatDisposition.noChange,
  }) {
    return StudioChatReceipt(
      operationId: ++_operations,
      disposition: disposition,
      version: _api._workspaces[_threadId]?.revision ?? 0,
      retained: true,
    );
  }

  /// 采纳一次已经交给消费者的窗口基线。
  void _adopt(int version, List<ThreadItemView> items) {
    _baselineVersion = version;
    _baselineItems = List.unmodifiable(items);
    // 离开窗口的条目不再需要块计数：不能让它随会话增长。
    final live = {for (final item in items) item.id};
    _deliveredChunks.removeWhere(
      (key, _) => !live.contains(key.split('|').first),
    );
  }

  @override
  Future<StudioChatSnapshot> initial() async {
    final snapshot = _snapshot();
    _adopt(snapshot.version, _items);
    return snapshot;
  }

  @override
  Future<StudioChatFrame?> next() async {
    // 控制操作也会唤醒这条唯一出口（与原生窗口一致）；唤醒不等于有可交付变化。
    // 只有窗口版本真的前进才交付帧，否则回到等待：空 Patch（from == to）是退化帧，
    // 下游会按非递增版本拒收并要求重同步。
    while (!_closed) {
      final from = _api._chatChangeSeq[_threadId] ?? 0;
      await _api._awaitItemsChanged(_threadId, from);
      if (_closed) return null;
      final version = _api._workspaces[_threadId]?.revision ?? 0;
      final baseline = _baselineVersion;
      if (baseline == null || version <= baseline) continue;
      return _patchFrame(baseline, version) ?? _resetFrame();
    }
    return null;
  }

  StudioChatReset _resetFrame() {
    final snapshot = _snapshot();
    _adopt(snapshot.version, _items);
    return StudioChatReset(snapshot);
  }

  /// 把「基线 -> 当前窗口」表达成 typed 差量；不可表达时返回 null，由调用方回退到 Reset。
  StudioChatPatch? _patchFrame(int from, int to) {
    final items = _items;
    final changes = _windowChanges(_baselineItems, items);
    if (changes == null) return null;
    _adopt(to, items);
    return StudioChatPatch(
      from: from,
      to: to,
      focusedItemId: _focusId,
      changes: changes,
      hasOlder: false,
      hasNewer: false,
    );
  }

  /// 与原生窗口同形的差量：前缀/后缀共同前缀裁剪后的单段 splice，或逐条 typed 字段提交。
  List<StudioViewChange>? _windowChanges(
    List<ThreadItemView> delivered,
    List<ThreadItemView> current,
  ) {
    var head = 0;
    while (head < delivered.length &&
        head < current.length &&
        delivered[head].id == current[head].id) {
      head++;
    }
    var tail = 0;
    while (tail < delivered.length - head &&
        tail < current.length - head &&
        delivered[delivered.length - 1 - tail].id ==
            current[current.length - 1 - tail].id) {
      tail++;
    }
    final removed = delivered.length - head - tail;
    final inserted = current.sublist(head, current.length - tail);
    if (removed != 0 || inserted.length != delivered.length - head - tail) {
      // 窗口移动：用一次 splice 表达插入/删除，不逐条重发正文。
      return [
        StudioViewSplice(
          index: head,
          remove: removed,
          items: [for (final item in inserted) _chatItem(item)],
        ),
      ];
    }
    final changes = <StudioViewChange>[];
    for (var index = 0; index < inserted.length; index++) {
      final previous = delivered[head + index];
      final next = current[head + index];
      final update = _itemUpdate(previous, next);
      if (update == null) return null;
      if (update.fields.isNotEmpty ||
          update.expectedRevision != update.revision ||
          previous.saved != next.saved ||
          previous.executionTerminal != next.executionTerminal ||
          update.itemShell != null) {
        changes.add(update);
      }
    }
    return changes;
  }

  StudioViewItemUpdate? _itemUpdate(
    ThreadItemView previous,
    ThreadItemView next,
  ) {
    final fields = _fieldUpdates(next.id, previous.state, next.state);
    if (fields == null) return null;
    return StudioViewItemUpdate(
      itemId: next.id,
      expectedRevision: previous.revision,
      revision: next.revision,
      omittedUnits: next.bodyOmittedUnits,
      saved: next.saved,
      lifecycle: next.executionTerminal
          ? StudioChatLifecycle.terminal
          : StudioChatLifecycle.streaming,
      itemShell:
          previous.state.runtimeType != next.state.runtimeType ||
              previous.updatedAt != next.updatedAt ||
              previous.isTerminal != next.isTerminal
          ? next
          : null,
      fields: fields,
    );
  }

  /// 两个条目状态之间的 typed 字段变化；出现 demo 无法表达的状态种类时返回 null。
  List<StudioFieldUpdateView>? _fieldUpdates(
    String itemId,
    ThreadItemStateView previous,
    ThreadItemStateView next,
  ) {
    switch (next) {
      case ThreadTextItemStateView() when previous is ThreadTextItemStateView:
        return _withoutUnchanged([
          _textField(const ThreadTextFieldView(), previous.text, next.text),
        ]);
      case ThreadThinkingItemStateView()
          when previous is ThreadThinkingItemStateView:
        return _chunkFields(itemId, previous, next);
      case ThreadToolItemStateView() when previous is ThreadToolItemStateView:
        return _withoutUnchanged([
          _textField(
            const ThreadToolArgumentsFieldView(),
            previous.invocation.arguments,
            next.invocation.arguments,
          ),
          _textField(
            const ThreadToolResultFieldView(),
            _toolResultText(previous.lifecycle),
            _toolResultText(next.lifecycle),
          ),
        ]);
      default:
        // 字段域与 reducer 不一致的条目种类：宁可用权威 Reset 整窗收束，也不能提交一个
        // 不带字段的版本——那会让消费者把版本前进当成“内容已同步”。
        return null;
    }
  }

  /// 工具生命周期里的结果正文（与 `TOOL_RESULT_FIELD` 对应）；该状态没有结果字段时为空串。
  String _toolResultText(ThreadToolLifecycleView lifecycle) {
    return switch (lifecycle) {
      RunningThreadToolView(:final streamedOutput) => streamedOutput,
      CancellingThreadToolView(:final streamedOutput) => streamedOutput,
      SucceededThreadToolView(:final output) => output.result,
      FailedThreadToolView(:final output) => output?.result ?? '',
      _ => '',
    };
  }

  List<StudioFieldUpdateView> _withoutUnchanged(
    List<StudioFieldUpdateView> fields,
  ) {
    return [
      for (final field in fields)
        if (field.change is! UnchangedThreadFieldChangeView) field,
    ];
  }

  StudioFieldUpdateView _textField(
    ThreadContentFieldView field,
    String previous,
    String next,
  ) {
    if (previous == next) {
      return StudioFieldUpdateView(
        field: field,
        change: const UnchangedThreadFieldChangeView(),
      );
    }
    if (next.startsWith(previous)) {
      return StudioFieldUpdateView(
        field: field,
        change: AppendThreadFieldChangeView(next.substring(previous.length)),
      );
    }
    return StudioFieldUpdateView(
      field: field,
      change: ReplaceThreadFieldChangeView(next),
    );
  }

  /// 按 chunk 追加表达 `summary` / `content`：只提交真正新增的块，并保留逻辑块下标。
  ///
  /// 生产链路的 `chunkIndex` 是**逻辑**下标（见 `ThreadThinkingItemStateView` 的
  /// `*ChunkBase`）：本地列表被有界裁剪后下标会整体前移，因此这里用「已交付块数」而不是本地
  /// 长度推下标；块列表一旦不是前缀追加（裁剪/替换），退回整字段 `Replace`。
  List<StudioFieldUpdateView> _chunkFields(
    String itemId,
    ThreadThinkingItemStateView previous,
    ThreadThinkingItemStateView next,
  ) {
    final fields = <StudioFieldUpdateView>[];
    final contentField = _chunkField(
      itemId: itemId,
      kind: 'content',
      isSummary: false,
      base: next.contentChunkBase,
      previous: previous.content,
      next: next.content,
    );
    if (contentField == null) return const <StudioFieldUpdateView>[];
    fields.addAll(contentField);
    final summaryField = _chunkField(
      itemId: itemId,
      kind: 'summary',
      isSummary: true,
      base: next.summaryChunkBase,
      previous: previous.summary,
      next: next.summary,
    );
    if (summaryField == null) return const <StudioFieldUpdateView>[];
    fields.addAll(summaryField);
    return fields;
  }

  List<StudioFieldUpdateView>? _chunkField({
    required String itemId,
    required String kind,
    required bool isSummary,
    required int base,
    required List<String> previous,
    required List<String> next,
  }) {
    final deliveredKey = '$itemId|$kind';
    final delivered = _deliveredChunks[deliveredKey] ?? 0;
    // 本地列表被裁剪过，或内容不是前缀追加：只能用权威整字段替换表达。
    final growth = next.length - delivered;
    if (delivered > previous.length || growth <= 0) {
      final previousText = previous.join();
      final nextText = next.join();
      if (previousText == nextText) {
        return const <StudioFieldUpdateView>[];
      }
      _deliveredChunks[deliveredKey] = next.length;
      return [
        StudioFieldUpdateView(
          field: isSummary
              ? ThreadThinkingSummaryFieldView(base)
              : ThreadThinkingContentFieldView(base),
          change: ReplaceThreadFieldChangeView(nextText),
        ),
      ];
    }
    for (var index = delivered; index < next.length; index++) {
      if (index < previous.length && previous[index] != next[index]) {
        return null;
      }
    }
    _deliveredChunks[deliveredKey] = next.length;
    return [
      for (var index = delivered; index < next.length; index++)
        StudioFieldUpdateView(
          field: isSummary
              ? ThreadThinkingSummaryFieldView(base + index)
              : ThreadThinkingContentFieldView(base + index),
          change: AppendThreadFieldChangeView(next[index]),
        ),
    ];
  }

  StudioChatItem _chatItem(ThreadItemView item) {
    return StudioChatItem(
      item,
      saved: item.saved,
      lifecycle: item.executionTerminal
          ? StudioChatLifecycle.terminal
          : StudioChatLifecycle.streaming,
    );
  }

  @override
  Future<StudioChatReceipt> load(TimelineDirection direction) async =>
      _receipt();

  @override
  Future<StudioChatReceipt> focus(String? itemId) async {
    ChatViewMetrics.focusCalls += 1;
    final changed = _focusId != itemId;
    _focusId = itemId;
    // 与原生窗口对齐：控制操作完成后必须唤醒唯一出口，让下一次 `next` 交付权威窗口。
    _api._notifyItemsChanged(_threadId);
    return _receipt(
      disposition: changed
          ? StudioChatDisposition.applied
          : StudioChatDisposition.noChange,
    );
  }

  /// demo 的窗口条目没有字节预览预算：窗口里已经是完整正文，按身份展开只需确认身份仍在窗口。
  @override
  Future<StudioChatReceipt> expandItem(String itemId) async {
    final retained = _items.any((item) => item.id == itemId);
    return StudioChatReceipt(
      operationId: ++_operations,
      version: _api._workspaces[_threadId]?.revision ?? 0,
      retained: retained,
      disposition: StudioChatDisposition.noChange,
    );
  }

  @override
  void requestClose() {
    _closed = true;
    _api._notifyItemsChanged(_threadId);
  }

  @override
  Future<void> close() async {
    _closed = true;
    // 与原生窗口对齐：close 必须打断挂起的 next 等待，否则内容泵永久泄漏。
    _api._notifyItemsChanged(_threadId);
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
      ThreadItemKind.raw ||
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
class DriverDemoStudioTransport extends DemoStudioTransport {
  DriverDemoStudioTransport({super.lspActivityLoop});

  bool _retiredPlannerScenario = false;
  int _fixtureScenarioRevision = 0;
  int _appliedFixtureScenarioRevision = -1;

  void prepareRetiredPlannerScenario() {
    prepareSessionLifecycleScenario();
    _retiredPlannerScenario = true;
    _selectedThreadId = 'thread-main';
  }

  @override
  ({StudioProject project, List<StudioThread> threads})
  _ensureWorkspaceFixture() {
    final fixture = super._ensureWorkspaceFixture();
    if (_appliedFixtureScenarioRevision != _fixtureScenarioRevision) {
      _workspaces.update('thread-main', _driverWorkspaceFixture);
      _resolvedInteractions.clear();
      _appliedFixtureScenarioRevision = _fixtureScenarioRevision;
    }
    if (!_retiredPlannerScenario) return fixture;
    final threads = [
      for (final thread in fixture.threads)
        if (thread.id == 'thread-reviewer')
          thread.copyWith(role: 'planner', status: ThreadStatusView.idle)
        else
          thread,
    ];
    final retired = threads.singleWhere(
      (thread) => thread.id == 'thread-reviewer',
    );
    _workspaces[retired.id] = _workspaces[retired.id]!.copyWith(
      thread: retired,
      interactions: const [],
    );
    return (project: fixture.project, threads: threads);
  }

  bool _themeScenario = false;
  bool _themePlanScenario = false;

  /// Resets the dedicated Driver fixture without introducing product settings.
  void prepareThemeScenario({bool plan = false}) {
    _fixtureScenarioRevision += 1;
    _themeScenario = true;
    _themePlanScenario = plan;
    _sessionLifecycleScenario = false;
    _fallbackInputScenario = false;
    _persistenceFailureScenario = false;
    _failNextTurn = false;
    _retryNextTurn = false;
    _selectedThreadId = 'thread-main';
  }

  bool _sessionLifecycleScenario = false;
  bool _persistenceFailureScenario = false;
  bool _failNextTurn = false;
  bool _retryNextTurn = false;

  void prepareConnectionRetryScenario() {
    _retryNextTurn = true;
  }

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
    if (_retryNextTurn) {
      _retryNextTurn = false;
      for (var attempt = 1; attempt <= 5; attempt++) {
        await Future<void>.delayed(promptActivityDelay);
        if (_promptGenerations[threadId] != generation) return;
        final now = DateTime.now();
        _upsertChatItem(
          threadId,
          ThreadItemView(
            id: '$turnId:connection-retry-$attempt',
            threadId: threadId,
            turnId: turnId,
            ordinal: _nextOrdinal(threadId),
            revision: 0,
            createdAt: now,
            updatedAt: now,
            state: ThreadTextItemStateView(
              channel: ThreadTextChannel.commentary,
              text: '连接中断，正在重试（$attempt/5）。',
              attachments: const [],
              lifecycle: CompletedThreadContentView(now),
            ),
          ),
        );
      }
    }
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
    // 注入的 Turn 失败也是终态：清空当前活动。
    _clearActivity(threadId);
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
    _fixtureScenarioRevision += 1;
    _sessionLifecycleScenario = false;
    _fallbackInputScenario = false;
    _pagingFixtureCount = count;
    _pageFillThreads.clear();
    _selectedThreadId = null;
  }

  void prepareSessionLifecycleScenario() {
    _fixtureScenarioRevision += 1;
    _themeScenario = false;
    _themePlanScenario = false;
    _sessionLifecycleScenario = true;
    _fallbackInputScenario = false;
    _pageFillThreads.clear();
    _selectedThreadId = null;
  }

  void prepareFallbackInputScenario() {
    _fixtureScenarioRevision += 1;
    _sessionLifecycleScenario = false;
    _fallbackInputScenario = true;
    _pageFillThreads.clear();
    _selectedThreadId = 'thread-main';
  }

  static const _themePlanMarkdown = '''
# Siamese workspace palette

Keep the existing workspace and use cream surfaces with seal-brown actions.

## Implementation

1. Archive the approved concept and document semantic colors.
2. Apply the shared Material theme to all pages and dialogs.
3. Keep blue limited to selected markers and active status icons.

> Normal text stays warm brown and remains readable on every surface.

```dart
final theme = pureStudioTheme();
```

## Acceptance

- [x] Preserve the layout and all existing actions.
- [ ] Verify narrow windows and keyboard focus.
- [ ] Inspect [the design reference](https://example.com/design).
''';

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
  PersistenceStateSnapshot _demoPersistenceState() =>
      _persistenceFailureScenario
      ? const PersistenceStateSnapshot(
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
        )
      : super._demoPersistenceState();

  ThreadWorkspace _driverWorkspaceFixture(ThreadWorkspace workspace) {
    if (_sessionLifecycleScenario) {
      return workspace.copyWith(interactions: const [], todo: null);
    }
    const threadId = 'thread-main';
    return workspace.copyWith(
      todo: _themeScenario
          ? const TimelineTodoListUpdate(
              callId: 'driver-theme-todo',
              explanation: 'Palette acceptance',
              items: [
                TimelineTodoItem(
                  step: 'Archive the approved concept',
                  status: 'completed',
                ),
                TimelineTodoItem(
                  step: 'Apply the shared theme',
                  status: 'inProgress',
                ),
                TimelineTodoItem(
                  step: 'Inspect all interface states',
                  status: 'pending',
                ),
              ],
            )
          : workspace.todo,
      interactions: _themePlanScenario
          ? const [
              PendingInteraction(
                id: 'driver-theme-plan',
                threadId: threadId,
                turnId: 'driver-origin-turn',
                kind: InteractionKind.userInput,
                title: 'Theme implementation plan',
                body: _themePlanMarkdown,
                payload: UserInputInteractionPayload(
                  questions: [
                    UserQuestionView(
                      id: agentSessionPlanConfirmationQuestionId,
                      header: 'Plan',
                      question: _themePlanMarkdown,
                      isOther: true,
                      isSecret: false,
                      options: [
                        UserQuestionOptionView(
                          label: agentSessionPlanApproveAnswer,
                          description: 'Approve this plan.',
                        ),
                        UserQuestionOptionView(
                          label: agentSessionPlanReviseAnswer,
                          description: 'Revise this plan.',
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ]
          : _fallbackInputScenario
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
    );
  }
}

String _demoProvisionalThreadTitle(String prompt) {
  final normalized = prompt.trim().replaceAll(RegExp(r'\s+'), ' ');
  if (normalized.isEmpty) return 'New Session';
  return normalized.runes.take(80).map(String.fromCharCode).join();
}

/// demo `worktree` 会话地址：与项目目录同级的 `worktrees` 目录，仅供 GUI 验收展示。
String _demoWorktreePath(String projectPath, String threadId) =>
    '${projectPath.replaceAll('\\', '/')}-worktrees/$threadId';

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
