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
  StudioMode _sessionMode = StudioMode.simple;
  final Set<String> _archivedProjectIds = <String>{};
  final _globalEvents = StreamController<Object>.broadcast();
  final _sessionEvents = StreamController<SessionStreamFrame>.broadcast();
  final Map<String, int> _promptGenerations = {};
  int _eventSequence = 0;

  Duration get promptStartDelay => const Duration(milliseconds: 120);

  Duration get promptActivityDelay => const Duration(milliseconds: 350);

  Duration get promptToolDelay => const Duration(milliseconds: 500);

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
      mode: _sessionMode,
      updatedAt: now,
    );
    final alternateSession = StudioSession(
      id: 'session-alt',
      projectId: project.id,
      title: 'Riverpod selector audit',
      mode: _sessionMode,
      createdAt: now.subtract(const Duration(minutes: 3)),
      updatedAt: now.subtract(const Duration(minutes: 3)),
    );
    final agentSession = StudioSession(
      id: 'session-agent-reviewer',
      projectId: project.id,
      title: 'Driver reviewer',
      mode: _sessionMode,
      createdAt: now.subtract(const Duration(minutes: 7)),
      updatedAt: now.subtract(const Duration(minutes: 7)),
      parentSessionId: session.id,
      rootSessionId: session.id,
      sessionKind: StudioSessionKind.agent,
      ownerAgentId: 'driver-reviewer',
      ownerRole: 'reviewer',
      agentStatus: 'waiting',
      agentSummary: 'Verifying Driver session switching.',
      agentUpdatedAt: now.subtract(const Duration(minutes: 7)),
    );
    final userCreatedAt = now.subtract(const Duration(minutes: 9));
    final assistantCreatedAt = now.subtract(const Duration(minutes: 8));
    final alternateCreatedAt = now.subtract(const Duration(minutes: 2));
    final agentCreatedAt = now.subtract(const Duration(minutes: 6));
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
    final alternatePart = TimelinePartSnapshot(
      id: 'turn-alt:text',
      messageId: 'turn-alt:assistant',
      sessionId: alternateSession.id,
      turnId: 'turn-alt',
      type: TimelinePartType.text,
      order: 0,
      revision: 0,
      text: 'Riverpod selector boundary is isolated.',
      status: 'completed',
      createdAt: alternateCreatedAt,
      updatedAt: alternateCreatedAt,
      textChannel: TimelineTextChannel.finalAnswer,
    );
    final agentPart = TimelinePartSnapshot(
      id: 'turn-agent:text',
      messageId: 'turn-agent:assistant',
      sessionId: agentSession.id,
      turnId: 'turn-agent',
      type: TimelinePartType.text,
      order: 0,
      revision: 0,
      text: 'Driver agent workspace selected.',
      status: 'completed',
      createdAt: agentCreatedAt,
      updatedAt: agentCreatedAt,
      textChannel: TimelineTextChannel.finalAnswer,
    );
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
      sessions: [session, agentSession, alternateSession],
      selectedProjectId: project.id,
      selectedSessionId: session.id,
      permissionMode: _permissionMode,
      runtimesBySession: {
        session.id: const SessionRuntimeView(
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
        alternateSession.id: const SessionRuntimeView(
          model: 'future-model',
          contextTokens: 640,
          contextWindow: 128000,
          totalTokens: 1024,
          costLabel: 'CNY 0.01',
          activeSkills: ['riverpod-audit'],
          activeMcpServers: ['dart'],
          activeLspServers: [],
          agentCount: 1,
        ),
        agentSession.id: const SessionRuntimeView(
          model: 'reviewer/model',
          contextTokens: 320,
          contextWindow: 128000,
          totalTokens: 512,
          costLabel: 'CNY 0.01',
          activeSkills: ['driver-verification'],
          activeMcpServers: ['dart'],
          activeLspServers: [],
          agentCount: 0,
        ),
      },
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
        alternateSession.id: [
          TimelineMessage(
            id: 'turn-alt:assistant',
            sessionId: alternateSession.id,
            role: 'assistant',
            createdAt: alternateCreatedAt,
          ),
        ],
        agentSession.id: [
          TimelineMessage(
            id: 'turn-agent:assistant',
            sessionId: agentSession.id,
            role: 'assistant',
            createdAt: agentCreatedAt,
          ),
        ],
      },
      partSnapshotsBySession: {
        session.id: {for (final part in demoParts) part.id: part},
        alternateSession.id: {alternatePart.id: alternatePart},
        agentSession.id: {agentPart.id: agentPart},
      },
      workspaceSyncBySession: {
        session.id: AgentWorkspaceSyncState.ready,
        alternateSession.id: AgentWorkspaceSyncState.ready,
        agentSession.id: AgentWorkspaceSyncState.ready,
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
        pendingInteractions: const [],
      );
    }
    return state;
  }

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
  Future<StudioState> createSession(String projectId, {String? title}) =>
      bootstrap();

  @override
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  }) => bootstrap();

  @override
  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(
    String issueId,
  ) async {
    return RecoveryCleanupPreview(
      issueId: issueId,
      expectedRevision: 'demo',
      scope: RecoveryIssueScope.session,
      detail: 'Demo recovery issue',
      resources: const [],
    );
  }

  @override
  Future<StudioState> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision, {
    String? selectedProjectId,
    String? selectedSessionId,
  }) => bootstrap();

  @override
  Future<StudioSession> setSessionMode(
    String sessionId,
    StudioMode mode,
  ) async {
    _sessionMode = mode;
    final state = await bootstrap();
    return state.sessions.firstWhere((session) => session.id == sessionId);
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
  Future<InteractionResolutionResult> resolveInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  ) async {
    if (resolution is PlanConfirmationResolutionCommand &&
        resolution.decision == PlanConfirmationDecision.implementFreshContext) {
      _sessionMode = StudioMode.task;
    }
    final state = await bootstrap();
    return InteractionResolutionResult(
      sessionId: state.selectedSessionId ?? '',
      interactionId: interactionId,
      status: 'resolved',
      sessions: state.sessions,
    );
  }

  @override
  Future<void> stopPrompt(String sessionId) async {
    _promptGenerations.update(
      sessionId,
      (value) => value + 1,
      ifAbsent: () => 1,
    );
    final now = DateTime.now();
    _emitSessionEvent(
      sessionId: sessionId,
      payload: TurnChangedPayload(
        turn: StudioTurnView(
          turnId: 'demo-turn-$_eventSequence',
          sessionId: sessionId,
          state: const StudioTurnState.cancelled('Stopped in demo mode'),
          updatedAt: now,
        ),
      ),
    );
  }

  @override
  Stream<Object> subscribeProductEvents() => _globalEvents.stream;

  @override
  Stream<SessionStreamFrame> subscribeSessionEvents(
    String sessionId, {
    int? afterSequence,
  }) => _sessionEvents.stream;

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
    final promptGeneration = _promptGenerations.update(
      sessionId,
      (value) => value + 1,
      ifAbsent: () => 1,
    );
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final turnId = 'demo-turn-$_eventSequence';
    final userMessageId = 'demo-user-$_eventSequence';
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessageUpdatedPayload(
        message: _demoTimelineMessage(
          id: userMessageId,
          sessionId: sessionId,
          turnId: turnId,
          role: 'user',
          createdAt: now,
        ),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: _demoTimelinePart(
          id: '$userMessageId:text',
          messageId: userMessageId,
          sessionId: sessionId,
          turnId: turnId,
          type: TimelinePartType.text,
          order: 0,
          revision: 0,
          status: 'completed',
          createdAt: now,
          textChannel: TimelineTextChannel.user,
          text: trimmed,
        ),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: TurnChangedPayload(
        turn: StudioTurnView(
          turnId: turnId,
          sessionId: sessionId,
          state: const StudioTurnState.inProgress(StudioTurnActivity.thinking),
          updatedAt: DateTime.fromMillisecondsSinceEpoch(now * 1000),
        ),
      ),
    );
    await Future<void>.delayed(promptStartDelay);
    if (_promptGenerations[sessionId] != promptGeneration) {
      return;
    }
    final assistantMessageId = 'demo-assistant-$_eventSequence';
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessageUpdatedPayload(
        message: _demoTimelineMessage(
          id: assistantMessageId,
          sessionId: sessionId,
          turnId: turnId,
          role: 'assistant',
          createdAt: now + 1,
        ),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: _demoTimelinePart(
          id: '$assistantMessageId:reasoning-1',
          messageId: assistantMessageId,
          sessionId: sessionId,
          turnId: turnId,
          type: TimelinePartType.reasoning,
          order: 0,
          revision: 0,
          status: 'streaming',
          createdAt: now + 1,
          reasoningSummary: const ['## Inspecting the request'],
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[sessionId] != promptGeneration) {
      return;
    }
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartDeltaPayload(
        delta: TimelinePartDelta(
          partId: '$assistantMessageId:reasoning-1',
          revision: 1,
          field: 'reasoning.summary',
          delta: '\n\nChecking the live timeline projection.',
          chunkIndex: 0,
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[sessionId] != promptGeneration) {
      return;
    }
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: _demoTimelinePart(
          id: '$assistantMessageId:reasoning-1',
          messageId: assistantMessageId,
          sessionId: sessionId,
          turnId: turnId,
          type: TimelinePartType.reasoning,
          order: 0,
          revision: 2,
          status: 'completed',
          createdAt: now + 1,
          reasoningSummary: const [
            '## Inspecting the request',
            'Checking the live timeline projection.',
          ],
        ),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: _demoTimelinePart(
          id: '$assistantMessageId:reasoning-2',
          messageId: assistantMessageId,
          sessionId: sessionId,
          turnId: turnId,
          type: TimelinePartType.reasoning,
          order: 1,
          revision: 0,
          status: 'streaming',
          createdAt: now + 1,
          reasoningContent: const ['## Preparing the tool call'],
        ),
      ),
    );
    await Future<void>.delayed(promptActivityDelay);
    if (_promptGenerations[sessionId] != promptGeneration) {
      return;
    }
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: _demoTimelinePart(
          id: '$assistantMessageId:reasoning-2',
          messageId: assistantMessageId,
          sessionId: sessionId,
          turnId: turnId,
          type: TimelinePartType.reasoning,
          order: 1,
          revision: 1,
          status: 'completed',
          createdAt: now + 1,
          reasoningSummary: const [
            '## Preparing the tool call',
            'Selecting the smallest verification command.',
          ],
        ),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: TurnChangedPayload(
        turn: StudioTurnView(
          turnId: turnId,
          sessionId: sessionId,
          state: const StudioTurnState.inProgress(
            StudioTurnActivity.runningTool,
          ),
          updatedAt: DateTime.fromMillisecondsSinceEpoch(now * 1000),
        ),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: _demoTimelinePart(
          id: '$assistantMessageId:tool',
          messageId: assistantMessageId,
          sessionId: sessionId,
          turnId: turnId,
          type: TimelinePartType.tool,
          order: 2,
          revision: 0,
          status: 'running',
          createdAt: now + 1,
          tool: TimelineToolPart(
            toolCallId: '$assistantMessageId:tool-call',
            name: 'exec',
            arguments: jsonEncode({
              'command': 'flutter test test/widget_test.dart',
            }),
          ),
        ),
      ),
    );
    await Future<void>.delayed(promptToolDelay);
    if (_promptGenerations[sessionId] != promptGeneration) {
      return;
    }
    unawaited(
      Future<void>.delayed(Duration.zero, () {
        _emitSessionEvent(
          sessionId: sessionId,
          payload: MessagePartUpdatedPayload(
            part: _demoTimelinePart(
              id: '$assistantMessageId:tool',
              messageId: assistantMessageId,
              sessionId: sessionId,
              turnId: turnId,
              type: TimelinePartType.tool,
              order: 2,
              revision: 1,
              status: 'completed',
              createdAt: now + 1,
              tool: TimelineToolPart(
                toolCallId: '$assistantMessageId:tool-call',
                name: 'exec',
                arguments: jsonEncode({
                  'command': 'flutter test test/widget_test.dart',
                }),
                result: 'All widget tests passed.',
              ),
            ),
          ),
        );
        _emitSessionEvent(
          sessionId: sessionId,
          payload: TurnChangedPayload(
            turn: StudioTurnView(
              turnId: turnId,
              sessionId: sessionId,
              state: const StudioTurnState.inProgress(
                StudioTurnActivity.responding,
              ),
              updatedAt: DateTime.fromMillisecondsSinceEpoch(now * 1000),
            ),
          ),
        );
        _emitSessionEvent(
          sessionId: sessionId,
          payload: MessagePartUpdatedPayload(
            part: _demoTimelinePart(
              id: '$assistantMessageId:text',
              messageId: assistantMessageId,
              sessionId: sessionId,
              turnId: turnId,
              type: TimelinePartType.text,
              order: 3,
              revision: 0,
              status: 'completed',
              createdAt: now + 1,
              textChannel: TimelineTextChannel.finalAnswer,
              text:
                  'Demo response for: **$trimmed**\n\n'
                  '- Reasoning summaries update in one activity row\n'
                  '- Tool activity takes over without duplicating history',
            ),
          ),
        );
        _emitSessionEvent(
          sessionId: sessionId,
          payload: TurnChangedPayload(
            turn: StudioTurnView(
              turnId: turnId,
              sessionId: sessionId,
              state: const StudioTurnState.completed(),
              updatedAt: DateTime.fromMillisecondsSinceEpoch(now * 1000),
            ),
          ),
        );
      }),
    );
  }

  @override
  Future<StudioState> saveRuntimePermissionMode(PermissionMode mode) async {
    _permissionMode = mode;
    return bootstrap();
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async => _providerCatalog;

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
  Future<StudioState> saveMcpSettings(McpSettingsCommand command) async {
    return bootstrap();
  }

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

  void _emitSessionEvent({
    required String sessionId,
    required StudioBridgeEventPayload payload,
  }) {
    _eventSequence += 1;
    _sessionEvents.add(
      SessionEventFrame(
        event: StudioBridgeEvent(
          payload: payload,
          sessionId: sessionId,
          sequence: BigInt.from(_eventSequence),
          createdAt: DateTime.now(),
        ),
      ),
    );
  }
}

