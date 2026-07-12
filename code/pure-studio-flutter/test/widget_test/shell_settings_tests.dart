part of '../widget_test.dart';

void registerShellSettingsTests() {
  testWidgets('sidebar session actions call Studio API', (tester) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_emptyState());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('New session'));
    await tester.pump();
    expect(api.createSessionCount, 1);

    await tester.tap(find.byTooltip('Archive session'));
    await tester.pump();
    expect(api.archivedSessionId, 'session-1');
  });

  testWidgets('sidebar footer uses aligned icon actions in zh Hans', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(900, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_emptyState());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(
          locale: const Locale('zh', 'Hans'),
          home: const StudioShell(),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final sidebar = find.byKey(const ValueKey('studio-sidebar'));
    final newSession = find.widgetWithIcon(
      IconButton,
      Icons.add_comment_outlined,
    );
    final openProject = find.widgetWithIcon(
      IconButton,
      Icons.create_new_folder,
    );
    final settings = find.widgetWithIcon(IconButton, Icons.settings);

    expect(sidebar, findsOneWidget);
    expect(find.byTooltip('新建会话'), findsOneWidget);
    expect(find.byTooltip('打开项目'), findsOneWidget);
    expect(find.byTooltip('设置'), findsOneWidget);
    expect(newSession, findsOneWidget);
    expect(openProject, findsOneWidget);
    expect(settings, findsOneWidget);
    expect(tester.getSize(newSession), const Size.square(40));
    expect(tester.getSize(openProject), const Size.square(40));
    expect(tester.getSize(settings), const Size.square(40));
    expect(tester.getCenter(newSession).dy, tester.getCenter(openProject).dy);
    expect(tester.getCenter(openProject).dy, tester.getCenter(settings).dy);
    expect(
      find.descendant(of: sidebar, matching: find.byType(OutlinedButton)),
      findsNothing,
    );
    expect(find.text('新建'), findsNothing);
    expect(find.text('打开'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('project close buttons respect current session busy state', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _twoProjectState(
        selectedProjectId: 'project-a',
        turnPhase: TurnPhase.streaming,
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump();
    await tester.pump();

    final closeProjectButtons = find.widgetWithIcon(IconButton, Icons.close);
    final closeButtons = tester
        .widgetList<IconButton>(closeProjectButtons)
        .toList();
    expect(closeButtons.length, 2);
    expect(closeButtons.first.onPressed, isNull);
    expect(closeButtons.last.onPressed, isNotNull);

    await tester.tap(closeProjectButtons.last);
    await tester.pump();
    await tester.pump();
    expect(api.archivedProjectId, 'project-b');
  });

  testWidgets('status bar routes model controls by session mode', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final updatedAt = DateTime.fromMillisecondsSinceEpoch(1);
    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        runtime: const SessionRuntimeView(
          model: 'planner/local',
          contextTokens: 42,
          contextWindow: 100,
          totalTokens: 128,
          costLabel: 'CNY 0.16',
          activeSkills: ['flutter-ui'],
          activeMcpServers: ['dart'],
          activeLspServers: ['rust-analyzer'],
          agentCount: 2,
        ),
        agentsBySession: {
          'session-1': {
            'agent-reviewer': StudioAgentView(
              id: 'agent-reviewer',
              sessionId: 'session-1',
              path: 'root/reviewer',
              role: 'reviewer',
              task: 'Audit timeline projection',
              status: 'running',
              summary: 'Checking status projection',
              updatedAt: updatedAt,
            ),
            'agent-worker': StudioAgentView(
              id: 'agent-worker',
              sessionId: 'session-1',
              path: 'root/worker',
              role: 'worker',
              task: 'Patch Flutter status bar',
              status: 'completed',
              depth: 1,
              updatedAt: updatedAt,
            ),
          },
        },
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byTooltip('Session mode'), findsOneWidget);
    expect(find.byTooltip('Executor model'), findsOneWidget);
    expect(find.byTooltip('Planner model'), findsNothing);
    expect(find.byTooltip('Reasoning effort'), findsOneWidget);
    expect(find.byType(StatusBarItem), findsWidgets);
    final contextReadout = find.bySemanticsLabel('Context');
    expect(contextReadout, findsOneWidget);
    expect(
      find.descendant(of: contextReadout, matching: find.byType(CustomPaint)),
      findsOneWidget,
    );
    expect(find.text('42%'), findsNothing);
    expect(find.text('42/100'), findsNothing);
    expect(find.text('CNY 0.16'), findsOneWidget);
    expect(find.text('1 skill · 1 MCP · 1 LSP · 2 agents'), findsOneWidget);
    expect(find.text('1 skill · 1 MCP · 1 LSP'), findsNothing);
    expect(find.text('2 agents · 1 running'), findsNothing);

    final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
    addTearDown(gesture.removePointer);
    await gesture.addPointer();
    await gesture.moveTo(tester.getCenter(find.bySemanticsLabel('Context')));
    await tester.pumpAndSettle();
    expect(find.text('42%'), findsOneWidget);
    expect(find.text('42 / 100'), findsOneWidget);
    expect(find.text('128'), findsOneWidget);
    expect(find.text('planner/local'), findsOneWidget);
    await gesture.moveTo(Offset.zero);
    await tester.pumpAndSettle();
    await gesture.removePointer();

    await tester.tap(find.text('CNY 0.16'));
    await tester.pumpAndSettle();
    expect(find.text('2 agents'), findsNothing);
    await tester.tapAt(Offset.zero);
    await tester.pumpAndSettle();

    final activityFinder = find.text('1 skill · 1 MCP · 1 LSP · 2 agents');
    final activityCenter = tester.getCenter(activityFinder);
    final activityRect = tester.getRect(activityFinder);
    await tester.tapAt(Offset(activityRect.left + 8, activityCenter.dy));
    await tester.pumpAndSettle();
    expect(find.text('ACTIVE CAPABILITIES'), findsOneWidget);
    expect(find.textContaining('Skills · flutter-ui'), findsOneWidget);
    expect(find.textContaining('MCP · dart'), findsOneWidget);
    expect(find.textContaining('LSP · rust-analyzer'), findsOneWidget);
    expect(find.text('SUBAGENTS'), findsOneWidget);
    expect(find.text('reviewer'), findsOneWidget);
    expect(find.text('worker'), findsOneWidget);
    expect(find.text('Running'), findsOneWidget);
    await tester.tap(find.text('reviewer'));
    await tester.pumpAndSettle();
    expect(find.textContaining('Checking status projection'), findsOneWidget);
    await tester.tapAt(Offset.zero);
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Session mode'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Task').last);
    await tester.pumpAndSettle();
    expect(api.sessionModeUpdate, CompileMode.task);
    api.emitGlobal(
      _sessionListChangedEvent(
        projectId: 'project-1',
        sessions: [
          StudioSession(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Session',
            mode: CompileMode.task,
            updatedAt: DateTime.fromMillisecondsSinceEpoch(1000),
          ),
        ],
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Task'), findsOneWidget);
    expect(find.byTooltip('Planner model'), findsOneWidget);

    await tester.tap(find.byTooltip('Planner model'));
    await tester.pumpAndSettle();
    await tester.tap(find.textContaining('Reasoner').last);
    await tester.pumpAndSettle();
    expect(api.roleUpdate?.roleKey, 'planner');
    expect(api.roleUpdate?.providerId, 'deepseek');
    expect(api.roleUpdate?.model, 'deepseek-reasoner');

    await tester.tap(find.byTooltip('Reasoning effort'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('max').last);
    await tester.pumpAndSettle();
    expect(api.roleUpdate?.roleKey, 'planner');
    expect(api.roleUpdate?.providerId, 'deepseek');
    expect(api.roleUpdate?.model, 'deepseek-reasoner');
    expect(api.roleUpdate?.effort, 'max');
  });

  testWidgets('status bar localizes every turn phase', (tester) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final labelsByLocale = {
      const Locale('en'): const {
        TurnPhase.idle: 'Idle',
        TurnPhase.queued: 'Queued',
        TurnPhase.contextLoading: 'Loading context',
        TurnPhase.waitingForModel: 'Waiting for model',
        TurnPhase.streaming: 'Streaming',
        TurnPhase.waitingForInteraction: 'Waiting for interaction',
        TurnPhase.runningTool: 'Running tool',
        TurnPhase.completed: 'Completed',
        TurnPhase.failed: 'Failed',
        TurnPhase.cancelled: 'Cancelled',
      },
      const Locale('zh', 'Hans'): const {
        TurnPhase.idle: '空闲',
        TurnPhase.queued: '排队中',
        TurnPhase.contextLoading: '加载上下文',
        TurnPhase.waitingForModel: '等待模型',
        TurnPhase.streaming: '正在生成',
        TurnPhase.waitingForInteraction: '等待交互',
        TurnPhase.runningTool: '运行工具',
        TurnPhase.completed: '已完成',
        TurnPhase.failed: '失败',
        TurnPhase.cancelled: '已取消',
      },
    };

    for (final localeEntry in labelsByLocale.entries) {
      for (final phaseEntry in localeEntry.value.entries) {
        await tester.pumpWidget(
          ProviderScope(
            child: _localizedApp(
              locale: localeEntry.key,
              home: SessionStatusBar(
                state: _emptyState().copyWith(turnPhase: phaseEntry.key),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();

        expect(
          find.text(phaseEntry.value),
          findsOneWidget,
          reason: '${localeEntry.key} ${phaseEntry.key}',
        );
        expect(find.text(phaseEntry.key.name), findsNothing);
      }
    }
  });

  testWidgets('header does not duplicate the localized running phase', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(turnPhase: TurnPhase.streaming),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Streaming'), findsOneWidget);
    expect(find.text(TurnPhase.streaming.name), findsNothing);
  });

  testWidgets('status bar localizes active interaction before turn phase', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final labelsByLocale = {
      const Locale('en'): const {
        InteractionKind.toolApproval: 'Waiting for tool approval',
        InteractionKind.userInput: 'Waiting for input',
        InteractionKind.planConfirmation: 'Waiting for plan confirmation',
      },
      const Locale('zh', 'Hans'): const {
        InteractionKind.toolApproval: '等待工具授权',
        InteractionKind.userInput: '等待输入',
        InteractionKind.planConfirmation: '等待计划确认',
      },
    };

    for (final localeEntry in labelsByLocale.entries) {
      for (final interactionEntry in localeEntry.value.entries) {
        final state = _emptyState().copyWith(
          turnPhase: TurnPhase.streaming,
          pendingInteractions: [
            PendingInteraction(
              id: 'interaction-${interactionEntry.key.name}',
              sessionId: 'session-1',
              kind: interactionEntry.key,
              title: 'Pending',
              body: 'Pending interaction',
            ),
          ],
        );
        await tester.pumpWidget(
          ProviderScope(
            child: _localizedApp(
              locale: localeEntry.key,
              home: SessionStatusBar(state: state),
            ),
          ),
        );
        await tester.pumpAndSettle();

        expect(
          find.text(interactionEntry.value),
          findsOneWidget,
          reason: '${localeEntry.key} ${interactionEntry.key}',
        );
        expect(find.text(interactionEntry.key.name), findsNothing);
        expect(find.text('Streaming'), findsNothing);
        expect(find.text('正在生成'), findsNothing);
      }
    }
  });

  testWidgets('dense shell uses a compact rail without overflow', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(760, 720);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        runtime: const SessionRuntimeView(
          model: 'planner/local',
          contextTokens: 42000,
          contextWindow: 100000,
          totalTokens: 128000,
          costLabel: 'CNY 12.34',
          activeSkills: ['flutter-ui'],
          activeMcpServers: ['dart'],
          activeLspServers: ['rust-analyzer'],
          agentCount: 1,
        ),
        agentsBySession: {
          'session-1': {
            'agent-reviewer': StudioAgentView(
              id: 'agent-reviewer',
              sessionId: 'session-1',
              path: 'root/reviewer',
              role: 'reviewer',
              task: 'Audit compact status layout',
              status: 'running',
              updatedAt: DateTime.fromMillisecondsSinceEpoch(1),
            ),
          },
        },
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(
      tester.getSize(find.byKey(const ValueKey('studio-sidebar'))).width,
      StudioLayout.compactRailWidth,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('active task locks mode and exposes durable coordinator detail', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(760, 720);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        runtime: const SessionRuntimeView(
          model: 'planner/local',
          contextTokens: 1200,
          contextWindow: 100000,
          totalTokens: 1800,
          costLabel: '',
          activeSkills: [],
          activeMcpServers: [],
          activeLspServers: [],
          agentCount: 1,
          task: TaskRuntimeView(
            runId: 'task-run-1',
            phase: 'implementing',
            branch: 'codex/task-mode',
            expectedHead: '1234567890abcdef',
            statusMessage: 'Executor delivery ready',
            workUnits: [
              TaskWorkUnitView(
                id: 'unit-1',
                title: 'Implement coordinator UI',
                status: 'delivered',
                worktreePath: '.pure/worktrees/task-run-1/agent-1',
                branch: 'pure-task-run-1-agent-1',
                agentId: 'agent-1',
              ),
            ],
            agents: [
              TaskAgentOutcomeView(
                agentId: 'agent-1',
                role: 'executor',
                status: 'completed',
                initiatedBy: 'planner',
                requestedByCallId: 'call-spawn-1',
                summary: 'Implemented UI',
                error: null,
                headCommit: 'abcdef1234567890',
              ),
              TaskAgentOutcomeView(
                agentId: 'agent-explorer',
                role: 'explorer',
                status: 'running',
                initiatedBy: 'planner',
                requestedByCallId: 'call-explore-1',
                summary: 'Inspecting design constraints',
                error: null,
                headCommit: null,
              ),
            ],
            merges: [
              TaskMergeView(
                id: 'merge-1',
                agentId: 'agent-1',
                status: 'conflicted',
                mergeCommit: null,
                conflictFiles: ['lib/status.dart'],
                resolutionSummary: null,
              ),
            ],
            reviews: [
              TaskReviewView(
                round: 1,
                headCommit: '1234567890abcdef',
                verdict: 'changesRequired',
                reviewerAgentId: 'reviewer-1',
                summary: 'One issue remains',
                designReferences: ['design/16-task-orchestration.md#UI 与兼容性'],
              ),
            ],
          ),
        ),
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    final taskActivity = find.textContaining('Implementing · 1 agent');
    expect(taskActivity, findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.tap(find.byTooltip('Session mode'));
    await tester.pumpAndSettle();
    expect(find.text('Task'), findsNothing);
    expect(api.sessionModeUpdate, isNull);

    await tester.tap(taskActivity);
    await tester.pumpAndSettle();
    expect(find.text('TASK COORDINATOR'), findsOneWidget);
    expect(find.text('codex/task-mode'), findsOneWidget);
    expect(find.text('Implement coordinator UI'), findsOneWidget);
    expect(find.text('.pure/worktrees/task-run-1/agent-1'), findsOneWidget);
    expect(find.text('abcdef1234'), findsNWidgets(2));
    expect(find.text('lib/status.dart'), findsOneWidget);
    expect(find.text('Delivered'), findsOneWidget);
    expect(find.text('Conflicted'), findsOneWidget);
    expect(find.text('Changes required'), findsOneWidget);
    expect(find.text('changesRequired'), findsNothing);
    expect(find.text('explorer · agent-explorer'), findsOneWidget);
    expect(find.text('call-explore-1'), findsOneWidget);
    expect(find.text('Inspecting design constraints'), findsOneWidget);
    expect(
      find.text('design/16-task-orchestration.md#UI 与兼容性'),
      findsOneWidget,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('zh Hans localizes session and permission mode labels', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        sessions: [
          StudioSession(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Session',
            mode: CompileMode.task,
            updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
          ),
        ],
        permissionMode: PermissionMode.fullAccess,
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(
          locale: const Locale('zh', 'Hans'),
          home: const StudioShell(),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('任务 · 更新于'), findsOneWidget);
    expect(find.text('任务'), findsWidgets);
    expect(find.text('完全'), findsOneWidget);
    expect(find.text('Plan'), findsNothing);
    expect(find.text('Full'), findsNothing);

    await tester.tap(find.byTooltip('会话模式'));
    await tester.pumpAndSettle();
    expect(find.text('简洁'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('权限模式'));
    await tester.pumpAndSettle();
    expect(find.text('请求'), findsOneWidget);
    expect(find.text('审查'), findsOneWidget);
    expect(find.text('完全'), findsWidgets);
  });

  testWidgets('select menus open upward and stay clear of their triggers', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await _expectMenuOpensAboveTrigger(
      tester: tester,
      triggerTooltip: 'Session mode',
      menuText: 'Task',
    );
    await _expectMenuOpensAboveTrigger(
      tester: tester,
      triggerTooltip: 'Executor model',
      menuText: 'DeepSeek / DeepSeek Reasoner',
    );
    await _expectMenuOpensAboveTrigger(
      tester: tester,
      triggerTooltip: 'Reasoning effort',
      menuText: 'max',
    );
    await _expectMenuOpensAboveTrigger(
      tester: tester,
      triggerTooltip: 'Permission mode',
      menuText: 'Full',
    );
  });

  testWidgets('provider settings can add provider through typed save', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('CNY 88.00'), findsOneWidget);
    expect(find.text('Available balance'), findsNothing);

    await tester.tap(find.widgetWithText(FilledButton, 'Add provider'));
    await tester.pumpAndSettle();
    expect(find.text('New provider'), findsOneWidget);
    expect(find.text('Search providers'), findsNothing);

    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    final settings = api.savedProviderSettings;
    expect(settings, isNotNull);
    final providers = settings!['providers'] as List<Object?>;
    expect(providers.length, 2);
    expect((providers.last! as Map<String, Object?>)['id'], 'deepseek-2');
    expect(settings['defaultProviderId'], 'deepseek-2');
  });

  test('provider settings save updates default provider in store', () async {
    final api = _FakeStudioApi(_stateWithPlannerModels());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await container
        .read(studioControllerProvider.notifier)
        .saveProviderSettings({
          'defaultProviderId': 'deepseek',
          'providers': [
            {
              'id': 'deepseek',
              'templateKind': 'deepseek',
              'name': 'DeepSeek',
              'baseUrl': 'https://api.deepseek.com',
              'defaultModel': 'deepseek-v4-flash',
              'models': [
                {'slug': 'deepseek-v4-flash'},
              ],
            },
          ],
        });

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.defaultProviderId, 'deepseek');
  });

  testWidgets('provider editor cancel does not save local draft', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(FilledButton, 'Add provider'));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.widgetWithText(TextFormField, 'Display name'),
      'Changed Provider',
    );
    await tester.pumpAndSettle();
    expect(api.savedProviderSettings, isNull);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Cancel'));
    await tester.pumpAndSettle();

    expect(api.savedProviderSettings, isNull);
    expect(find.text('Search providers'), findsOneWidget);
  });

  testWidgets('provider search empty state explains active filter', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField).first, 'no-such-provider');
    await tester.pumpAndSettle();

    expect(find.text('No providers match this filter'), findsOneWidget);
    expect(
      find.text('Add a provider to configure credentials and models.'),
      findsNothing,
    );
  });

  testWidgets('editing non-default provider keeps current default provider', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        defaultProviderId: 'deepseek',
        providers: [
          ..._stateWithPlannerModels().providers,
          const ProviderSettingsView(
            id: 'openai',
            templateKind: 'openai',
            name: 'OpenAI',
            subtitle: 'OpenAI Platform',
            baseUrl: 'https://api.openai.com/v1',
            defaultModel: 'gpt-5.5',
            models: [
              ProviderModelView(
                slug: 'gpt-5.5',
                displayName: 'GPT-5.5',
                reasoningEfforts: ['medium'],
              ),
            ],
            status: 'ready',
            usageLabel: '1 models',
            modelCount: '1',
            providerKind: 'open_ai',
          ),
        ],
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Provider actions').last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Edit provider').last);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    expect(api.savedProviderSettings?['defaultProviderId'], 'deepseek');
  });

  testWidgets(
    'provider list uses one compact column and opens details from the row',
    (tester) async {
      _configureSettingsTestView(tester);
      final api = _FakeStudioApi(
        _providerListState(),
        providerUsages: _providerListUsages,
      );
      await _pumpSettingsPage(tester, api);

      expect(find.byType(StudioPanel), findsOneWidget);
      expect(find.byTooltip('Open details'), findsNothing);
      final deepSeekOrigin = tester.getTopLeft(find.text('DeepSeek'));
      final zhipuOrigin = tester.getTopLeft(find.text('Zhipu Coding Plan'));
      expect(zhipuOrigin.dx, closeTo(deepSeekOrigin.dx, 1));
      expect(zhipuOrigin.dy, greaterThan(deepSeekOrigin.dy));

      await tester.tap(find.text('Zhipu Coding Plan'));
      await tester.pumpAndSettle();

      expect(find.text('Search providers'), findsNothing);
      expect(find.widgetWithText(FilledButton, 'Edit'), findsOneWidget);
      expect(find.text('Usage'), findsOneWidget);
    },
  );

  testWidgets('provider row actions share one accessible overflow menu', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    final api = _FakeStudioApi(
      _providerListState(),
      providerUsages: _providerListUsages,
    );
    await _pumpSettingsPage(tester, api);

    expect(find.byTooltip('Provider actions'), findsNWidgets(2));
    expect(find.byTooltip('Edit provider'), findsNothing);
    expect(find.byTooltip('Delete provider'), findsNothing);
    final initialUsageLoads = api.loadProviderUsagesCount;

    await tester.tap(find.byTooltip('Provider actions').last);
    await tester.pumpAndSettle();
    expect(find.text('Set as default'), findsOneWidget);
    expect(find.text('Refresh usage'), findsWidgets);
    expect(find.text('Edit provider'), findsOneWidget);
    expect(find.text('Delete provider'), findsOneWidget);
    await tester.tap(find.text('Refresh usage').last);
    await tester.pumpAndSettle();
    expect(api.loadProviderUsagesCount, initialUsageLoads + 1);

    await tester.tap(find.byTooltip('Provider actions').last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Set as default'));
    await tester.pumpAndSettle();
    expect(
      api.savedProviderSettings?['defaultProviderId'],
      'zhipu-coding-plan',
    );
  });

  testWidgets('provider list shows compact DeepSeek and ordered Zhipu usage', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    final api = _FakeStudioApi(
      _providerListState(),
      providerUsages: _providerListUsages,
    );
    await _pumpSettingsPage(tester, api);

    expect(find.text('CNY 88.00'), findsOneWidget);
    expect(find.text('Available balance'), findsNothing);
    expect(find.text('Granted 8.00'), findsNothing);
    expect(find.text('Topped up 80.00'), findsNothing);
    final fiveHour = tester.getCenter(find.text('5 hour quota'));
    final weekly = tester.getCenter(find.text('Weekly quota'));
    final mcp = tester.getCenter(find.text('MCP quota'));
    expect(fiveHour.dy, lessThan(weekly.dy));
    expect(weekly.dy, lessThan(mcp.dy));
    expect(find.text('Other injected quota'), findsNothing);
    expect(find.text('25% remaining'), findsOneWidget);
    expect(find.text('50% remaining'), findsOneWidget);
    expect(find.text('80% remaining'), findsOneWidget);
    expect(find.text('25%'), findsNothing);
    expect(find.text('50%'), findsNothing);
    expect(find.text('80%'), findsNothing);
    expect(find.textContaining('Reset '), findsNWidgets(3));

    final progress = find.byType(LinearProgressIndicator);
    expect(progress, findsNWidgets(3));
    expect(
      tester
          .widgetList<LinearProgressIndicator>(progress)
          .map((bar) => bar.value),
      orderedEquals(const [0.25, 0.5, 0.8]),
    );
    for (final bar in progress.evaluate()) {
      expect(
        tester.getSize(find.byElementPredicate((item) => item == bar)).height,
        5,
      );
    }

    await tester.tap(find.text('DeepSeek'));
    await tester.pumpAndSettle();
    expect(find.text('Available balance'), findsOneWidget);
    expect(find.text('Granted 8.00'), findsOneWidget);
    expect(find.text('Topped up 80.00'), findsOneWidget);
  });

  testWidgets('provider list localizes remaining quota semantics in zh Hans', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    final api = _FakeStudioApi(
      _providerListState(zhipuOnly: true),
      providerUsages: _providerListUsages,
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(
          locale: const Locale.fromSubtags(
            languageCode: 'zh',
            scriptCode: 'Hans',
          ),
          home: const SettingsPage(),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('剩余 25%'), findsOneWidget);
    expect(find.text('剩余 50%'), findsOneWidget);
    expect(find.text('剩余 80%'), findsOneWidget);
    expect(find.text('25%'), findsNothing);
    expect(find.text('50%'), findsNothing);
    expect(find.text('80%'), findsNothing);
  });

  testWidgets('provider list omits absent Zhipu quotas', (tester) async {
    _configureSettingsTestView(tester);
    final api = _FakeStudioApi(
      _providerListState(zhipuOnly: true),
      providerUsages: const [
        ProviderUsageView(
          providerId: 'zhipu-coding-plan',
          updatedAt: 1,
          status: 'ready',
          usageKind: 'zhipuCodingPlan',
          codingPlan: ZhipuCodingPlanUsageView(
            limits: [
              ZhipuQuotaLimitView(
                window: 'weekly',
                label: 'weekly',
                percentage: 40,
                total: 100,
                remaining: 60,
                nextResetAt: 1735689600,
                usageDetails: [],
              ),
            ],
          ),
        ),
      ],
    );
    await _pumpSettingsPage(tester, api);

    expect(find.text('5 hour quota'), findsNothing);
    expect(find.text('Weekly quota'), findsOneWidget);
    expect(find.text('MCP quota'), findsNothing);
    expect(find.byType(LinearProgressIndicator), findsOneWidget);
  });

  testWidgets('provider list shows compact loading hint without fake quota', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    final staleUsage = _providerListUsages.last;
    final api = _FakeStudioApi(
      _providerListState(
        zhipuOnly: true,
      ).copyWith(providerUsages: [staleUsage]),
      providerUsages: [staleUsage],
    );
    final blocked = Completer<List<ProviderUsageView>>();
    api.blockedProviderUsageLoad = blocked;

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pump();
    await tester.pump();

    expect(find.text('Checking usage'), findsOneWidget);
    expect(find.text('Checking usage...'), findsNothing);
    expect(find.text('5 hour quota'), findsNothing);
    expect(find.byType(LinearProgressIndicator), findsNothing);

    blocked.complete(const []);
    await tester.pumpAndSettle();
  });

  for (final usageState in const [
    (
      status: 'missingCredential',
      shortMessage: 'Missing key',
      verboseMessage: 'Provider API key is not configured',
    ),
    (
      status: 'failed',
      shortMessage: 'Usage failed',
      verboseMessage: 'Usage query failed',
    ),
  ]) {
    testWidgets(
      'provider list shows compact ${usageState.status} hint without quota',
      (tester) async {
        _configureSettingsTestView(tester);
        final api = _FakeStudioApi(
          _providerListState(zhipuOnly: true),
          providerUsages: [
            ProviderUsageView(
              providerId: 'zhipu-coding-plan',
              updatedAt: 1,
              status: usageState.status,
              usageKind: 'zhipuCodingPlan',
            ),
          ],
        );
        await _pumpSettingsPage(tester, api);

        expect(find.text(usageState.shortMessage), findsOneWidget);
        expect(find.text(usageState.verboseMessage), findsNothing);
        expect(find.byType(LinearProgressIndicator), findsNothing);
      },
    );
  }

  testWidgets(
    'settings tabs use one group and Security has no duplicate mode',
    (tester) async {
      _configureSettingsTestView(tester);
      final base = _stateWithPlannerModels();
      final api = _FakeStudioApi(
        base.copyWith(
          runtime: base.runtime.copyWith(
            activeSkills: ['flutter-ui-polish', 'rust-review'],
          ),
          skills: const SkillsSettingsView(disabled: []),
          mcpServers: const [
            McpServerSettingsView(
              id: 'local',
              transport: 'stdio',
              endpoint: 'npx',
              enabled: true,
              status: 'enabled',
            ),
            McpServerSettingsView(
              id: 'remote',
              transport: 'http',
              endpoint: 'https://example.test/mcp',
              enabled: false,
              status: 'disabled',
            ),
          ],
        ),
      );
      await _pumpSettingsPage(tester, api);

      for (final tab in const ['Roles', 'Skills', 'MCP', 'General']) {
        await tester.tap(find.text(tab));
        await tester.pumpAndSettle();
        expect(
          find.byType(StudioPanel),
          findsOneWidget,
          reason: '$tab should use one outer settings group',
        );
      }

      await tester.tap(find.text('Security'));
      await tester.pumpAndSettle();
      expect(find.byType(StudioPanel), findsOneWidget);
      expect(find.text('Current: Request'), findsNothing);
      expect(
        find.text('Workspace boundary policy remains unchanged.'),
        findsOneWidget,
      );
    },
  );

  testWidgets(
    'settings ordinary controls save immediately without draft buttons',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final api = _FakeStudioApi(
        _stateWithPlannerModels().copyWith(
          runtime: _stateWithPlannerModels().runtime.copyWith(
            activeSkills: ['flutter-ui-polish'],
          ),
          skills: const SkillsSettingsView(disabled: []),
          mcpServers: const [
            McpServerSettingsView(
              id: 'local',
              transport: 'stdio',
              endpoint: 'npx',
              enabled: true,
              status: 'enabled',
            ),
          ],
        ),
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const SettingsPage()),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('Save draft'), findsNothing);

      await tester.tap(find.text('Roles'));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(DropdownButtonFormField<String>).first);
      await tester.pumpAndSettle();
      await tester.tap(find.textContaining('DeepSeek Reasoner').last);
      await tester.pumpAndSettle();
      expect(api.roleUpdate?.roleKey, 'explorer');

      await tester.tap(find.text('Skills'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('flutter-ui-polish'));
      await tester.pumpAndSettle();
      expect(
        api.savedSkillsSettings?['disabled'],
        contains('flutter-ui-polish'),
      );

      await tester.tap(find.text('MCP'));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(Switch).first);
      await tester.pumpAndSettle();
      final servers = api.savedMcpSettings?['servers'] as List<Object?>?;
      expect((servers?.single as Map<String, Object?>?)?['enabled'], isFalse);

      await tester.tap(find.text('General'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Compact timeline'));
      await tester.pumpAndSettle();
      expect(api.savedGeneralSettings?['compactTimeline'], isTrue);

      await tester.tap(find.text('Security'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Full'));
      await tester.pumpAndSettle();
      expect(api.savedPermissionMode, PermissionMode.fullAccess);
    },
  );

  testWidgets('instructions text saves after debounce', (tester) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Instructions'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).first, 'new base');
    await tester.pump(const Duration(milliseconds: 500));
    expect(api.savedInstructionsSettings, isNull);
    await tester.pump(const Duration(milliseconds: 200));

    expect(api.savedInstructionsSettings?['baseOverride'], 'new base');
  });

  testWidgets(
    'zh Hans locale localizes shell while config names pass through',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final api = _FakeStudioApi(_stateWithPlannerModels());
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(
            locale: const Locale.fromSubtags(
              languageCode: 'zh',
              scriptCode: 'Hans',
            ),
            home: const SettingsPage(),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('服务'), findsWidgets);
      expect(find.text('添加 provider'), findsOneWidget);
      expect(find.text('DeepSeek'), findsOneWidget);
      expect(find.text('deepseek-reasoner'), findsWidgets);

      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(
            locale: const Locale.fromSubtags(
              languageCode: 'zh',
              scriptCode: 'Hans',
            ),
            home: const StudioShell(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('描述你的需求...'), findsOneWidget);
      expect(find.text('deepseek-v4-flash'), findsOneWidget);
      expect(find.text('high'), findsOneWidget);
    },
  );
}

