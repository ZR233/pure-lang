part of '../widget_test.dart';

void registerShellSettingsTests() {
  testWidgets('zero sessions render the unpersisted start page', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final state = _emptyState().copyWith(
      threadDirectory: const ThreadDirectoryWindow(),
      workspacesByThread: const {},
      workspaceUiByThread: const {},
      selectedThreadId: null,
    );
    final api = _FakeStudioApi(state);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.startPage), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.composerInput), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.composerSubmit), findsOneWidget);
    expect(api.createdThreadProjectId, isNull);
  });

  testWidgets('new session stays transient until its first message', (
    tester,
  ) async {
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

    expect(find.byKey(StudioDriverKeys.threadRow('session-1')), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.newSession), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.archiveThread('session-1')),
      findsOneWidget,
    );

    await tester.tap(find.byKey(StudioDriverKeys.newSession));
    await tester.pumpAndSettle();

    expect(api.createdThreadProjectId, isNull);
    expect(find.byKey(StudioDriverKeys.startPage), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.threadRow('session-created')),
      findsNothing,
    );

    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'create the first turn',
    );
    await tester.pump();
    await tester.tap(find.byKey(StudioDriverKeys.composerSubmit));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.createdThreadProjectId, 'project-1');
    expect(api.newThreadPrompt, 'create the first turn');
    expect(api.createdThreadMode, StudioMode.simple);
    expect(
      find.byKey(StudioDriverKeys.threadRow('session-created')),
      findsOneWidget,
    );
    expect(api.threadSubscriptions.last, 'session-created');
  });

  testWidgets('start page selectors route by draft mode and submit with it', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _stateWithPlannerModels().copyWith(
      threadDirectory: const ThreadDirectoryWindow(),
      workspacesByThread: const {},
      workspaceUiByThread: const {},
      selectedThreadId: null,
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.startPageSelectors), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.sessionMode), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.model), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.reasoningEffort), findsOneWidget);
    expect(find.text('Simple'), findsOneWidget);
    expect(find.byTooltip('Executor model'), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.sessionMode));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(StudioDriverKeys.sessionModeOption(StudioMode.task.name)),
    );
    await tester.pumpAndSettle();
    expect(find.text('Task'), findsOneWidget);
    expect(find.byTooltip('Planner model'), findsOneWidget);
    expect(find.byTooltip('Executor model'), findsNothing);
    expect(api.createdThreadProjectId, isNull);

    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'plan the first turn',
    );
    await tester.pump();
    await tester.tap(find.byKey(StudioDriverKeys.composerSubmit));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.createdThreadMode, StudioMode.task);
    expect(find.byKey(StudioDriverKeys.startPageSelectors), findsNothing);
  });

  testWidgets('start page keeps the per-project mode draft after leaving', (
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

    expect(find.byKey(StudioDriverKeys.startPageSelectors), findsNothing);

    await tester.tap(find.byKey(StudioDriverKeys.newSession));
    await tester.pumpAndSettle();
    expect(find.text('Simple'), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.sessionMode));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(StudioDriverKeys.sessionModeOption(StudioMode.task.name)),
    );
    await tester.pumpAndSettle();
    expect(find.text('Task'), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.threadRow('session-1')));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.startPageSelectors), findsNothing);

    await tester.tap(find.byKey(StudioDriverKeys.newSession));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.startPageSelectors), findsOneWidget);
    expect(find.text('Task'), findsOneWidget);
    expect(find.text('Simple'), findsNothing);
  });

  testWidgets('sidebar archives a root Thread and adopts canonical fallback', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final initial = _emptyState();
    final first = initial.threads.single;
    final second = StudioThread(
      id: 'session-2',
      projectId: first.projectId,
      title: 'Session 2',
      mode: StudioMode.simple,
      updatedAt: DateTime.fromMillisecondsSinceEpoch(1),
    );
    final state = initial.copyWith(
      threadDirectory: ThreadDirectoryWindow(threads: [first, second]),
    );
    final api = _FakeStudioApi(state)
      ..archiveThreadState = state.copyWith(
        threadDirectory: ThreadDirectoryWindow(threads: [second]),
        selectedThreadId: second.id,
      );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.archiveThread(first.id)));
    await tester.pumpAndSettle();

    expect(api.archivedThreadId, first.id);
    expect(find.byKey(StudioDriverKeys.threadRow(first.id)), findsNothing);
    expect(find.byKey(StudioDriverKeys.threadRow(second.id)), findsOneWidget);
    expect(api.threadSubscriptions.last, second.id);
  });

  testWidgets('sidebar shows a localized error when archive is rejected', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_emptyState())
      ..archiveThreadError = StateError('Thread became busy');
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.archiveThread('session-1')));
    await tester.pumpAndSettle();

    expect(api.archiveThreadCallCount, 1);
    expect(
      find.text('Could not archive this session. It may still be running.'),
      findsOneWidget,
    );
    expect(find.byKey(StudioDriverKeys.threadRow('session-1')), findsOneWidget);
  });

  testWidgets('compact rail keeps new and archive Thread actions', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(800, 700);
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

    expect(find.byKey(StudioDriverKeys.newSession), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.archiveThread('session-1')),
      findsOneWidget,
    );
    await tester.tap(find.byKey(StudioDriverKeys.newSession));
    await tester.pumpAndSettle();
    expect(api.createdThreadProjectId, isNull);
    expect(find.byKey(StudioDriverKeys.startPage), findsOneWidget);
  });

  testWidgets('selected busy root Thread cannot be archived', (tester) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _withSelectedTurn(
      _emptyState(),
      _testTurn(
        threadId: 'session-1',
        state: const RunningStudioTurnState(
          startedAt: 1,
          activity: StudioTurnActivity.responding,
        ),
      ),
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    final button = tester.widget<IconButton>(
      find.byKey(StudioDriverKeys.archiveThread('session-1')),
    );
    expect(button.onPressed, isNull);
  });

  testWidgets('driver project path dialog opens the entered project', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_emptyState());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(api),
          projectDirectoryPickerProvider.overrideWithValue(
            showDriverProjectPathDialog,
          ),
        ],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.openProject));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.projectPathDialog), findsOneWidget);

    const path = r'C:\workspace\shooter';
    await tester.enterText(find.byKey(StudioDriverKeys.projectPathInput), path);
    await tester.pump();
    expect(
      tester
          .widget<TextField>(find.byKey(StudioDriverKeys.projectPathInput))
          .controller
          ?.text,
      path,
    );

    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(api.openedProjectPath, path);
    expect(find.byKey(StudioDriverKeys.projectPathDialog), findsNothing);
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
    expect(find.byKey(StudioDriverKeys.openProject), findsOneWidget);
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

  testWidgets(
    'project cleanup remains available while current session is busy',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final api = _FakeStudioApi(
        _twoProjectState(
          selectedProjectId: 'project-a',
          turnState: const RunningStudioTurnState(
            startedAt: 1,
            activity: StudioTurnActivity.responding,
          ),
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
      expect(closeButtons.first.onPressed, isNotNull);
      expect(closeButtons.last.onPressed, isNotNull);

      await tester.tap(closeProjectButtons.first);
      await tester.pumpAndSettle();
      expect(api.previewProjectCleanupCount, 1);
      expect(
        find.text('Remove project and clean up Pure worktrees?'),
        findsOneWidget,
      );
      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();
      expect(api.archivedProjectId, isNull);
      expect(api.cleanedProjectId, isNull);
    },
  );

  testWidgets('status bar routes model controls by Thread mode', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _withSelectedRuntime(
        _stateWithPlannerModels(),
        const ThreadRuntimeView(
          model: 'planner/local',
          contextTokens: 42,
          contextWindow: 100,
          totalTokens: 128,
          costLabel: '￥0.16',
          activeSkills: ['flutter-ui'],
          activeMcpServers: ['dart'],
          activeLspServers: ['rust-analyzer'],
          agentCount: 0,
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

    expect(find.byTooltip('Session mode'), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.sessionMode), findsOneWidget);
    expect(find.text('Simple'), findsOneWidget);
    expect(find.byTooltip('Executor model'), findsOneWidget);
    expect(find.byTooltip('Planner model'), findsNothing);
    expect(find.byTooltip('Reasoning effort'), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.reasoningEffort), findsOneWidget);
    expect(find.byType(StatusBarItem), findsWidgets);
    final contextReadout = find.bySemanticsLabel('Context');
    expect(contextReadout, findsOneWidget);
    expect(
      find.descendant(of: contextReadout, matching: find.byType(CustomPaint)),
      findsOneWidget,
    );
    expect(find.text('42%'), findsNothing);
    expect(find.text('42/100'), findsNothing);
    expect(find.text('￥0.16'), findsNothing);
    expect(find.text('1 skill · 1 MCP · 1 LSP'), findsOneWidget);
    expect(find.text('1 skill · 1 MCP · 1 LSP · 1 agent'), findsNothing);
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
    expect(find.text('￥0.16'), findsOneWidget);
    await gesture.moveTo(Offset.zero);
    await tester.pumpAndSettle();
    await gesture.removePointer();

    final activityFinder = find.text('1 skill · 1 MCP · 1 LSP');
    final activityCenter = tester.getCenter(activityFinder);
    final activityRect = tester.getRect(activityFinder);
    await tester.tapAt(Offset(activityRect.left + 8, activityCenter.dy));
    await tester.pumpAndSettle();
    expect(find.text('ACTIVE CAPABILITIES'), findsOneWidget);
    expect(find.text('Skills'), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.statusActiveSkill('flutter-ui')),
      findsOneWidget,
    );
    expect(find.textContaining('MCP · dart'), findsOneWidget);
    expect(find.textContaining('LSP · rust-analyzer'), findsOneWidget);
    expect(find.text('SUBAGENTS'), findsNothing);
    expect(find.text('reviewer'), findsNothing);
    expect(find.text('worker'), findsNothing);
    await tester.tapAt(Offset.zero);
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.sessionMode));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(StudioDriverKeys.sessionModeOption(StudioMode.task.name)),
    );
    await tester.pumpAndSettle();
    expect(api.modeUpdate?.threadId, 'session-1');
    expect(api.modeUpdate?.mode, StudioMode.task);
    expect(
      api.threadSubscriptions.where((threadId) => threadId == 'session-1'),
      hasLength(2),
    );
    expect(find.text('Task'), findsOneWidget);
    expect(find.byTooltip('Planner model'), findsOneWidget);

    await tester.tap(find.byTooltip('Planner model'));
    await tester.pumpAndSettle();
    await tester.tap(find.textContaining('Reasoner').last);
    await tester.pumpAndSettle();
    expect(api.roleUpdate?.roleKey, 'planner');
    expect(api.roleUpdate?.providerId, 'deepseek');
    expect(api.roleUpdate?.model, 'deepseek-reasoner');

    api.emitGlobal(
      _threadDirectoryChangedEvent(
        projectId: 'project-1',
        threads: [
          StudioThread(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Session',
            mode: StudioMode.task,
            updatedAt: DateTime.fromMillisecondsSinceEpoch(2000),
          ),
        ],
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Reasoning effort'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('max').last);
    await tester.pumpAndSettle();
    expect(api.roleUpdate?.roleKey, 'planner');
    expect(api.roleUpdate?.providerId, 'deepseek');
    expect(api.roleUpdate?.model, 'deepseek-reasoner');
    expect(api.roleUpdate?.effort, 'max');
  });

  testWidgets('header shows session cost only and selected agent throughput', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final base = _rootAndChildState();
    final rootWorkspace = base.workspacesByThread['session-1']!;
    final childWorkspace = base.workspacesByThread['child-1']!;
    final state = base.copyWith(
      modelPerformance: _modelPerformanceFixture(),
      workspacesByThread: {
        'session-1': rootWorkspace.copyWith(
          runtime: rootWorkspace.runtime.copyWith(
            turnCompletionTokens: 150,
            turnDecodeMillis: 1000,
          ),
        ),
        'child-1': childWorkspace.copyWith(
          runtime: childWorkspace.runtime.copyWith(
            turnCompletionTokens: 75,
            turnDecodeMillis: 1000,
          ),
        ),
      },
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    final sessionCost = find.byKey(StudioDriverKeys.sessionCost);
    expect(sessionCost, findsOneWidget);
    expect(
      find.descendant(of: sessionCost, matching: find.text(r'￥0.14 + $0.02')),
      findsOneWidget,
    );
    expect(find.textContaining('Total cost'), findsNothing);
    expect(find.byKey(StudioDriverKeys.threadThroughput), findsOneWidget);
    expect(find.text('150 t/s'), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.agentSwitcher));
    await tester.pumpAndSettle();
    if (find.byKey(StudioDriverKeys.agentRow('child-1')).evaluate().isEmpty) {
      await tester.tap(find.byKey(StudioDriverKeys.agentSwitcher));
      await tester.pumpAndSettle();
    }
    await tester.tap(find.byKey(StudioDriverKeys.agentRow('child-1')));
    await tester.pumpAndSettle();

    expect(find.text(r'￥0.14 + $0.02'), findsOneWidget);
    expect(find.text('75 t/s'), findsOneWidget);
    expect(find.text('150 t/s'), findsNothing);
  });

  testWidgets('status bar shows a placeholder without a speed sample', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(900, 160);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      ProviderScope(
        child: _localizedApp(
          home: Scaffold(
            body: ThreadStatusBar(
              workspace: _emptyState().selectedAgentWorkspace!,
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.threadThroughput), findsOneWidget);
    expect(find.text('- t/s'), findsOneWidget);
  });

  testWidgets('status bar omits turn and interaction activity readouts', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      ProviderScope(
        child: _localizedApp(
          locale: const Locale('en'),
          home: ThreadStatusBar(
            workspace: _withSelectedInteractions(
              _withSelectedTurn(
                _emptyState(),
                _testTurn(
                  threadId: 'session-1',
                  state: const CompletedStudioTurnState(
                    startedAt: 1,
                    completedAt: 2,
                    completion: StudioTurnCompletion.normal,
                  ),
                ),
              ),
              const [
                PendingInteraction(
                  id: 'interaction-1',
                  threadId: 'session-1',
                  turnId: 'turn-1',
                  kind: InteractionKind.userInput,
                  title: 'Pending',
                  body: 'Pending interaction',
                ),
              ],
            ).selectedAgentWorkspace!,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Waiting for input'), findsNothing);
    expect(find.text('Responding'), findsNothing);
  });

  testWidgets('dense shell uses a compact rail without overflow', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(760, 720);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _withSelectedRuntime(
        _stateWithPlannerModels(),
        const ThreadRuntimeView(
          model: 'planner/local',
          contextTokens: 42000,
          contextWindow: 100000,
          totalTokens: 128000,
          costLabel: '￥12.34',
          activeSkills: ['flutter-ui'],
          activeMcpServers: ['dart'],
          activeLspServers: ['rust-analyzer'],
          agentCount: 0,
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

    expect(
      tester.getSize(find.byKey(const ValueKey('studio-sidebar'))).width,
      StudioLayout.compactRailWidth,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('active task locks mode and exposes the status phase', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(760, 720);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _stateWithPlannerModels();
    final thread = state.selectedThread!.copyWith(
      mode: StudioMode.task,
      status: ThreadStatusView.running,
    );
    final api = _FakeStudioApi(
      state.copyWith(
        threadDirectory: ThreadDirectoryWindow(threads: [thread]),
        workspacesByThread: {
          thread.id: state.selectedWorkspace!.copyWith(
            thread: thread,
            runtime: const ThreadRuntimeView(
              model: 'planner/local',
              contextTokens: 1200,
              contextWindow: 100000,
              totalTokens: 1800,
              costLabel: '',
              activeSkills: [],
              activeMcpServers: [],
              activeLspServers: [],
              agentCount: 1,
            ),
          ),
        },
        taskDirectory: TaskDirectoryState(
          values: [
            TaskDirectoryEntryView(
              rootThreadId: 'session-1',
              task: TaskRuntimeView(
                runId: 'task-run-1',
                state: const WorkingTaskStateView(
                  documentEditSummary: 'test documents updated',
                ),
                revision: 0,
                generation: 0,
                workUnits: [],
                completions: [],
                merges: [],
                reviews: [],
              ),
            ),
          ],
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

    expect(
      find.descendant(
        of: find.byType(ThreadStatusBar),
        matching: find.text('Working'),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskRuntime('task-run-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.taskPhase('task-run-1', TaskStateKind.working),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.taskStatus('task-run-1', 'test documents updated'),
      ),
      findsOneWidget,
    );
    expect(tester.takeException(), isNull);
    expect(
      find.byTooltip(
        'Session mode cannot change while the session is running or a Task is active',
      ),
      findsOneWidget,
    );
    expect(find.text('Task'), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.sessionMode), findsOneWidget);
    await tester.tap(find.byKey(StudioDriverKeys.sessionMode));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.sessionModeOption(StudioMode.simple.name)),
      findsNothing,
    );
    expect(api.modeUpdate, isNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'idle active task is presented as paused without changing phase',
    (tester) async {
      tester.view.physicalSize = const Size(760, 720);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final state = _stateWithPlannerModels();
      final thread = state.selectedThread!.copyWith(mode: StudioMode.task);
      final api = _FakeStudioApi(
        state.copyWith(
          threadDirectory: ThreadDirectoryWindow(threads: [thread]),
          workspacesByThread: {
            thread.id: state.selectedWorkspace!.copyWith(
              thread: thread,
              runtime: const ThreadRuntimeView(
                model: 'planner/local',
                contextTokens: 1200,
                contextWindow: 100000,
                totalTokens: 1800,
                costLabel: '',
                activeSkills: [],
                activeMcpServers: [],
                activeLspServers: [],
                agentCount: 1,
              ),
            ),
          },
          taskDirectory: TaskDirectoryState(
            values: [
              TaskDirectoryEntryView(
                rootThreadId: 'session-1',
                task: TaskRuntimeView(
                  runId: 'task-run-1',
                  state: const WorkingTaskStateView(
                    documentEditSummary: 'test documents updated',
                  ),
                  revision: 0,
                  generation: 0,
                  workUnits: [],
                  completions: [],
                  merges: [],
                  reviews: [],
                ),
              ),
            ],
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

      expect(
        find.descendant(
          of: find.byType(ThreadStatusBar),
          matching: find.text('Paused'),
        ),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.taskPaused('task-run-1')),
        findsOneWidget,
      );
      expect(
        find.byKey(
          StudioDriverKeys.taskPhase('task-run-1', TaskStateKind.working),
        ),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('running thread locks the session mode selector', (tester) async {
    tester.view.physicalSize = const Size(760, 720);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _stateWithPlannerModels();
    final thread = state.selectedThread!.copyWith(
      status: ThreadStatusView.running,
    );
    final api = _FakeStudioApi(
      state.copyWith(
        threadDirectory: ThreadDirectoryWindow(threads: [thread]),
        workspacesByThread: {
          thread.id: state.selectedWorkspace!.copyWith(thread: thread),
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

    expect(find.byKey(StudioDriverKeys.sessionMode), findsOneWidget);
    expect(
      find.byTooltip(
        'Session mode cannot change while the session is running or a Task is active',
      ),
      findsOneWidget,
    );
    await tester.tap(find.byKey(StudioDriverKeys.sessionMode));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.sessionModeOption(StudioMode.task.name)),
      findsNothing,
    );
    expect(api.modeUpdate, isNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets('zh Hans localizes Thread and permission mode labels', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _stateWithPlannerModels();
    final taskThread = state.selectedThread!.copyWith(mode: StudioMode.task);
    final api = _FakeStudioApi(
      _withSettingsFixture(
        state.copyWith(
          threadDirectory: ThreadDirectoryWindow(threads: [taskThread]),
          workspacesByThread: {
            taskThread.id: state.selectedWorkspace!.copyWith(
              thread: taskThread,
            ),
          },
        ),
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

    await tester.tap(find.byTooltip('权限模式'));
    await tester.pumpAndSettle();
    expect(find.text('请求'), findsOneWidget);
    expect(find.text('审查'), findsOneWidget);
    expect(find.text('完全'), findsWidgets);

    await tester.pumpWidget(
      ProviderScope(
        key: const ValueKey('simple-thread-provider-scope'),
        overrides: [studioApiProvider.overrideWithValue(_FakeStudioApi(state))],
        child: _localizedApp(
          locale: const Locale('zh', 'Hans'),
          home: const StudioShell(),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('简洁'), findsOneWidget);
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

  testWidgets('status model and effort controls expose stable driver keys', (
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

    await tester.tap(find.byKey(StudioDriverKeys.model));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.modelOption('deepseek', 'deepseek-reasoner')),
      findsOneWidget,
    );
    await tester.tap(
      find.byKey(StudioDriverKeys.modelOption('deepseek', 'deepseek-reasoner')),
    );
    await tester.pumpAndSettle();
    expect(api.roleUpdate?.model, 'deepseek-reasoner');

    await tester.tap(find.byKey(StudioDriverKeys.reasoningEffort));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.reasoningEffortOption('max')),
      findsOneWidget,
    );
  });

  testWidgets('MCP refresh is read-only and reset commands are explicit', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const server = McpServerSettingsView(
      id: 'dart',
      transport: 'stdio',
      endpoint: 'dart mcp-server',
      state: McpAvailableState(checkedAt: 0, toolCount: 4),
    );
    final state =
        _withSettingsFixture(
          _emptyState(),
          mcpServers: const [server],
        ).copyWith(
          mcpState: McpStateSnapshot(
            revision: 1,
            activeServers: const ['dart'],
            servers: const [server],
          ),
        );
    final api = _FakeStudioApi(state);
    await _pumpSettingsPage(tester, api);
    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('mcp')));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.mcpRefresh));
    await tester.pumpAndSettle();
    expect(api.readMcpStateCount, 1);
    expect(api.resetMcpServerId, isNull);
    expect(api.resetAllMcpCount, 0);

    await tester.tap(find.byKey(StudioDriverKeys.mcpResetServer('dart')));
    await tester.pumpAndSettle();
    expect(api.resetMcpServerId, 'dart');

    await tester.tap(find.byKey(StudioDriverKeys.mcpResetAll));
    await tester.pumpAndSettle();
    expect(api.resetAllMcpCount, 0);
    await tester.tap(find.byKey(StudioDriverKeys.mcpResetAllConfirm));
    await tester.pumpAndSettle();
    expect(api.resetAllMcpCount, 1);
  });

  testWidgets('MCP runtime errors stay scoped to the unavailable server', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const unavailable = McpServerSettingsView(
      id: 'zhipu_vision',
      transport: 'stdio',
      endpoint: 'npx',
      state: McpUnavailableState(
        checkedAt: 0,
        code: 'mcpServerUnavailable',
        message: 'MCP connection failed: credential [REDACTED]',
        retryable: true,
      ),
    );
    const available = McpServerSettingsView(
      id: 'dart',
      transport: 'stdio',
      endpoint: 'dart mcp-server',
      state: McpAvailableState(checkedAt: 0, toolCount: 4),
    );
    final state =
        _withSettingsFixture(
          _emptyState(),
          mcpServers: const [unavailable, available],
        ).copyWith(
          mcpState: McpStateSnapshot(
            revision: 1,
            activeServers: const ['dart'],
            servers: const [unavailable, available],
          ),
        );
    final api = _FakeStudioApi(state);
    await _pumpSettingsPage(tester, api);
    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('mcp')));
    await tester.pumpAndSettle();

    final unavailableRow = find.byKey(
      StudioDriverKeys.mcpServerRow('zhipu_vision'),
    );
    final availableRow = find.byKey(StudioDriverKeys.mcpServerRow('dart'));
    expect(
      find.descendant(of: unavailableRow, matching: find.text('unavailable')),
      findsOneWidget,
    );
    expect(
      find.descendant(of: availableRow, matching: find.text('available')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.mcpServerError('zhipu_vision')),
      findsOneWidget,
    );
    expect(find.byKey(StudioDriverKeys.mcpServerError('dart')), findsNothing);
  });

  testWidgets('LSP refresh, probe, repair and reset use typed commands', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const server = LspServerStateView(
      id: 'rust-analyzer',
      displayName: 'rust-analyzer',
      state: LspUnavailableState(
        checkedAt: 0,
        code: 'lspComponentMissing',
        message: 'component missing',
        retryable: true,
      ),
    );
    final state = _emptyState().copyWith(
      lspState: LspStateSnapshot(revision: 1, servers: const [server]),
    );
    final api = _FakeStudioApi(state);
    await _pumpSettingsPage(tester, api);
    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('lsp')));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.lspRefresh));
    await tester.pumpAndSettle();
    expect(api.readLspStateCount, 1);
    expect(api.probedLspProjectId, isNull);

    await tester.tap(find.byKey(StudioDriverKeys.lspProbe));
    await tester.pumpAndSettle();
    expect(api.probedLspProjectId, 'project-1');

    await tester.tap(
      find.byKey(StudioDriverKeys.lspRepairServer('rust-analyzer')),
    );
    await tester.pumpAndSettle();
    expect(api.repairedLspServer?.projectId, 'project-1');
    expect(api.repairedLspServer?.serverId, 'rust-analyzer');

    await tester.tap(
      find.byKey(StudioDriverKeys.lspResetServer('rust-analyzer')),
    );
    await tester.pumpAndSettle();
    expect(api.resetLspServerRequest?.projectId, 'project-1');
    expect(api.resetLspServerRequest?.serverId, 'rust-analyzer');

    await tester.tap(find.byKey(StudioDriverKeys.lspResetWorkspace));
    await tester.pumpAndSettle();
    expect(api.resetLspWorkspaceProjectId, 'project-1');
  });

  testWidgets('LSP settings row shows activity pill and progress detail', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const servers = [
      LspServerStateView(
        id: 'rust-analyzer',
        displayName: 'rust-analyzer',
        state: LspAvailableState(
          checkedAt: 0,
          diagnosticCount: 0,
          activity: LspIndexingActivity(
            title: 'Roots Scanned',
            message: '166/408',
            percentage: 40,
          ),
        ),
      ),
      LspServerStateView(
        id: 'dart',
        displayName: 'dart',
        state: LspAvailableState(
          checkedAt: 0,
          diagnosticCount: 0,
          activity: LspBusyActivity(),
        ),
      ),
    ];
    final state = _emptyState().copyWith(
      lspState: LspStateSnapshot(revision: 1, servers: servers),
    );
    final api = _FakeStudioApi(state);
    await _pumpSettingsPage(tester, api);
    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('lsp')));
    await tester.pumpAndSettle();

    expect(find.text('Indexing · 40%'), findsOneWidget);
    expect(find.text('Roots Scanned · 166/408'), findsOneWidget);
    expect(find.text('Busy'), findsOneWidget);
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

  testWidgets('OpenAI provider template exposes GPT-5.6 variants', (
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
    await tester.tap(find.byType(DropdownButtonFormField<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('OpenAI').last);
    await tester.pumpAndSettle();

    final textInputValues = tester
        .widgetList<EditableText>(find.byType(EditableText))
        .map((field) => field.controller.text)
        .toList();
    expect(
      textInputValues,
      containsAll(['openai', 'OpenAI', 'https://api.openai.com/v1']),
    );
    expect(textInputValues, isNot(contains('https://api.deepseek.com')));
    final dropdownValues = tester
        .widgetList<DropdownButton<String>>(find.byType(DropdownButton<String>))
        .map((dropdown) => dropdown.value)
        .toList();
    expect(
      dropdownValues,
      containsAll(['openai', 'gpt-5.6-sol', 'preset_defaults']),
    );
    expect(find.text('OPENAI_API_KEY'), findsOneWidget);
    expect(find.text('GPT-5.6-Sol'), findsOneWidget);
    expect(find.text('gpt-5.6-sol'), findsOneWidget);
    expect(find.text('GPT-5.6-Terra'), findsOneWidget);
    expect(find.text('gpt-5.6-terra'), findsOneWidget);
    expect(find.text('GPT-5.6-Luna'), findsOneWidget);
    expect(find.text('gpt-5.6-luna'), findsOneWidget);
    expect(find.text('Responses · WS'), findsWidgets);
    expect(find.text('WS'), findsWidgets);
    expect(find.text('HTTP'), findsWidgets);
    expect(
      find.byKey(
        StudioDriverKeys.providerModelConnectionMode('openai', 'gpt-5.6-sol'),
      ),
      findsOneWidget,
    );
    final httpMode = find.byKey(
      StudioDriverKeys.providerModelConnectionModeOption(
        'openai',
        'gpt-5.6-sol',
        'http',
      ),
    );
    await tester.ensureVisible(httpMode);
    await tester.pumpAndSettle();
    expect(httpMode.hitTestable(), findsOneWidget);
    await tester.tap(httpMode);
    await tester.pumpAndSettle();
    expect(find.text('Responses · HTTP'), findsOneWidget);

    await tester.fling(find.byType(ListView).last, const Offset(0, 1000), 2000);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    final providers = api.savedProviderSettings!['providers'] as List<Object?>;
    final openai = providers.last! as Map<String, Object?>;
    expect(openai['id'], 'openai');
    expect(openai['templateKind'], 'openai');
    expect(openai['name'], 'OpenAI');
    expect(openai['baseUrl'], 'https://api.openai.com/v1');
    expect(openai['defaultModel'], 'gpt-5.6-sol');
    expect(openai['capabilitySource'], 'preset_defaults');
    expect(openai['hostedWebSearch'], isTrue);
    expect(openai['standaloneWebSearch'], 'open_ai_search_api');
    expect(openai['promptCacheDialect'], 'open_ai_prompt_cache_key');
    expect(openai['responsesProgrammaticToolCalling'], isTrue);
    final modes = openai['modelConnectionModes'] as List<Object?>;
    expect(
      modes.cast<Map<String, Object?>>().singleWhere(
        (mode) => mode['slug'] == 'gpt-5.6-sol',
      )['connectionMode'],
      'http',
    );
  });

  testWidgets('unknown provider catalog entry works without UI branches', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _stateWithPlannerModels(),
      providerCatalog: _testProviderCatalog,
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(FilledButton, 'Add provider'));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(DropdownButtonFormField<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Future Provider').last);
    await tester.pumpAndSettle();

    expect(find.text('Future Model'), findsOneWidget);
    expect(find.text('future-model'), findsOneWidget);
    final editor = find.byType(ListView).last;
    await tester.fling(editor, const Offset(0, -1000), 2000);
    await tester.pumpAndSettle();
    expect(
      find.byWidgetPredicate(
        (widget) =>
            widget is TextFormField &&
            widget.initialValue == 'future_search_dialect',
      ),
      findsOneWidget,
    );

    await tester.fling(editor, const Offset(0, 1000), 2000);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    final providers = api.savedProviderSettings!['providers'] as List<Object?>;
    final future = providers.last! as Map<String, Object?>;
    expect(future['templateKind'], 'future-provider');
    expect(future['defaultModel'], 'future-model');
    expect(future['capabilitySource'], 'preset_defaults');
    expect(future['standaloneWebSearch'], 'future_search_dialect');
    expect(future['promptCacheDialect'], 'implicit_prefix');
  });

  testWidgets('custom Responses provider defaults to HTTP without a preset', (
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
    await tester.tap(find.byType(DropdownButtonFormField<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Custom provider').last);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(OutlinedButton, 'Add model'));
    await tester.pumpAndSettle();
    final protocolDropdown = find
        .widgetWithText(DropdownButtonFormField<String>, 'Chat Completions')
        .last;
    await tester.ensureVisible(protocolDropdown);
    await tester.pumpAndSettle();
    await tester.tap(protocolDropdown);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Responses').last);
    await tester.pumpAndSettle();

    expect(find.text('WS'), findsWidgets);
    expect(find.text('HTTP'), findsWidgets);
    await tester.fling(find.byType(ListView).last, const Offset(0, 1000), 2000);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    final providers = api.savedProviderSettings!['providers'] as List<Object?>;
    final custom = providers.last! as Map<String, Object?>;
    expect(custom['templateKind'], '');
    final customModels = custom['customModels'] as List<Object?>;
    final customModel = customModels.single! as Map<String, Object?>;
    expect(customModel['wireProtocol'], 'responses');
    expect(customModel['defaultConnectionMode'], 'http');
    expect(customModel['supportedConnectionModes'], ['web_socket', 'http']);
    expect(custom['capabilitySource'], 'explicit');
    expect(custom['promptCacheDialect'], 'none');
  });

  testWidgets('custom Chat model exposes only HTTP and persists transport', (
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
    await tester.tap(find.byType(DropdownButtonFormField<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Custom provider').last);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(OutlinedButton, 'Add model'));
    await tester.pumpAndSettle();

    expect(find.text('Chat Completions'), findsWidgets);
    final supportedConnections = find.byType(SegmentedButton<String>).last;
    expect(
      tester.widget<SegmentedButton<String>>(supportedConnections).selected,
      {'http'},
    );
    final wsOption = find.descendant(
      of: supportedConnections,
      matching: find.text('WS'),
    );
    await tester.ensureVisible(wsOption);
    await tester.pumpAndSettle();
    await tester.tap(wsOption);
    await tester.pumpAndSettle();
    expect(
      tester.widget<SegmentedButton<String>>(supportedConnections).selected,
      {'http'},
    );
    await tester.fling(find.byType(ListView).last, const Offset(0, 1000), 2000);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    final providers = api.savedProviderSettings!['providers'] as List<Object?>;
    final custom = providers.last! as Map<String, Object?>;
    final customModels = custom['customModels'] as List<Object?>;
    final customModel = customModels.single! as Map<String, Object?>;
    expect(customModel['wireProtocol'], 'chat_completions');
    expect(customModel['defaultConnectionMode'], 'http');
    expect(customModel['supportedConnectionModes'], ['http']);
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
        .saveProviderSettings(
          const ProviderSettingsCommand(
            defaultProviderId: 'deepseek',
            providers: [
              ProviderCommand(
                id: 'deepseek',
                templateKind: 'deepseek',
                name: 'DeepSeek',
                baseUrl: 'https://api.deepseek.com',
                secret: ProviderSecretCommand.preserve(),
                capabilitySource: 'preset_defaults',
                hostedWebSearch: false,
                promptCacheDialect: 'implicit_prefix',
                responsesProgrammaticToolCalling: false,
                defaultModel: 'deepseek-v4-flash',
                customModels: [],
                modelConnectionModes: [],
              ),
            ],
            roles: [],
          ),
        );

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.defaultProviderId, 'deepseek');
  });

  test('catalog metadata preserves endpoint-resolved capabilities', () {
    const provider = ProviderSettingsView(
      id: 'openai',
      templateKind: 'openai',
      name: 'Compatible OpenAI',
      baseUrl: 'https://compatible.example/v1',
      capabilitySource: 'preset_defaults',
      hostedWebSearch: false,
      promptCacheDialect: 'none',
      responsesProgrammaticToolCalling: false,
      defaultModel: 'gpt-5.6-sol',
      models: [],
      modelConnectionModes: {'gpt-5.6-sol': 'http'},
      status: 'ready',
      usageLabel: '',
    );

    final resolved = providerWithCatalogMetadata(
      provider,
      _testProviderCatalog,
    );

    expect(resolved.hostedWebSearch, isFalse);
    expect(resolved.promptCacheDialect, 'none');
    expect(resolved.responsesProgrammaticToolCalling, isFalse);
    expect(resolved.credentialEnv, 'OPENAI_API_KEY');
    expect(
      resolved.models.first.inputCapabilities.map((value) => value.modality),
      [ModelModalityView.text],
    );
    expect(resolved.models.first.connectionMode, 'http');
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
      _withSettingsFixture(
        _stateWithPlannerModels(),
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
            models: [],
            customModels: [
              ProviderModelView(
                slug: 'gpt-5.5',
                displayName: 'GPT-5.5',
                reasoningEfforts: ['medium'],
                wireProtocol: 'responses',
                supportedConnectionModes: ['web_socket', 'http'],
                defaultConnectionMode: 'web_socket',
                connectionMode: 'web_socket',
              ),
            ],
            status: 'ready',
            usageLabel: '1 models',
            modelCount: '1',
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
    await tester.enterText(
      find.widgetWithText(TextFormField, 'Provider key'),
      'openai-team',
    );
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    expect(api.savedProviderSettings?['defaultProviderId'], 'deepseek');
    final providers = api.savedProviderSettings!['providers'] as List<Object?>;
    final openAi = providers.last! as Map<String, Object?>;
    expect(openAi['id'], 'openai-team');
    expect(openAi['originalId'], 'openai');
    final modelConnectionModes =
        openAi['modelConnectionModes'] as List<Object?>;
    final customModelConnection = modelConnectionModes
        .cast<Map<String, Object?>>()
        .singleWhere((mode) => mode['slug'] == 'gpt-5.5');
    expect(customModelConnection['connectionMode'], 'web_socket');
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
          state: ReadyProviderUsageView(
            data: ZhipuCodingPlanProviderUsageView(
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
      _providerListState(zhipuOnly: true).copyWith(
        providerUsageState: ProviderUsageStateSnapshot(usages: [staleUsage]),
      ),
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
      label: 'missingCredential',
      state: MissingCredentialProviderUsageView(
        message: 'Provider API key is not configured',
      ),
      shortMessage: 'Missing key',
      verboseMessage: 'Provider API key is not configured',
    ),
    (
      label: 'failed',
      state: FailedProviderUsageView(
        code: 'providerUsageQueryFailed',
        message: 'Usage query failed',
        retryable: true,
      ),
      shortMessage: 'Usage failed',
      verboseMessage: 'Usage query failed',
    ),
  ]) {
    testWidgets(
      'provider list shows compact ${usageState.label} hint without quota',
      (tester) async {
        _configureSettingsTestView(tester);
        final api = _FakeStudioApi(
          _providerListState(zhipuOnly: true),
          providerUsages: [
            ProviderUsageView(
              providerId: 'zhipu-coding-plan',
              updatedAt: 1,
              state: usageState.state,
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
      final settingsState = _withSettingsFixture(
        _withSelectedRuntime(
          base,
          base.runtime.copyWith(
            activeSkills: ['flutter-ui-polish', 'rust-review'],
          ),
        ),
        skills: const SkillsSettingsView(disabled: []),
        mcpServers: const [
          McpServerSettingsView(
            id: 'local',
            transport: 'stdio',
            endpoint: 'npx',
            state: McpCheckingState(message: 'pending'),
          ),
          McpServerSettingsView(
            id: 'remote',
            transport: 'http',
            endpoint: 'https://example.test/mcp',
            state: McpDisabledState(message: 'disabled'),
          ),
        ],
      );
      final api = _FakeStudioApi(
        settingsState.copyWith(
          mcpState: McpStateSnapshot(
            revision: 1,
            servers: settingsState.mcpServers,
          ),
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

  testWidgets('statistics tab shows weighted summaries and filters history', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        modelPerformance: _modelPerformanceFixture(),
      ),
    );
    await _pumpSettingsPage(tester, api);

    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('statistics')));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.statisticsSummary), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.statisticsHistory), findsOneWidget);
    expect(find.byType(DataTable), findsOneWidget);
    expect(find.text('120 t/s'), findsOneWidget);
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow('provider-a', 'model-a', 3000),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow('provider-b', 'model-b', 2000),
      ),
      findsOneWidget,
    );

    await tester.tap(find.byKey(StudioDriverKeys.statisticsFilter));
    await tester.pumpAndSettle();
    await tester.tap(find.text('OpenAI · model-b').last);
    await tester.pumpAndSettle();

    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow('provider-a', 'model-a', 3000),
      ),
      findsNothing,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow('provider-b', 'model-b', 2000),
      ),
      findsOneWidget,
    );
  });

  testWidgets('statistics tab uses virtualized cards in compact layout', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(700, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        modelPerformance: _modelPerformanceFixture(),
      ),
    );
    await _pumpSettingsPage(tester, api);

    final statisticsTab = find.byKey(
      StudioDriverKeys.settingsTab('statistics'),
    );
    await tester.scrollUntilVisible(
      statisticsTab,
      300,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.tap(statisticsTab);
    await tester.pumpAndSettle();

    expect(find.byType(DataTable), findsNothing);
    expect(find.byKey(StudioDriverKeys.statisticsSummary), findsOneWidget);
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow('provider-a', 'model-a', 3000),
      ),
      findsOneWidget,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'settings ordinary controls save immediately without draft buttons',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final settingsState = _withSettingsFixture(
        _withSelectedRuntime(
          _stateWithPlannerModels(),
          _stateWithPlannerModels().runtime.copyWith(
            activeSkills: ['flutter-ui-polish'],
          ),
        ),
        skills: const SkillsSettingsView(disabled: []),
        mcpServers: const [
          McpServerSettingsView(
            id: 'local',
            transport: 'stdio',
            endpoint: 'npx',
            state: McpCheckingState(message: 'pending'),
          ),
        ],
      );
      final api = _FakeStudioApi(
        settingsState.copyWith(
          mcpState: McpStateSnapshot(
            revision: 1,
            activeServers: const ['local'],
            servers: settingsState.mcpServers,
          ),
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
      for (final role in const [
        'explorer',
        'planner',
        'executor',
        'reviewer',
      ]) {
        expect(
          find.byKey(StudioDriverKeys.settingsRoleModel(role)),
          findsOneWidget,
        );
        expect(
          find.byKey(StudioDriverKeys.settingsRoleEffort(role)),
          findsOneWidget,
        );
      }

      await tester.tap(
        find.byKey(StudioDriverKeys.settingsRoleModel('explorer')),
      );
      await tester.pumpAndSettle();
      final flashOption = find.byKey(
        StudioDriverKeys.settingsRoleModelOption(
          'explorer',
          'deepseek',
          'deepseek-v4-flash',
        ),
      );
      expect(
        find.descendant(
          of: flashOption,
          matching: find.text(
            'DeepSeek / DeepSeek V4 Flash · 文本 · Responses · HTTP',
          ),
        ),
        findsOneWidget,
      );
      final reasonerOption = find.byKey(
        StudioDriverKeys.settingsRoleModelOption(
          'explorer',
          'deepseek',
          'deepseek-reasoner',
        ),
      );
      expect(
        find.descendant(
          of: reasonerOption,
          matching: find.text(
            'DeepSeek / DeepSeek Reasoner · 文本 · Chat Completions · HTTP',
          ),
        ),
        findsOneWidget,
      );
      expect(reasonerOption.hitTestable(), findsOneWidget);
      await tester.tap(reasonerOption);
      await tester.pumpAndSettle();
      expect(api.roleUpdate?.roleKey, 'explorer');
      expect(api.roleUpdate?.model, 'deepseek-reasoner');

      await tester.tap(
        find.byKey(StudioDriverKeys.settingsRoleEffort('planner')),
      );
      await tester.pumpAndSettle();
      expect(
        find
            .byKey(StudioDriverKeys.settingsRoleEffortOption('planner', 'max'))
            .hitTestable(),
        findsOneWidget,
      );
      await tester.tap(
        find.byKey(StudioDriverKeys.settingsRoleEffortOption('planner', 'max')),
      );
      await tester.pumpAndSettle();
      expect(api.roleUpdate?.roleKey, 'planner');
      expect(api.roleUpdate?.providerId, 'deepseek');
      expect(api.roleUpdate?.model, 'deepseek-v4-flash');
      expect(api.roleUpdate?.effort, 'max');

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

  testWidgets('web search settings show gating and save typed values', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _withSettingsFixture(
        _stateWithPlannerModels(),
        webSearch: const WebSearchSettingsView(
          configuredMode: 'cached',
          effectiveMode: 'disabled',
          availability: 'missingCredential',
        ),
      ),
    );
    await _pumpSettingsPage(tester, api);
    await tester.tap(find.text('General'));
    await tester.pumpAndSettle();

    expect(find.text('Web search'), findsOneWidget);
    expect(find.text('Missing credential'), findsOneWidget);
    expect(
      find.textContaining('Remote web search is fully disabled'),
      findsOneWidget,
    );

    await tester.tap(find.byType(DropdownButtonFormField<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Live').last);
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byType(TextField).first,
      'example.com, docs.example.com',
    );
    await tester.enterText(find.byType(TextField).at(1), 'US');
    await tester.enterText(find.byType(TextField).at(4), 'America/New_York');
    await tester.tap(find.text('Save web search'));
    await tester.pumpAndSettle();

    final saved = api.savedWebSearchSettings;
    expect(saved?.mode, 'live');
    expect(saved?.allowedDomains, ['example.com', 'docs.example.com']);
    expect(saved?.country, 'US');
    expect(saved?.timezone, 'America/New_York');
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
      await tester.tap(find.byKey(StudioDriverKeys.providerRow('deepseek')));
      await tester.pumpAndSettle();
      expect(find.text('DeepSeek Reasoner'), findsOneWidget);
      final capabilityTags = find.byKey(
        StudioDriverKeys.modelCapabilityTags('deepseek', 'deepseek-reasoner'),
      );
      expect(capabilityTags, findsOneWidget);
      expect(tester.widget<Text>(capabilityTags).data, '文本');
      final visionCapabilityTags = find.byKey(
        StudioDriverKeys.modelCapabilityTags(
          'deepseek',
          'deepseek-v4-flash-vision-exp',
        ),
      );
      expect(visionCapabilityTags, findsOneWidget);
      expect(tester.widget<Text>(visionCapabilityTags).data, '文本 · 视觉');

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
      expect(find.text('deepseek-v4-flash · 文本'), findsOneWidget);
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

ModelPerformanceSnapshotView _modelPerformanceFixture() {
  return ModelPerformanceSnapshotView(
    revision: 3,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(3000),
    sessionCosts: const [
      SessionCostView(
        rootThreadId: 'session-1',
        estimatedCosts: [
          RuntimeCostView(currency: 'CNY', amount: 0.14),
          RuntimeCostView(currency: 'USD', amount: 0.02),
        ],
        hasUnpricedUsage: false,
      ),
    ],
    summaries: const [
      ModelPerformanceSummaryView(
        providerInstanceId: 'provider-a',
        providerDisplayName: 'DeepSeek',
        model: 'model-a',
        sampleCount: 2,
        completionTokens: 150,
        totalTtftMillis: 200,
        totalDecodeMillis: 1250,
        totalResponseMillis: 1450,
        tokensPerSecond: 120,
        averageTtftMillis: 100,
        averageResponseMillis: 725,
      ),
      ModelPerformanceSummaryView(
        providerInstanceId: 'provider-b',
        providerDisplayName: 'OpenAI',
        model: 'model-b',
        sampleCount: 1,
        completionTokens: 30,
        totalTtftMillis: 80,
        totalDecodeMillis: 300,
        totalResponseMillis: 380,
        tokensPerSecond: 100,
        averageTtftMillis: 80,
        averageResponseMillis: 380,
      ),
    ],
    history: [
      ModelPerformanceSampleView(
        completedAt: DateTime.fromMillisecondsSinceEpoch(3000),
        providerInstanceId: 'provider-a',
        providerDisplayName: 'DeepSeek',
        model: 'model-a',
        completionTokens: 50,
        ttftMillis: 100,
        decodeMillis: 250,
        totalResponseMillis: 350,
        tokensPerSecond: 200,
      ),
      ModelPerformanceSampleView(
        completedAt: DateTime.fromMillisecondsSinceEpoch(2000),
        providerInstanceId: 'provider-b',
        providerDisplayName: 'OpenAI',
        model: 'model-b',
        completionTokens: 30,
        ttftMillis: 80,
        decodeMillis: 300,
        totalResponseMillis: 380,
        tokensPerSecond: 100,
      ),
    ],
  );
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
  );
  if (zhipuOnly) {
    return _withSettingsFixture(
      base,
      defaultProviderId: zhipu.id,
      providers: const [zhipu],
    );
  }
  final deepSeek = base.providers.single.copyWith(
    templateKind: 'deepseek',
    subtitle: 'DeepSeek Platform',
    hasBearerToken: true,
    modelCount: '2',
  );
  return _withSettingsFixture(
    base,
    defaultProviderId: deepSeek.id,
    providers: [deepSeek, zhipu],
  );
}

const _providerListUsages = [
  ProviderUsageView(
    providerId: 'deepseek',
    updatedAt: 1735689600,
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
  ProviderUsageView(
    providerId: 'zhipu-coding-plan',
    updatedAt: 1735689600,
    state: ReadyProviderUsageView(
      data: ZhipuCodingPlanProviderUsageView(
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
    ),
  ),
];