TimelineMessage _demoTimelineMessage({
  required String id,
  required String sessionId,
  required String turnId,
  required String role,
  required int createdAt,
}) {
  final timestamp = DateTime.fromMillisecondsSinceEpoch(createdAt * 1000);
  return TimelineMessage(
    id: id,
    sessionId: sessionId,
    turnId: turnId,
    role: role,
    createdAt: timestamp,
    updatedAt: timestamp,
  );
}

TimelinePartSnapshot _demoTimelinePart({
  required String id,
  required String messageId,
  required String sessionId,
  required String turnId,
  required TimelinePartType type,
  required int order,
  required int revision,
  required String status,
  required int createdAt,
  String text = '',
  List<String> reasoningSummary = const [],
  List<String> reasoningContent = const [],
  TimelineTextChannel? textChannel,
  TimelineToolPart? tool,
}) {
  final timestamp = DateTime.fromMillisecondsSinceEpoch(createdAt * 1000);
  return TimelinePartSnapshot(
    id: id,
    messageId: messageId,
    sessionId: sessionId,
    turnId: turnId,
    type: type,
    order: order,
    revision: revision,
    text: text,
    reasoningSummary: reasoningSummary,
    reasoningContent: reasoningContent,
    status: status,
    createdAt: timestamp,
    updatedAt: timestamp,
    textChannel: textChannel,
    tool: tool,
  );
}

/// Deterministic demo fixture exposed only by the dedicated Driver build.
///
/// `cargo xtask run-gui --demo --driver` enables this fixture through the
/// `PURE_STUDIO_DRIVER` compile-time define. Production and release entrypoints
/// never set that define.
class DriverDemoStudioApi extends DemoStudioApi {
  @override
  Duration get promptActivityDelay => const Duration(seconds: 3);

  @override
  Duration get promptToolDelay => const Duration(seconds: 3);

  @override
  Future<StudioState> bootstrap() async {
    final state = await super.bootstrap();
    return state.copyWith(
      pendingInteractions: const [
        PendingInteraction(
          id: 'driver-tool',
          sessionId: 'session-main',
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
          sessionId: 'session-main',
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
          sessionId: 'session-main',
          kind: InteractionKind.planConfirmation,
          title: 'Confirm demo plan',
          body: 'Implement the deterministic demo plan.',
          payload: PlanConfirmationInteractionPayload(
            planId: 'driver-plan',
            content: '1. Verify stable Driver keys.',
          ),
        ),
      ],
    );
  }
}