void _configureSettingsTestView(WidgetTester tester) {
  tester.view.physicalSize = const Size(1280, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
}

Future<void> _pumpSettingsPage(WidgetTester tester, _FakeStudioApi api) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: [studioApiProvider.overrideWithValue(api)],
      child: _localizedApp(home: const SettingsPage()),
    ),
  );
  await tester.pumpAndSettle();
}

StudioState _providerListState({bool zhipuOnly = false}) {
  final base = _stateWithPlannerModels();
  const zhipu = ProviderSettingsView(
    id: 'zhipu-coding-plan',
    templateKind: 'zhipu-coding-plan',
    name: 'Zhipu Coding Plan',
    subtitle: 'Zhipu Platform',
    baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
    hasBearerToken: true,
    defaultModel: 'glm-5.2',
    models: [
      ProviderModelView(
        slug: 'glm-5.2',
        displayName: 'GLM-5.2',
        reasoningEfforts: ['enabled'],
      ),
    ],
    status: 'ready',
    usageLabel: '1 model',
    modelCount: '1',
    providerKind: 'zhipu',
  );
  if (zhipuOnly) {
    return base.copyWith(defaultProviderId: zhipu.id, providers: const [zhipu]);
  }
  final deepSeek = base.providers.single.copyWith(
    templateKind: 'deepseek',
    subtitle: 'DeepSeek Platform',
    hasBearerToken: true,
    modelCount: '2',
    providerKind: 'deep_seek',
  );
  return base.copyWith(
    defaultProviderId: deepSeek.id,
    providers: [deepSeek, zhipu],
  );
}

