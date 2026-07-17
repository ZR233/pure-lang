part of 'studio_api.dart';

class DemoStudioApi implements StudioApi {
  DemoStudioApi({this._providerCatalog = demoProviderCatalogFixture});

  final ProviderCatalogView _providerCatalog;
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
  final Set<String> _archivedProjectIds = <String>{};
  final Map<String, Map<String, Object?>> _settingsDrafts = {};
  final _globalEvents = StreamController<Object>.broadcast();
  final _sessionEvents = StreamController<Object>.broadcast();
  int _eventSequence = 0;

  @override
  Future<StudioState> bootstrap() async {
    final now = DateTime.now();
    const project = StudioProject(
      id: 'project-local',
      name: 'pure-lang',
      path: r'C:\Users\zhoudongsheng\.codex\worktrees\3bc1\pure-lang',
    );
    final session = StudioSession(
      id: 'session-main',
      projectId: project.id,
      title: 'Flutter + FRB 重构',
      mode: StudioMode.simple,
      updatedAt: now,
    );
    final userCreatedAt = now.subtract(const Duration(minutes: 9));
    final assistantCreatedAt = now.subtract(const Duration(minutes: 8));
    final demoParts = [
      TimelinePartSnapshot(
        id: 'turn-demo:user-text',
        messageId: 'turn-demo:user',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.text,
        order: 0,
        revision: 0,
        text:
            '用 Flutter 重构 Pure Studio。\n\n'
            '- timeline 要像 Web 版一样即时渲染 Markdown\n'
            '- streaming 中的代码块和表格不要抖动',
        status: 'completed',
        createdAt: userCreatedAt,
        updatedAt: userCreatedAt,
        textChannel: TimelineTextChannel.user,
      ),
      TimelinePartSnapshot(
        id: 'turn-demo:reasoning-1',
        messageId: 'turn-demo:assistant',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.reasoning,
        order: 0,
        revision: 0,
        text:
            '## 判断\n\n'
            '> UI 只消费当前会话的高频事件，后台会话不应该继续推 delta。\n\n'
            '- `messagePartDelta` 只作为 live overlay\n'
            '- terminal snapshot 到达后覆盖未完成文本',
        status: 'completed',
        createdAt: assistantCreatedAt,
        updatedAt: assistantCreatedAt,
      ),
      TimelinePartSnapshot(
        id: 'turn-demo:tool-1',
        messageId: 'turn-demo:assistant',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.tool,
        order: 1,
        revision: 0,
        text: '',
        status: 'completed',
        createdAt: assistantCreatedAt,
        updatedAt: assistantCreatedAt,
        tool: const TimelineToolPart(
          toolCallId: 'turn-demo:tool-call-1',
          name: 'cargo test -p pl-studio-bridge',
          result: '1 passed; bridge envelope uses typed payload.',
        ),
      ),
      TimelinePartSnapshot(
        id: 'turn-demo:plan-1',
        messageId: 'turn-demo:assistant',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.plan,
        order: 2,
        revision: 0,
        text: '',
        status: 'completed',
        createdAt: assistantCreatedAt,
        updatedAt: assistantCreatedAt,
        planContent:
            '## Implementation checklist\n\n'
            '1. Keep the Flutter shell aligned with runtime contracts.\n'
            '2. Use Riverpod selectors for derived views.\n'
            '3. Subscribe only the selected session stream.\n'
            '4. Verify Markdown in streaming mode.\n\n'
            '| Area | Status |\n'
            '| --- | --- |\n'
            '| FRB runtime | ready |\n'
            '| Timeline Markdown | streaming |\n\n'
            '```text\n'
            'WeatherDay>```\n\n'
            '## Inline fence recovery\n\n'
            '| Renderer | Result |\n'
            '| --- | --- |\n'
            '| Timeline | headings and tables stay live |',
      ),
      TimelinePartSnapshot(
        id: 'turn-demo:final-1',
        messageId: 'turn-demo:assistant',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.text,
        order: 3,
        revision: 0,
        text:
            '### Streaming Markdown preview\n\n'
            '正文、**加粗**、`inline code` 和链接都应该按 GFM 渲染。\n\n'
            '- text / plan / reasoning 走同一个 renderer\n'
            '- fenced code block 即使还没收到结束 fence，也应该显示成代码块\n\n'
            '```dart\n'
            'final stream = subscribeSessionEvents(sessionId);\n'
            'await for (final event in stream) {\n'
            '  reducer.apply(event);\n'
            '}',
        status: 'completed',
        createdAt: assistantCreatedAt,
        updatedAt: assistantCreatedAt,
        textChannel: TimelineTextChannel.finalAnswer,
      ),
    ];
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
    final state = StudioState(
      projects: const [project],
      sessions: [session],
      selectedProjectId: project.id,
      selectedSessionId: session.id,
      permissionMode: _permissionMode,
      turnPhase: TurnPhase.idle,
      runtime: const SessionRuntimeView(
        model: 'planner/local-responses',
        contextTokens: 18342,
        contextWindow: 128000,
        totalTokens: 26320,
        costLabel: 'CNY 0.16',
        activeSkills: [
          'flutter-apply-architecture-best-practices',
          'verification-before-completion',
        ],
        activeMcpServers: ['dart'],
        activeLspServers: ['rust-analyzer'],
        agentCount: 4,
      ),
      messagesBySession: {
        session.id: [
          TimelineMessage(
            id: 'turn-demo:user',
            sessionId: session.id,
            role: 'user',
            createdAt: userCreatedAt,
          ),
          TimelineMessage(
            id: 'turn-demo:assistant',
            sessionId: session.id,
            role: 'assistant',
            createdAt: assistantCreatedAt,
          ),
        ],
      },
      partSnapshotsBySession: {
        session.id: {for (final part in demoParts) part.id: part},
      },
      agentsBySession: {
        session.id: {
          'agent-reviewer': StudioAgentView(
            id: 'agent-reviewer',
            sessionId: session.id,
            path: 'root/reviewer',
            role: 'reviewer',
            task: 'Audit timeline projection',
            status: 'running',
            summary: '已审阅 2 个文件，正在核对 row projection 排序逻辑',
            updatedAt: assistantCreatedAt,
          ),
          'agent-reviewer-lint': StudioAgentView(
            id: 'agent-reviewer-lint',
            sessionId: session.id,
            path: 'root/reviewer/lint',
            parentPath: 'root/reviewer',
            role: 'lint',
            task: 'Run clippy on pl-core',
            status: 'queued',
            depth: 1,
            updatedAt: assistantCreatedAt,
          ),
          'agent-worker': StudioAgentView(
            id: 'agent-worker',
            sessionId: session.id,
            path: 'root/worker',
            role: 'worker',
            task: 'Implement visible progress',
            status: 'completed',
            summary: 'Patched Flutter projection，timeline 行已按 sequence 排序',
            updatedAt: assistantCreatedAt,
          ),
          'agent-worker-test': StudioAgentView(
            id: 'agent-worker-test',
            sessionId: session.id,
            path: 'root/worker/test',
            parentPath: 'root/worker',
            role: 'test',
            task: 'Run cargo test -p pl-core',
            status: 'errored',
            depth: 1,
            error: '3 tests failed: timeline projection ordering mismatch',
            updatedAt: assistantCreatedAt,
          ),
        },
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
      pendingInteractions: const [],
    );
    if (_archivedProjectIds.contains(project.id)) {
      return StudioState(
        projects: const [],
        sessions: const [],
        messagesBySession: const {},
        agentsBySession: const {},
        providers: state.providers,
        providerCatalog: _providerCatalog,
        roles: state.roles,
        mcpServers: state.mcpServers,
        instructions: state.instructions,
        skills: state.skills,
        general: state.general,
        webSearch: state.webSearch,
        selectedProjectId: null,
        selectedSessionId: null,
        permissionMode: state.permissionMode,
        turnPhase: TurnPhase.idle,
        runtime: state.runtime,
        pendingInteractions: const [],
      );
    }
    return state;
  }

  @override
  Future<StudioState> loadSessionState(String sessionId) => bootstrap();

  @override
  Future<StudioState> openProject(String path) {
    _archivedProjectIds.remove('project-local');
    return bootstrap();
  }

  @override
  Future<StudioState> selectProject(String projectId) => bootstrap();

  @override
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  }) {
    _archivedProjectIds.add(projectId);
    return bootstrap();
  }

  @override
  Future<StudioState> createSession(String projectId, {String? title}) =>
      bootstrap();

  @override
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  }) => bootstrap();

  @override
  Future<StudioState> setSessionMode(String sessionId, StudioMode mode) async {
    final state = await bootstrap();
    return state.copyWith(
      sessions: [
        for (final session in state.sessions)
          session.id == sessionId ? session.copyWith(mode: mode) : session,
      ],
    );
  }

  @override
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedSessionId,
  }) async {
    final state = await bootstrap();
    final roles = [
      for (final role in state.roles)
        role.key == roleKey
            ? RoleSettingsView(
                key: role.key,
                providerId: providerId,
                model: model,
                effort: effort ?? role.effort,
              )
            : role,
    ];
    return state.copyWith(roles: roles);
  }

  @override
  Future<List<StudioBridgeEvent>> loadStudioEvents(
    String sessionId, {
    int? afterSequence,
    int limit = 500,
  }) async => const [];

  @override
  Future<void> resolveInteraction(
    String interactionId,
    Map<String, Object?> resolution,
  ) async {}

  @override
  Future<void> stopPrompt(String sessionId) async {
    _emitSessionEvent(
      sessionId: sessionId,
      payload: TurnChangedPayload(
        turn: StudioTurnView(sessionId: sessionId, status: 'cancelled'),
      ),
    );
  }

  @override
  Stream<Object> subscribeGlobalEvents() => _globalEvents.stream;

  @override
  Stream<Object> subscribeSessionEvents(String sessionId) =>
      _sessionEvents.stream;

  @override
  Future<void> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  ) async {
    final trimmed = prompt.trim();
    if (trimmed.isEmpty) {
      return;
    }
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final userMessageId = 'demo-user-$_eventSequence';
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessageUpdatedPayload(
        message: timelineMessageFromJson({
          'messageId': userMessageId,
          'sessionId': sessionId,
          'role': 'user',
          'createdAt': now,
        }),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: timelinePartSnapshotFromJson({
          'partId': '$userMessageId:text',
          'messageId': userMessageId,
          'sessionId': sessionId,
          'partType': 'text',
          'order': 0,
          'revision': 0,
          'status': 'completed',
          'createdAt': now,
          'updatedAt': now,
          'textChannel': 'user',
          'text': trimmed,
        }),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: TurnChangedPayload(
        turn: StudioTurnView(sessionId: sessionId, status: 'streaming'),
      ),
    );
    await Future<void>.delayed(const Duration(milliseconds: 120));
    final assistantMessageId = 'demo-assistant-$_eventSequence';
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessageUpdatedPayload(
        message: timelineMessageFromJson({
          'messageId': assistantMessageId,
          'sessionId': sessionId,
          'role': 'assistant',
          'createdAt': now + 1,
        }),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: timelinePartSnapshotFromJson({
          'partId': '$assistantMessageId:text',
          'messageId': assistantMessageId,
          'sessionId': sessionId,
          'partType': 'text',
          'order': 1,
          'revision': 0,
          'status': 'completed',
          'createdAt': now + 1,
          'updatedAt': now + 1,
          'textChannel': 'final',
          'text':
              'Demo response for: **$trimmed**\n\n'
              '- FRB session stream is connected\n'
              '- Markdown renders through the live timeline path',
        }),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: TurnChangedPayload(
        turn: StudioTurnView(sessionId: sessionId, status: 'completed'),
      ),
    );
  }

  @override
  Future<void> saveRuntimePermissionMode(PermissionMode mode) async {
    _permissionMode = mode;
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async => _providerCatalog;

  @override
  Future<StudioState> saveProviderSettings(
    Map<String, Object?> settings,
  ) async {
    final current = await bootstrap();
    _providers = _providersFromSettingsPayload(
      settings,
      previous: current.providers,
      catalog: _providerCatalog,
    );
    _roles = _rolesFromSettingsPayload(settings);
    return bootstrap();
  }

  @override
  Future<StudioState> saveInstructionsSettings(
    Map<String, Object?> settings,
  ) async {
    _instructions = _instructionsFromSettingsPayload(settings);
    return bootstrap();
  }

  @override
  Future<StudioState> saveSkillsSettings(Map<String, Object?> settings) async {
    _skills = _skillsFromSettingsPayload(settings);
    return bootstrap();
  }

  @override
  Future<StudioState> saveMcpSettings(Map<String, Object?> settings) async {
    return bootstrap();
  }

  @override
  Future<StudioState> saveGeneralSettings(Map<String, Object?> settings) async {
    _general = _generalFromJson(settings);
    return bootstrap();
  }

  @override
  Future<StudioState> saveWebSearchSettings(
    WebSearchSettingsView settings,
  ) async {
    _webSearch = WebSearchSettingsView(
      configuredMode: settings.configuredMode,
      effectiveMode: settings.configuredMode,
      availability: settings.configuredMode == 'disabled'
          ? 'disabled'
          : 'available',
      contextSize: settings.contextSize,
      allowedDomains: settings.allowedDomains,
      country: settings.country,
      region: settings.region,
      city: settings.city,
      timezone: settings.timezone,
      providerId: 'openai',
      model: 'gpt-5',
    );
    return bootstrap();
  }

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async {
    final state = await bootstrap();
    return [
      for (final provider in state.providers) _demoProviderUsage(provider),
    ];
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    if (_archivedProjectIds.contains(projectId)) {
      return const [];
    }
    return const ['flutter-ui-polish', 'runtime-review', 'studio-settings'];
  }

  @override
  Future<void> saveStudioSettingsDraft(
    String section,
    Map<String, Object?> draft,
  ) async {
    _settingsDrafts[section] = Map<String, Object?>.from(draft);
    _globalEvents.add(
      StudioBridgeEvent(
        payload: SettingsDraftSavedPayload(section: section, saved: true),
      ),
    );
  }

  void _emitSessionEvent({
    required String sessionId,
    required StudioBridgeEventPayload payload,
  }) {
    _eventSequence += 1;
    _sessionEvents.add(
      StudioBridgeEvent(
        payload: payload,
        sessionId: sessionId,
        sequence: BigInt.from(_eventSequence),
        createdAt: DateTime.now(),
      ),
    );
  }
}