const _providerListUsages = [
  ProviderUsageView(
    providerId: 'deepseek',
    updatedAt: 1735689600,
    status: 'ready',
    usageKind: 'deepseekBalance',
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
  ProviderUsageView(
    providerId: 'zhipu-coding-plan',
    updatedAt: 1735689600,
    status: 'ready',
    usageKind: 'zhipuCodingPlan',
    codingPlan: ZhipuCodingPlanUsageView(
      level: 'Pro',
      limits: [
        ZhipuQuotaLimitView(
          window: 'mcpMonthly',
          label: 'mcp',
          percentage: 20,
          nextResetAt: 1735689600,
          usageDetails: [],
        ),
        ZhipuQuotaLimitView(
          window: 'other',
          label: 'Other injected quota',
          percentage: 10,
          nextResetAt: 1735689600,
          usageDetails: [],
        ),
        ZhipuQuotaLimitView(
          window: 'weekly',
          label: 'weekly',
          percentage: 50,
          total: 200,
          remaining: 100,
          nextResetAt: 1735689600,
          usageDetails: [],
        ),
        ZhipuQuotaLimitView(
          window: 'fiveHour',
          label: 'five hour',
          percentage: 75,
          total: 100,
          remaining: 25,
          nextResetAt: 1735689600,
          usageDetails: [],
        ),
      ],
    ),
  ),
];
