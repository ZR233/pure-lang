part of '../widget_test.dart';

void registerShellSettingsTests() {
  testWidgets(
    'failed history load preserves content and retries the subscription',
    (tester) async {
      _configureResponsiveView(tester, const Size(1280, 800));
      final api = _FakeStudioApi(_stateWithPlannerModels())
        ..publishSnapshotOnSubscribe = true;
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      expect(api.threadSubscriptions, ['session-1']);
      api._thread.addError(StateError('history unavailable'));
      await tester.pumpAndSettle();
      expect(find.textContaining('history unavailable'), findsOneWidget);
      expect(find.byKey(StudioDriverKeys.sidebar), findsOneWidget);
      final before = api.threadSubscriptions.length;
      await tester.tap(find.text('Retry'));
      await tester.pumpAndSettle();
      expect(api.threadSubscriptions.length, before + 1);
      expect(find.textContaining('history unavailable'), findsNothing);
      expect(
        find.byKey(const ValueKey('agent-workspace-loading')),
        findsNothing,
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'recovery scan keeps known worktrees visible and retries failed checks',
    (tester) async {
      _configureSettingsTestView(tester);
      final initial = _stateWithPlannerModels().copyWith(
        recoveryState: _worktreeRecoverySnapshot(),
      );
      final api = _FakeStudioApi(initial);
      await _pumpSettingsPage(tester, api);
      await tester.tap(find.text('Agents'));
      await tester.pumpAndSettle();
      final checking = RecoveryStateSnapshot.fromState(
        state: RefreshingObservedResource<List<StudioRecoveryIssue>>(
          revision: 100,
          operation: 'check',
          operationId: 'recovery',
          startedAt: 1,
          lastCheckedAt: null,
          value: initial.recoveryIssues,
        ),
      );
      api._global.add(
        StudioBridgeEvent(payload: RecoveryStateChangedPayload(checking)),
      );
      await tester.pump();
      await tester.pump();
      expect(
        find.text('Checking conversation history and workspaces…'),
        findsOneWidget,
      );
      expect(find.text('pure-agent-child-1'), findsOneWidget);
      api._global.add(
        StudioBridgeEvent(
          payload: RecoveryStateChangedPayload(
            RecoveryStateSnapshot.fromState(
              state: DegradedObservedResource<List<StudioRecoveryIssue>>(
                revision: 101,
                failedAt: 2,
                lastCheckedAt: null,
                operation: 'check',
                error: const ObservedResourceError(
                  code: 'failed',
                  message: 'disk unavailable',
                  retryable: true,
                ),
                value: initial.recoveryIssues,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.textContaining('disk unavailable'), findsOneWidget);
      expect(find.text('pure-agent-child-1'), findsOneWidget);
      api._currentState = initial.copyWith(
        recoveryState: RecoveryStateSnapshot(
          values: initial.recoveryIssues,
          revision: 102,
        ),
      );
      await tester.tap(find.byKey(const ValueKey('recovery-check-retry')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('recovery-check-status')), findsNothing);
      expect(find.text('pure-agent-child-1'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('archived migration is visible on the fresh start page', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(640, 720));
    const archive = 'C:\\Users\\example\\.anywork.recovery-fixture';
    final state = _emptyState().copyWith(
      threadDirectory: const ThreadDirectoryWindow(),
      workspacesByThread: const {},
      workspaceUiByThread: const {},
      selectedThreadId: null,
      recoveryState: RecoveryStateSnapshot(
        values: const [
          StudioRecoveryIssue(
            id: 'fresh-start-archive',
            scope: RecoveryIssueScope.application,
            category: RecoveryIssueCategory.storage,
            availableActions: [],
            detail: archive,
          ),
        ],
        revision: 1,
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(_FakeStudioApi(state))],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('Previous data was archived'), findsOneWidget);
    expect(find.text(archive), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.startPage), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('recovery-archive-dismiss')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Previous data was archived'), findsNothing);
    expect(find.text(archive), findsNothing);
    expect(find.byKey(StudioDriverKeys.startPage), findsOneWidget);
    expect(tester.takeException(), isNull);
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
    expect(find.byKey(const ValueKey('thread-menu-session-1')), findsOneWidget);

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
    expect(api.createdThreadMode, ThreadModeId.simple);
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
    expect(find.byTooltip('Main agent model'), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.sessionMode));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(StudioDriverKeys.sessionModeOption(ThreadModeId.task.name)),
    );
    await tester.pumpAndSettle();
    expect(find.text('Task'), findsOneWidget);
    expect(find.text('deepseek-v4-pro'), findsOneWidget);
    expect(find.text('max'), findsOneWidget);
    expect(find.byTooltip('Main agent model'), findsOneWidget);
    expect(api.createdThreadProjectId, isNull);

    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'plan the first turn',
    );
    await tester.pump();
    await tester.tap(find.byKey(StudioDriverKeys.composerSubmit));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.createdThreadMode, ThreadModeId.task);
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
    api.publishSnapshotOnSubscribe = true;
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
      find.byKey(StudioDriverKeys.sessionModeOption(ThreadModeId.task.name)),
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
      mode: ThreadModeId.simple,
      updatedAt: DateTime.fromMillisecondsSinceEpoch(1),
      workspacePath: first.workspacePath,
    );
    final state = initial.copyWith(
      threadDirectory: ThreadDirectoryWindow(threads: [first, second]),
    );
    final api = _FakeStudioApi(state)
      ..archiveThreadState = state.copyWith(
        threadDirectory: ThreadDirectoryWindow(threads: [second]),
        selectedThreadId: second.id,
        workspacesByThread: {
          ...state.workspacesByThread,
          second.id: state.selectedWorkspace!.copyWith(thread: second),
        },
      );
    api.publishSnapshotOnSubscribe = true;
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(ValueKey('thread-menu-${first.id}')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.archiveThread(first.id)));
    await tester.pumpAndSettle();
    expect(
      find.textContaining('worktree along with any uncommitted'),
      findsOneWidget,
    );
    await tester.tap(find.byKey(StudioDriverKeys.archiveThreadConfirm));
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
      ..archiveThreadError = const StudioFailure(
        code: StudioFailureCode.busy,
        message: 'The session could not be ended before archiving',
        retryable: true,
        correlationId: 'busy-1',
      );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(ValueKey('thread-menu-${'session-1'}')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.archiveThread('session-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.archiveThreadConfirm));
    await tester.pumpAndSettle();

    expect(api.archiveThreadCallCount, 1);
    expect(
      find.text('Could not archive this session. It may still be running.'),
      findsOneWidget,
    );
    expect(find.byKey(StudioDriverKeys.threadRow('session-1')), findsOneWidget);
  });

  testWidgets('sidebar archive shows a redacted failure and diagnostic ID', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_emptyState())
      ..archiveThreadError = const StudioFailure(
        code: StudioFailureCode.internal,
        message: 'An internal Studio error occurred',
        retryable: false,
        correlationId: 'studio-123',
      );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey('thread-menu-session-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.archiveThread('session-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.archiveThreadConfirm));
    await tester.pumpAndSettle();

    expect(api.archiveThreadCallCount, 1);
    expect(
      find.text(
        'Could not archive this session: An internal Studio error occurred '
        '(diagnostic ID: studio-123)',
      ),
      findsOneWidget,
    );
    expect(find.byKey(StudioDriverKeys.threadRow('session-1')), findsOneWidget);
  });

  testWidgets('sidebar rename updates the canonical title in the header', (
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

    await tester.tap(find.byKey(const ValueKey('thread-menu-session-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.renameThread('session-1')));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.renameThreadDialog('session-1')),
      findsOneWidget,
    );
    await tester.enterText(
      find.byKey(StudioDriverKeys.renameThreadInput('session-1')),
      'Manual title',
    );
    await tester.tap(
      find.byKey(StudioDriverKeys.renameThreadSave('session-1')),
    );
    await tester.pumpAndSettle();

    expect(api.renamedThreadId, 'session-1');
    expect(api.renamedThreadTitle, 'Manual title');
    expect(find.text('Manual title'), findsAtLeastNWidgets(1));
  });

  testWidgets('thread directory title event updates the selected header', (
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
    api.emitGlobal(
      StudioBridgeEvent(
        payload: ThreadDirectoryChangedPayload(
          upserted: [
            _emptyState().threads.single.copyWith(title: 'Auto title'),
          ],
          removed: const [],
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Auto title'), findsAtLeastNWidgets(1));
  });

  testWidgets('narrow drawer keeps project new-session and archive actions', (
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
    await tester.tap(find.byKey(const ValueKey('sidebar-toggle')));
    await tester.pumpAndSettle();

    final sidebar = find.byKey(StudioDriverKeys.sidebar);
    final newSession = find.byKey(StudioDriverKeys.newSession);
    expect(newSession, findsOneWidget);
    expect(find.byKey(const ValueKey('thread-menu-session-1')), findsOneWidget);
    // compact rail 同样移除品牌头，不再渲染应用 logo 或名字。
    expect(
      find.descendant(
        of: sidebar,
        matching: find.byIcon(Icons.auto_awesome_motion),
      ),
      findsNothing,
    );
    final localizedTitle = AppLocalizations.of(
      tester.element(find.byKey(StudioDriverKeys.shell)),
    ).appTitle;
    expect(
      find.descendant(of: sidebar, matching: find.text(localizedTitle)),
      findsNothing,
    );
    // 新建会话仍保留，并紧贴侧栏顶部 12px 起始。
    expect(newSession.hitTestable(), findsOneWidget);
    await tester.tap(newSession);
    await tester.pumpAndSettle();
    expect(api.createdThreadProjectId, isNull);
    expect(find.byKey(StudioDriverKeys.startPage), findsOneWidget);
  });

  testWidgets('project recovery issue detail overrides the name tooltip', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _sidebarTooltipNameState().copyWith(
      recoveryState: RecoveryStateSnapshot(
        revision: 5,
        values: const [
          StudioRecoveryIssue(
            id: 'sidebar-tooltip-recovery-1',
            scope: RecoveryIssueScope.project,
            category: RecoveryIssueCategory.repository,
            availableActions: [RecoveryIssueAction.removeProject],
            detail: _sidebarTooltipIssueDetail,
            projectId: 'project-1',
          ),
        ],
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

    final projectRow = find.byKey(StudioDriverKeys.projectRow('project-1'));
    final threadRow = find.byKey(StudioDriverKeys.threadRow('session-1'));
    expect(
      find.descendant(
        of: projectRow,
        matching: find.byTooltip(_sidebarTooltipIssueDetail),
      ),
      findsOneWidget,
    );
    expect(
      find.descendant(
        of: projectRow,
        matching: find.byTooltip(_sidebarTooltipProjectName),
      ),
      findsNothing,
    );
    // 无 issue 的会话仍保留完整标题提示。
    expect(
      find.descendant(
        of: threadRow,
        matching: find.byTooltip(_sidebarTooltipThreadTitle),
      ),
      findsOneWidget,
    );

    // 悬停行为验证：issue 行悬停标题区域显示诊断 overlay，而不是名称。
    final issueDetailCountBefore = find
        .text(_sidebarTooltipIssueDetail)
        .evaluate()
        .length;
    final nameCountBefore = find
        .text(_sidebarTooltipProjectName)
        .evaluate()
        .length;
    final issueTooltip = find.descendant(
      of: projectRow,
      matching: find.byTooltip(_sidebarTooltipIssueDetail),
    );
    final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer();
    await gesture.moveTo(
      tester.getCenter(
        find.descendant(
          of: projectRow,
          matching: find.text(_sidebarTooltipProjectName),
        ),
      ),
    );
    await _pumpTooltipHover(tester, _tooltipHoverWait(tester, issueTooltip));
    expect(
      find.text(_sidebarTooltipIssueDetail).evaluate().length,
      issueDetailCountBefore + 1,
      reason: 'issue 行悬停后 overlay 应显示 issue.detail',
    );
    expect(
      find.text(_sidebarTooltipProjectName).evaluate().length,
      nameCountBefore,
      reason: 'issue 行悬停不应出现名称 overlay',
    );

    await gesture.moveTo(const Offset(1, 1));
    await tester.pumpAndSettle();
    await gesture.removePointer();
  });

  testWidgets('selected busy root Thread can be archived after confirmation', (
    tester,
  ) async {
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
        child: _localizedApp(
          home: const StudioShell(),
          disableAnimations: true,
        ),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey('thread-menu-session-1')));
    await tester.pumpAndSettle();
    final action = tester.widget<PopupMenuItem<String>>(
      find.byKey(StudioDriverKeys.archiveThread('session-1')),
    );
    expect(action.enabled, isTrue);

    await tester.tap(find.byKey(StudioDriverKeys.archiveThread('session-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.archiveThreadConfirm));
    await tester.pumpAndSettle();

    expect(api.archiveThreadCallCount, 1);
    expect(api.archivedThreadId, 'session-1');
  });

  testWidgets('sidebar archive confirmation cancels without archiving', (
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

    await tester.tap(find.byKey(const ValueKey('thread-menu-session-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.archiveThread('session-1')));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.archiveThreadConfirm), findsOneWidget);

    await tester.tap(
      find.descendant(
        of: find.byType(AlertDialog),
        matching: find.text('Cancel'),
      ),
    );
    await tester.pumpAndSettle();

    expect(api.archiveThreadCallCount, 0);
    expect(find.byKey(StudioDriverKeys.archiveThreadConfirm), findsNothing);
    expect(find.byKey(StudioDriverKeys.threadRow('session-1')), findsOneWidget);
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
    await tester.tap(find.byKey(const ValueKey('add-project-local')));
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('add-project-continue')));
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

  testWidgets(
    'sidebar exposes add-project above project-owned new-session actions',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 800);
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
      final newSession = find.byKey(StudioDriverKeys.newSession);
      final openProject = find.byKey(StudioDriverKeys.openProject);
      final settings = find.byKey(StudioDriverKeys.settingsOpen);
      expect(sidebar, findsOneWidget);
      expect(newSession.hitTestable(), findsOneWidget);
      expect(openProject.hitTestable(), findsOneWidget);
      expect(settings.hitTestable(), findsOneWidget);
      expect(
        find.byTooltip('新建会话 · ${_emptyState().projects.first.name}'),
        findsOneWidget,
      );
      expect(find.text('添加项目'), findsOneWidget);
      expect(find.text('设置'), findsOneWidget);
      // 侧栏不再重复系统标题栏的 logo 与 app 名字。
      expect(
        find.descendant(
          of: sidebar,
          matching: find.byIcon(Icons.auto_awesome_motion),
        ),
        findsNothing,
      );
      final localizedTitle = AppLocalizations.of(
        tester.element(find.byKey(StudioDriverKeys.shell)),
      ).appTitle;
      expect(
        find.descendant(of: sidebar, matching: find.text(localizedTitle)),
        findsNothing,
      );
      expect(tester.getSize(sidebar).width, StudioLayout.sidebarWidth);
      expect(
        tester.getCenter(openProject).dy,
        lessThan(tester.getCenter(newSession).dy),
      );
      expect(
        tester.getCenter(openProject).dy,
        lessThan(tester.getCenter(settings).dy),
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'closing the busy selected project is rejected without legacy cleanup UI',
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
          child: _localizedApp(
            home: const StudioShell(),
            disableAnimations: true,
          ),
        ),
      );
      await tester.pump();
      await tester.pump();

      await tester.tap(find.byKey(const ValueKey('project-menu-project-a')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('project-close-project-a')));
      await tester.pumpAndSettle();
      expect(api.archivedProjectId, isNull);
      expect(find.textContaining('worktree'), findsNothing);
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
          modelRoute: ThreadModelRouteView(
            providerId: 'deepseek',
            model: 'deepseek-flash',
            effort: 'high',
            revision: 1,
            available: true,
          ),
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
    api.publishSnapshotOnSubscribe = true;
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(api.threadSubscriptions, ['session-1']);

    expect(find.byTooltip('Session mode'), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.sessionMode), findsOneWidget);
    expect(find.text('Simple'), findsOneWidget);
    expect(find.byTooltip('Main agent model'), findsOneWidget);
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
      find.byKey(StudioDriverKeys.sessionModeOption(ThreadModeId.task.name)),
    );
    await tester.pumpAndSettle();
    expect(api.modeUpdate?.threadId, 'session-1');
    expect(api.modeUpdate?.mode, ThreadModeId.task);
    expect(
      api.threadSubscriptions.where((threadId) => threadId == 'session-1'),
      hasLength(2),
    );
    expect(find.text('Task'), findsOneWidget);
    expect(find.text('deepseek-v4-pro'), findsOneWidget);
    expect(find.text('max'), findsOneWidget);
    expect(find.byTooltip('Main agent model'), findsOneWidget);

    await tester.tap(find.byTooltip('Main agent model'));
    await tester.pumpAndSettle();
    await tester.tap(find.textContaining('V4 Pro').last);
    await tester.pumpAndSettle();
    expect(api.threadModelRouteUpdate?.threadId, 'session-1');
    expect(api.threadModelRouteUpdate?.providerId, 'deepseek');
    expect(api.threadModelRouteUpdate?.model, 'deepseek-v4-pro');

    api.emitGlobal(
      _threadDirectoryChangedEvent(
        projectId: 'project-1',
        threads: [
          StudioThread(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Session',
            mode: ThreadModeId.task,
            updatedAt: DateTime.fromMillisecondsSinceEpoch(2000),
            workspacePath: '.',
          ),
        ],
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Reasoning effort'));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.reasoningEffortOption('max')),
      findsOneWidget,
    );
    await tester.tap(find.byKey(StudioDriverKeys.reasoningEffortOption('max')));
    await tester.pumpAndSettle();
    expect(api.threadModelRouteUpdate?.threadId, 'session-1');
    expect(api.threadModelRouteUpdate?.providerId, 'deepseek');
    expect(api.threadModelRouteUpdate?.model, 'deepseek-v4-pro');
    expect(api.threadModelRouteUpdate?.effort, 'max');
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
      modelPerformance: _modelPerformanceFixture(hasUnpricedUsage: true),
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
    api.publishSnapshotOnSubscribe = true;
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
    expect(
      find.descendant(
        of: sessionCost,
        matching: find.text('Partially unpriced'),
      ),
      findsNothing,
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
    expect(find.text('Partially unpriced'), findsNothing);
    expect(find.text('75 t/s'), findsOneWidget);
    expect(find.text('150 t/s'), findsNothing);
  });

  testWidgets(
    'child switcher shows the canonical task summary and full tooltip',
    (tester) async {
      tester.view.physicalSize = const Size(1047, 741);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final base = _rootAndChildState();
      const summary = '核对父智能体初始任务与后续补充消息的完整投递、持久化恢复和界面展示';
      final state = base.copyWith(
        threadDirectory: ThreadDirectoryWindow(
          threads: [
            for (final thread in base.threadDirectory.threads)
              if (thread.id == 'child-1')
                thread.copyWith(title: summary)
              else
                thread,
          ],
        ),
      );
      final api = _FakeStudioApi(state)..publishSnapshotOnSubscribe = true;
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.agentSwitcher));
      await tester.pumpAndSettle();
      final summaryFinder = find.byKey(
        StudioDriverKeys.agentTaskSummary('child-1'),
      );
      expect(summaryFinder, findsOneWidget);
      final text = tester.widget<Text>(summaryFinder);
      expect(text.data, summary);
      expect(text.maxLines, 1);
      final tooltip = tester.widget<Tooltip>(
        find.ancestor(of: summaryFinder, matching: find.byType(Tooltip)).first,
      );
      expect(tooltip.message, summary);
      final row = find.byKey(StudioDriverKeys.agentRow('child-1'));
      expect(
        find.descendant(of: row, matching: find.text('Reviewer')),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
      await tester.tap(row);
      await tester.pumpAndSettle();
      expect(find.text('child timeline'), findsOneWidget);
      await tester.tap(find.byKey(StudioDriverKeys.agentSwitcher));
      await tester.pumpAndSettle();
      expect(tester.widget<Text>(summaryFinder).data, summary);
    },
  );

  testWidgets('agent switcher groups rows by status and scrolls the overflow', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(1280, 800));
    const statuses = <String, ThreadStatusView>{
      'child-running': ThreadStatusView.running,
      'child-idle': ThreadStatusView.idle,
      'child-closed': ThreadStatusView.closed,
      'child-faulted': ThreadStatusView.faulted,
      'child-queued': ThreadStatusView.queued,
      'child-waiting': ThreadStatusView.waitingTool,
      'child-idle-2': ThreadStatusView.idle,
      'child-closing': ThreadStatusView.closing,
      'child-closed-2': ThreadStatusView.closed,
      'child-cancelling': ThreadStatusView.cancelling,
    };
    final base = _rootAndChildState();
    final root = base.threads.firstWhere((thread) => thread.id == 'session-1');
    final ids = statuses.keys.toList();
    final children = [
      for (final id in ids)
        StudioThread(
          id: id,
          projectId: root.projectId,
          title: id,
          mode: ThreadModeId.task,
          createdAt: _fixtureDate(100 + ids.indexOf(id)),
          updatedAt: _fixtureDate(100),
          parentThreadId: root.id,
          rootThreadId: root.id,
          agentPath: 'root/$id',
          role: 'reviewer',
          status: statuses[id]!,
          workspacePath: root.workspacePath,
        ),
    ];
    final state = base.copyWith(
      threadDirectory: ThreadDirectoryWindow(threads: [root, ...children]),
    );
    final api = _FakeStudioApi(state)..publishSnapshotOnSubscribe = true;
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.agentSwitcher));
    await tester.pumpAndSettle();

    const expectedOrder = [
      // 执行中
      'child-running',
      'child-queued',
      'child-waiting',
      'child-cancelling',
      // 失败
      'child-faulted',
      // 空闲，owner 保持同组内的原始顺序
      'session-1',
      'child-idle',
      'child-idle-2',
      // 关闭中或已关闭
      'child-closed',
      'child-closing',
      'child-closed-2',
    ];
    final tops = [
      for (final id in expectedOrder)
        tester.getTopLeft(find.byKey(StudioDriverKeys.agentRow(id))).dy,
    ];
    expect(tops, orderedEquals([...tops]..sort()));

    final menuScrollable = find
        .ancestor(
          of: find.byKey(StudioDriverKeys.agentRow(expectedOrder.last)),
          matching: find.byType(Scrollable),
        )
        .first;
    expect(
      find.ancestor(of: menuScrollable, matching: find.byType(Scrollbar)),
      findsWidgets,
    );
    expect(tester.getSize(menuScrollable).height, lessThanOrEqualTo(320));
    expect(
      tester
          .widget<Scrollable>(menuScrollable)
          .controller!
          .position
          .maxScrollExtent,
      greaterThan(0),
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('header shows a dash when the session has no priced costs', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _rootAndChildState().copyWith(
      modelPerformance: _modelPerformanceFixture(
        hasUnpricedUsage: true,
        estimatedCosts: const [],
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

    final sessionCost = find.byKey(StudioDriverKeys.sessionCost);
    expect(
      find.descendant(of: sessionCost, matching: find.text('-')),
      findsOneWidget,
    );
    expect(
      find.descendant(
        of: sessionCost,
        matching: find.text('Partially unpriced'),
      ),
      findsNothing,
    );
  });

  testWidgets(
    'dense shell uses a readable navigation drawer without overflow',
    (tester) async {
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
      await tester.tap(find.byKey(const ValueKey('sidebar-toggle')));
      await tester.pumpAndSettle();

      expect(
        tester.getSize(find.byKey(const ValueKey('studio-sidebar'))).width,
        360.0,
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
        'Session mode cannot change while the session is running or a workflow is active',
      ),
      findsOneWidget,
    );
    await tester.tap(find.byKey(StudioDriverKeys.sessionMode));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.sessionModeOption(ThreadModeId.task.name)),
      findsNothing,
    );
    expect(api.modeUpdate, isNull);
    expect(tester.takeException(), isNull);
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

  testWidgets(
    'SSH reconnect reserves each server and retries failures without stale success',
    (tester) async {
      _configureSettingsTestView(tester);
      const first = SshServer(
        alias: 'first',
        hostName: 'host-one',
        port: 22,
        username: 'runner',
        managed: true,
      );
      const second = SshServer(
        alias: 'second',
        hostName: 'host-two',
        port: 22,
        username: 'runner',
        managed: true,
      );
      final gates = <String, Completer<SshConnectionView>>{
        first.alias: Completer<SshConnectionView>(),
        second.alias: Completer<SshConnectionView>(),
      };
      final api = _FakeStudioApi(_emptyState())
        ..sshServers = const [first, second]
        ..reconnectSshHandler = (id) => gates[id]!.future;
      await _pumpSettingsPage(tester, api);
      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
      await tester.pumpAndSettle();

      final firstButton = tester.widget<TextButton>(
        find.byKey(StudioDriverKeys.sshReconnect(first.alias)),
      );
      final firstTest = tester.widget<TextButton>(
        find.byKey(StudioDriverKeys.sshTest(first.alias)),
      );
      firstButton.onPressed!();
      firstButton.onPressed!();
      firstTest.onPressed!();
      await tester.pump();
      expect(api.reconnectSshCalls, [first.alias]);
      expect(api.testedSshServerAlias, isNull);
      expect(
        tester
            .widget<TextButton>(
              find.byKey(StudioDriverKeys.sshReconnect(first.alias)),
            )
            .onPressed,
        isNull,
      );
      expect(
        tester
            .widget<TextButton>(
              find.byKey(StudioDriverKeys.sshOpen(first.alias)),
            )
            .onPressed,
        isNull,
      );
      expect(
        find.descendant(
          of: find.byKey(StudioDriverKeys.sshReconnect(first.alias)),
          matching: find.byType(CircularProgressIndicator),
        ),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: find.byKey(StudioDriverKeys.sshTest(first.alias)),
          matching: find.byType(CircularProgressIndicator),
        ),
        findsNothing,
      );

      tester
          .widget<TextButton>(
            find.byKey(StudioDriverKeys.sshReconnect(second.alias)),
          )
          .onPressed!();
      await tester.pump();
      expect(api.reconnectSshCalls, [first.alias, second.alias]);
      gates[first.alias]!.complete(
        const SshConnectionView(
          alias: 'first',
          state: 'ready',
          helperVersion: 'new-helper',
          architecture: 'x86_64',
        ),
      );
      await tester.pump();
      expect(find.text('x86_64 · helper new-helper'), findsOneWidget);
      expect(
        tester
            .widget<TextButton>(
              find.byKey(StudioDriverKeys.sshReconnect(second.alias)),
            )
            .onPressed,
        isNull,
      );

      gates[first.alias] = Completer<SshConnectionView>();
      tester
          .widget<TextButton>(
            find.byKey(StudioDriverKeys.sshReconnect(first.alias)),
          )
          .onPressed!();
      gates[first.alias]!.completeError(
        StateError('environment initialization failed'),
      );
      await tester.pump();
      expect(find.text('x86_64 · helper new-helper'), findsNothing);
      expect(
        find.textContaining('environment initialization failed'),
        findsOneWidget,
      );
      expect(
        tester
            .widget<TextButton>(
              find.byKey(StudioDriverKeys.sshReconnect(first.alias)),
            )
            .onPressed,
        isNotNull,
      );

      gates[first.alias] = Completer<SshConnectionView>();
      tester
          .widget<TextButton>(
            find.byKey(StudioDriverKeys.sshReconnect(first.alias)),
          )
          .onPressed!();
      gates[first.alias]!.complete(
        const SshConnectionView(
          alias: 'first',
          state: 'ready',
          helperVersion: 'refreshed',
          architecture: 'x86_64',
        ),
      );
      gates[second.alias]!.complete(
        const SshConnectionView(
          alias: 'second',
          state: 'ready',
          helperVersion: 'second-helper',
          architecture: 'aarch64',
        ),
      );
      await tester.pumpAndSettle();
      expect(
        find.textContaining('environment initialization failed'),
        findsNothing,
      );
      expect(find.text('x86_64 · helper refreshed'), findsOneWidget);
      expect(find.text('aarch64 · helper second-helper'), findsOneWidget);
    },
  );

  testWidgets('SSH settings delegate connection and project actions to core', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const server = SshServer(
      alias: 'ssh-arm',
      hostName: '192.168.100.12',
      port: 22,
      username: 'root',
      managed: true,
    );
    final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
    api.selectProjectStates['remote-project'] = _remoteProjectAdoptedState();
    await _pumpSettingsPage(tester, api);

    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
    await tester.pumpAndSettle();
    expect(find.text('root@192.168.100.12:22'), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.sshReconnect(server.alias)),
      findsOneWidget,
    );

    await tester.tap(find.byKey(StudioDriverKeys.sshTest(server.alias)));
    await tester.pumpAndSettle();
    expect(api.testedSshServerAlias, server.alias);
    expect(find.text('aarch64 · helper 0.1.0'), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.sshReconnect(server.alias)));
    await tester.pumpAndSettle();
    expect(api.reconnectedSshServerAlias, server.alias);

    await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);
    expect(api.browsedRemoteDirectory?.alias, server.alias);
    await tester.tap(find.text('Open this directory'));
    await tester.pumpAndSettle();
    expect(api.openedRemoteProject, (alias: server.alias, path: '/workspace'));
  });

  testWidgets('SSH server dialog validates and saves a complete profile', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    final api = _FakeStudioApi(_emptyState());
    await _pumpSettingsPage(tester, api);

    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sshAddServer));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.sshServerSave));
    await tester.pump();
    expect(
      find.byKey(StudioDriverKeys.sshServerValidationError),
      findsOneWidget,
    );
    expect(find.text('Enter an alias'), findsOneWidget);

    await tester.enterText(
      find.byKey(StudioDriverKeys.sshServerAliasInput),
      'qcl-server',
    );
    await tester.enterText(
      find.byKey(StudioDriverKeys.sshServerHostInput),
      '10.3.10.9',
    );
    await tester.enterText(
      find.byKey(StudioDriverKeys.sshServerUsernameInput),
      'zhourui',
    );
    await tester.enterText(
      find.byKey(StudioDriverKeys.sshServerPortInput),
      '22',
    );
    await tester.tap(find.byKey(StudioDriverKeys.sshServerSave));
    await tester.pumpAndSettle();

    expect(api.savedSshServer?.alias, 'qcl-server');
    expect(api.savedSshServer?.hostName, '10.3.10.9');
    expect(api.savedSshServer?.username, 'zhourui');
    expect(api.savedSshServer?.port, 22);
    expect(find.text('zhourui@10.3.10.9:22'), findsOneWidget);
  });

  testWidgets(
    'SSH directory dialog syncs a manual absolute path and opens it',
    (tester) async {
      _configureSettingsTestView(tester);
      const server = SshServer(
        alias: 'ssh-arm',
        hostName: '192.168.100.12',
        port: 22,
        username: 'root',
        managed: true,
      );
      final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
      api.selectProjectStates['remote-project'] = _remoteProjectAdoptedState(
        path: '/workspace/project',
      );
      await _pumpSettingsPage(tester, api);

      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
      await tester.pumpAndSettle();

      expect(
        find.byKey(StudioDriverKeys.sshDirectoryPathInput),
        findsOneWidget,
      );
      expect(_dialogText('root@192.168.100.12:22'), findsOneWidget);

      await tester.enterText(
        find.byKey(StudioDriverKeys.sshDirectoryPathInput),
        '/workspace/project',
      );
      await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
      await tester.pumpAndSettle();

      expect(api.browsedRemoteDirectory, (
        alias: server.alias,
        path: '/workspace/project',
      ));
      expect(
        find.byKey(StudioDriverKeys.sshDirectoryCurrent('/workspace/project')),
        findsOneWidget,
      );

      await tester.tap(find.text('Open this directory'));
      await tester.pumpAndSettle();
      expect(api.openedRemoteProject, (
        alias: server.alias,
        path: '/workspace/project',
      ));
    },
  );

  testWidgets('SSH directory dialog treats Enter like the Go button', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const server = SshServer(
      alias: 'ssh-arm',
      hostName: '192.168.100.12',
      port: 22,
      username: 'root',
      managed: true,
    );
    final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
    await _pumpSettingsPage(tester, api);

    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(StudioDriverKeys.sshDirectoryPathInput),
      '/workspace/project',
    );
    expect(
      tester
          .widget<TextField>(find.byKey(StudioDriverKeys.sshDirectoryPathInput))
          .textInputAction,
      TextInputAction.go,
    );
    await tester.testTextInput.receiveAction(TextInputAction.go);
    await tester.pumpAndSettle();

    expect(api.browsedRemoteDirectory, (
      alias: server.alias,
      path: '/workspace/project',
    ));
  });

  testWidgets(
    'SSH directory dialog rejects invalid paths without calling the API',
    (tester) async {
      _configureSettingsTestView(tester);
      const server = SshServer(
        alias: 'ssh-arm',
        hostName: '192.168.100.12',
        port: 22,
        username: 'root',
        managed: true,
      );
      final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
      await _pumpSettingsPage(tester, api);

      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
      await tester.pumpAndSettle();

      await tester.enterText(
        find.byKey(StudioDriverKeys.sshDirectoryPathInput),
        '',
      );
      await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
      await tester.pump();
      expect(find.byKey(StudioDriverKeys.sshDirectoryError), findsOneWidget);
      expect(find.text('Enter a remote directory path'), findsOneWidget);
      expect(api.browsedRemoteDirectory, (alias: server.alias, path: null));

      await tester.enterText(
        find.byKey(StudioDriverKeys.sshDirectoryPathInput),
        'relative/path',
      );
      await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
      await tester.pump();
      expect(
        find.text('Path must be an absolute POSIX path starting with /'),
        findsOneWidget,
      );
      expect(api.browsedRemoteDirectory, (alias: server.alias, path: null));

      for (final invalid in const [
        '~',
        'ssh://example.test/workspace',
        'root@example.test:/workspace',
      ]) {
        await tester.enterText(
          find.byKey(StudioDriverKeys.sshDirectoryPathInput),
          invalid,
        );
        await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
        await tester.pump();
        expect(
          find.text('Path must be an absolute POSIX path starting with /'),
          findsOneWidget,
        );
      }
      expect(api.browseRemoteCallCount, 1);
    },
  );

  testWidgets(
    'SSH directory dialog disables open when input drifts from the loaded path',
    (tester) async {
      _configureSettingsTestView(tester);
      const server = SshServer(
        alias: 'ssh-arm',
        hostName: '192.168.100.12',
        port: 22,
        username: 'root',
        managed: true,
      );
      final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
      await _pumpSettingsPage(tester, api);

      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
      await tester.pumpAndSettle();

      final openButton = find.byKey(StudioDriverKeys.sshOpenCurrentDirectory);
      expect(tester.widget<FilledButton>(openButton).onPressed, isNotNull);

      await tester.enterText(
        find.byKey(StudioDriverKeys.sshDirectoryPathInput),
        '/other',
      );
      await tester.pump();
      expect(tester.widget<FilledButton>(openButton).onPressed, isNull);
    },
  );

  testWidgets(
    'SSH directory dialog navigates into a subdirectory and back up',
    (tester) async {
      _configureSettingsTestView(tester);
      const server = SshServer(
        alias: 'ssh-arm',
        hostName: '192.168.100.12',
        port: 22,
        username: 'root',
        managed: true,
      );
      final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
      api.remoteDirListings['/workspace'] = const RemoteDirectoryListing(
        path: '/workspace',
        parent: '/',
        entries: [
          RemoteDirectoryEntry(name: 'project', path: '/workspace/project'),
        ],
      );
      api.remoteDirListings['/workspace/project'] =
          const RemoteDirectoryListing(
            path: '/workspace/project',
            parent: '/workspace',
            entries: [
              RemoteDirectoryEntry(name: 'src', path: '/workspace/project/src'),
            ],
          );
      await _pumpSettingsPage(tester, api);

      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
      await tester.pumpAndSettle();

      expect(
        find.byKey(StudioDriverKeys.sshDirectoryCurrent('/workspace')),
        findsOneWidget,
      );

      await tester.tap(
        find.byKey(StudioDriverKeys.sshDirectoryEntry('/workspace/project')),
      );
      await tester.pumpAndSettle();
      expect(api.browsedRemoteDirectory, (
        alias: server.alias,
        path: '/workspace/project',
      ));
      expect(
        find.byKey(StudioDriverKeys.sshDirectoryCurrent('/workspace/project')),
        findsOneWidget,
      );

      await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryUp));
      await tester.pumpAndSettle();
      expect(api.browsedRemoteDirectory, (
        alias: server.alias,
        path: '/workspace',
      ));
      expect(
        find.byKey(StudioDriverKeys.sshDirectoryCurrent('/workspace')),
        findsOneWidget,
      );
    },
  );

  testWidgets('SSH directory dialog shows a localized empty state', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const server = SshServer(
      alias: 'ssh-arm',
      hostName: '192.168.100.12',
      port: 22,
      username: 'root',
      managed: true,
    );
    final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
    api.remoteDirListings['/workspace'] = const RemoteDirectoryListing(
      path: '/workspace',
      parent: '/',
      entries: [],
    );
    await _pumpSettingsPage(tester, api);

    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.sshDirectoryEmpty), findsOneWidget);
    expect(find.text('This directory is empty'), findsOneWidget);
    expect(
      find.text('No subdirectories here — you can still open this directory.'),
      findsOneWidget,
    );
    final openButton = tester.widget<FilledButton>(
      find.byKey(StudioDriverKeys.sshOpenCurrentDirectory),
    );
    expect(openButton.onPressed, isNotNull);
  });

  testWidgets('SSH directory dialog recovers from a browse failure', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const server = SshServer(
      alias: 'ssh-arm',
      hostName: '192.168.100.12',
      port: 22,
      username: 'root',
      managed: true,
    );
    final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
    api.browseRemoteError = StateError('unreachable');
    await _pumpSettingsPage(tester, api);

    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.sshDirectoryError), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);

    api.browseRemoteError = null;
    await tester.enterText(
      find.byKey(StudioDriverKeys.sshDirectoryPathInput),
      '/workspace',
    );
    await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.sshDirectoryError), findsNothing);
    expect(find.byKey(StudioDriverKeys.sshDirectoryList), findsOneWidget);
    expect(api.browsedRemoteDirectory, (
      alias: server.alias,
      path: '/workspace',
    ));

    // 已有 listing 后再次浏览同一路径失败时，旧结果不能绕过 canonical
    // 校验重新启用 Open；修正连接后仍可在原上下文重试。
    api.browseRemoteError = StateError('temporary browse failure');
    await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.sshDirectoryError), findsOneWidget);
    expect(
      tester
          .widget<FilledButton>(
            find.byKey(StudioDriverKeys.sshOpenCurrentDirectory),
          )
          .onPressed,
      isNull,
    );

    api.browseRemoteError = null;
    await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.sshDirectoryError), findsNothing);
    expect(
      tester
          .widget<FilledButton>(
            find.byKey(StudioDriverKeys.sshOpenCurrentDirectory),
          )
          .onPressed,
      isNotNull,
    );
  });

  testWidgets('SSH directory dialog keeps the window on open failure', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const server = SshServer(
      alias: 'ssh-arm',
      hostName: '192.168.100.12',
      port: 22,
      username: 'root',
      managed: true,
    );
    final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
    api.openRemoteProjectError = StateError('remote open failed');
    await _pumpSettingsPage(tester, api);

    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Open this directory'));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.sshDirectoryError), findsOneWidget);
    expect(api.openedRemoteProject, (alias: server.alias, path: '/workspace'));
    final pathController = tester
        .widget<TextField>(find.byKey(StudioDriverKeys.sshDirectoryPathInput))
        .controller;
    expect(
      pathController,
      isNotNull,
      reason: 'The SSH directory path field must expose its input controller.',
    );
    expect(pathController!.text, '/workspace');

    api.openRemoteProjectError = null;
    api.selectProjectStates['remote-project'] = _remoteProjectAdoptedState();
    await tester.tap(find.text('Open this directory'));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
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
      find.descendant(of: unavailableRow, matching: find.text('Unavailable')),
      findsOneWidget,
    );
    expect(
      find.descendant(of: availableRow, matching: find.text('Available')),
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

  testWidgets('OpenAI provider template saves a selected transport', (
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
    final savedOpenAi = (await api.readStudioState()).providers.singleWhere(
      (provider) => provider.id == 'openai',
    );
    expect(savedOpenAi.pricingEnabled, isTrue);
    expect(savedOpenAi.hostedWebSearch, isTrue);
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
    await tester.fling(editor, const Offset(0, 1000), 2000);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    final providers = api.savedProviderSettings!['providers'] as List<Object?>;
    final future = providers.last! as Map<String, Object?>;
    expect(future['templateKind'], 'future-provider');
    expect(future['defaultModel'], 'future-model');
    final savedFuture = (await api.readStudioState()).providers.singleWhere(
      (provider) => provider.templateKind == 'future-provider',
    );
    expect(savedFuture.standaloneWebSearch, 'future_search_dialect');
    expect(savedFuture.promptCacheDialect, 'implicit_prefix');
  });

  testWidgets(
    'compatible models save both supported APIs with explicit pricing choices',
    (tester) async {
      _configureSettingsTestView(tester);
      for (final protocol in ['chat_completions', 'responses']) {
        final api = _FakeStudioApi(_stateWithPlannerModels());
        await tester.pumpWidget(
          ProviderScope(
            key: ValueKey(protocol),
            overrides: [studioApiProvider.overrideWithValue(api)],
            child: _localizedApp(home: const SettingsPage()),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(StudioDriverKeys.providerAdd));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(StudioDriverKeys.providerPreset));
        await tester.pumpAndSettle();
        await tester.tap(find.text('OpenAI API 兼容').last);
        await tester.pumpAndSettle();
        final pricing = find.byKey(StudioDriverKeys.providerPricing);
        await tester.ensureVisible(pricing);
        expect(tester.widget<SwitchListTile>(pricing).value, isFalse);
        await tester.tap(pricing);
        await tester.ensureVisible(
          find.byKey(StudioDriverKeys.providerModelAdd),
        );
        await tester.tap(find.byKey(StudioDriverKeys.providerModelAdd));
        await tester.pumpAndSettle();
        final modelId = find.descendant(
          of: find.byKey(StudioDriverKeys.customModelId(0)),
          matching: find.byType(TextFormField),
        );
        await tester.ensureVisible(modelId);
        await tester.enterText(modelId, 'local-coder');
        await tester.pumpAndSettle();
        if (protocol == 'responses') {
          await tester.ensureVisible(find.text('Optional model settings'));
          await tester.tap(find.text('Optional model settings'));
          await tester.pumpAndSettle();
          final dropdown = find
              .widgetWithText(
                DropdownButtonFormField<String>,
                'Chat Completions (HTTP)',
              )
              .last;
          await tester.ensureVisible(dropdown);
          await tester.pumpAndSettle();
          expect(dropdown.hitTestable(), findsOneWidget);
          await tester.tap(dropdown);
          await tester.pumpAndSettle();
          await tester.tap(find.text('Responses (HTTP)').last);
          await tester.pumpAndSettle();
        }
        await tester.scrollUntilVisible(
          find.byKey(StudioDriverKeys.providerSave),
          -400,
          scrollable: find
              .descendant(
                of: find.byKey(StudioDriverKeys.providerEditorScroll),
                matching: find.byType(Scrollable),
              )
              .first,
        );
        await tester.tap(find.byKey(StudioDriverKeys.providerSave));
        await tester.pumpAndSettle();
        final saved = (await api.readStudioState()).providers.singleWhere(
          (provider) => provider.templateKind == 'openai-compatible',
        );
        expect(saved.defaultModel, 'local-coder');
        expect(saved.pricingEnabled, isTrue);
        expect(saved.customModels.single.wireProtocol, protocol);
        expect(saved.customModels.single.reasoningEfforts, isEmpty);
      }
    },
  );

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
      find.widgetWithText(TextFormField, 'Display name'),
      'OpenAI Team',
    );
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    expect(api.savedProviderSettings?['defaultProviderId'], 'deepseek');
    final providers = api.savedProviderSettings!['providers'] as List<Object?>;
    final openAi = providers.last! as Map<String, Object?>;
    expect(openAi['id'], 'openai');
    expect(openAi['name'], 'OpenAI Team');
    expect(openAi['originalId'], isNull);
    final modelConnectionModes =
        openAi['modelConnectionModes'] as List<Object?>;
    final customModelConnection = modelConnectionModes
        .cast<Map<String, Object?>>()
        .singleWhere((mode) => mode['slug'] == 'gpt-5.5');
    expect(customModelConnection['connectionMode'], 'web_socket');
  });

  testWidgets(
    'wide provider details preserve list usage and both refresh paths',
    (tester) async {
      tester.view.physicalSize = const Size(1600, 1000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final api = _FakeStudioApi(
        _providerListState(),
        providerUsages: _providerListUsages,
      );
      await _pumpSettingsPage(tester, api);
      await tester.tap(find.text('DeepSeek'));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.providerRow('deepseek')),
        findsOneWidget,
      );
      expect(find.text('CNY 88.00'), findsWidgets);
      expect(find.text('Granted 8.00'), findsOneWidget);
      expect(find.text('Topped up 80.00'), findsOneWidget);
      final before = api.loadProviderUsagesCount;
      await tester.tap(find.byKey(StudioDriverKeys.providerUsageCheck));
      await tester.pumpAndSettle();
      expect(api.loadProviderUsagesCount, before + 1);
      await tester.tap(find.byTooltip('Provider actions').first);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Refresh usage').last);
      await tester.pumpAndSettle();
      expect(api.loadProviderUsagesCount, before + 2);
      expect(find.text('Granted 8.00'), findsOneWidget);
      expect(tester.takeException(), isNull);
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
    final modeRoutes =
        api.savedProviderSettings!['modeRoutes'] as List<Object?>;
    final simple = modeRoutes.cast<Map<String, Object?>>().singleWhere(
      (route) => route['modeId'] == 'mode.simple',
    );
    expect(simple['provider'], 'zhipu-coding-plan');
    expect(simple['model'], 'glm-5.2');
    expect(simple['effort'], isEmpty);
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
    await tester.tap(find.text('DeepSeek'));
    await tester.pumpAndSettle();
    expect(find.text('Available balance'), findsOneWidget);
    expect(find.text('Granted 8.00'), findsOneWidget);
    expect(find.text('Topped up 80.00'), findsOneWidget);
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
    expect(find.text('120 t/s'), findsOneWidget);
    expect(
      find.byKey(
        StudioDriverKeys.statisticsSummaryRow('provider-a', 'model-a', 'high'),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsSummaryRow('provider-a', 'model-a', 'none'),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsSummaryRow('provider-a', 'model-a', null),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsSummaryRow('provider-b', 'model-b', null),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'high',
          0,
        ),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'high',
          1,
        ),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow('provider-a', 'model-a', null, 3),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'none',
          2,
        ),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow('provider-b', 'model-b', null, 4),
      ),
      findsOneWidget,
    );
    expect(tester.takeException(), isNull);

    final semantics = tester.ensureSemantics();
    expect(
      find.bySemanticsLabel(
        RegExp(
          'Configured model: catalog-model-a.*Sent model: model-a.*Reported model: provider-other.*Mismatch',
          dotAll: true,
        ),
      ),
      findsWidgets,
    );
    semantics.dispose();

    await tester.tap(find.byKey(StudioDriverKeys.statisticsFilter));
    await tester.pumpAndSettle();
    await tester.tap(find.text('DeepSeek · provider-a · model-a · high').last);
    await tester.pumpAndSettle();

    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'high',
          0,
        ),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'high',
          1,
        ),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'none',
          1,
        ),
      ),
      findsNothing,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow('provider-a', 'model-a', null, 1),
      ),
      findsNothing,
    );

    await tester.tap(find.byKey(StudioDriverKeys.statisticsFilter));
    await tester.pumpAndSettle();
    await tester.tap(
      find
          .text('OpenAI · provider-b · model-b · Unspecified / not recorded')
          .last,
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'high',
          0,
        ),
      ),
      findsNothing,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'none',
          0,
        ),
      ),
      findsNothing,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow('provider-b', 'model-b', null, 0),
      ),
      findsOneWidget,
    );

    await tester.tap(find.byKey(StudioDriverKeys.statisticsMismatchOnly));
    await tester.pumpAndSettle();
    expect(
      find.text('No model mismatches match these filters.'),
      findsOneWidget,
    );

    await tester.tap(find.byKey(StudioDriverKeys.statisticsFilter));
    await tester.pumpAndSettle();
    await tester.tap(find.text('DeepSeek · provider-a · model-a · high').last);
    await tester.pumpAndSettle();
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'high',
          0,
        ),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.statisticsHistoryRow(
          'provider-a',
          'model-a',
          'high',
          1,
        ),
      ),
      findsNothing,
    );
  });

  testWidgets(
    'compact statistics keep the same metrics and history accessible',
    (tester) async {
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

      expect(find.byKey(StudioDriverKeys.statisticsSummary), findsOneWidget);
      final highHistoryKey = StudioDriverKeys.statisticsHistoryRow(
        'provider-a',
        'model-a',
        'high',
        0,
      );
      final statisticsScrollable = find
          .descendant(
            of: find.byKey(StudioDriverKeys.statisticsHistory),
            matching: find.byType(Scrollable),
          )
          .first;
      await tester.scrollUntilVisible(
        find.byKey(highHistoryKey),
        300,
        scrollable: statisticsScrollable,
      );
      expect(find.byKey(highHistoryKey), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );

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

      await tester.tap(find.text('Agents'));
      await tester.pumpAndSettle();
      for (final role in const [
        'explorer',
        'executor',
        'worktree_executor',
        'reviewer',
      ]) {
        await tester.scrollUntilVisible(
          find.byKey(StudioDriverKeys.settingsRoleModel(role)),
          300,
          scrollable: _settingsPaneScrollable(),
        );
        expect(
          find.byKey(StudioDriverKeys.settingsRoleModel(role)),
          findsOneWidget,
        );
        expect(
          find.byKey(StudioDriverKeys.settingsRoleEffort(role)),
          findsOneWidget,
        );
      }

      await tester.scrollUntilVisible(
        find.byKey(StudioDriverKeys.settingsRoleModel('explorer')),
        -300,
        scrollable: _settingsPaneScrollable(),
      );
      await tester.ensureVisible(
        find.byKey(StudioDriverKeys.settingsRoleModel('explorer')),
      );
      await tester.tap(
        find.byKey(StudioDriverKeys.settingsRoleModel('explorer')),
      );
      await tester.pumpAndSettle();
      final flashOption = find.byKey(
        StudioDriverKeys.settingsRoleModelOption(
          'explorer',
          'deepseek',
          'deepseek-flash',
        ),
      );
      expect(
        find.descendant(
          of: flashOption,
          matching: find.text(
            'DeepSeek / DeepSeek V4.1 Flash · Text/Vision · Responses · HTTP',
          ),
        ),
        findsOneWidget,
      );
      final proOption = find.byKey(
        StudioDriverKeys.settingsRoleModelOption(
          'explorer',
          'deepseek',
          'deepseek-v4-pro',
        ),
      );
      expect(
        find.descendant(
          of: proOption,
          matching: find.text(
            'DeepSeek / DeepSeek V4 Pro · Text · Responses · HTTP',
          ),
        ),
        findsOneWidget,
      );
      expect(proOption.hitTestable(), findsOneWidget);
      await tester.tap(proOption);
      await tester.pumpAndSettle();
      expect(api.roleUpdate?.roleKey, 'explorer');
      expect(api.roleUpdate?.model, 'deepseek-v4-pro');

      await tester.tap(
        find.byKey(StudioDriverKeys.settingsRoleEffort('explorer')),
      );
      await tester.pumpAndSettle();
      expect(
        find
            .byKey(StudioDriverKeys.settingsRoleEffortOption('explorer', 'max'))
            .hitTestable(),
        findsOneWidget,
      );
      await tester.tap(
        find.byKey(
          StudioDriverKeys.settingsRoleEffortOption('explorer', 'max'),
        ),
      );
      await tester.pumpAndSettle();
      expect(api.roleUpdate?.roleKey, 'explorer');
      expect(api.roleUpdate?.providerId, 'deepseek');
      expect(api.roleUpdate?.model, 'deepseek-v4-pro');
      expect(api.roleUpdate?.effort, 'max');

      await tester.tap(find.text('Skills'));
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(const ValueKey('skill-enabled-flutter-ui-polish')),
      );
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

  testWidgets('instruction sections preserve and save all three documents', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    final api = _FakeStudioApi(_stateWithPlannerModels());
    await _pumpSettingsPage(tester, api);
    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('instructions')));
    await tester.pumpAndSettle();
    for (var index = 0; index < 3; index++) {
      await tester.tap(find.byKey(ValueKey('instruction-section-$index')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(ValueKey('instruction-editor-$index')),
        'document-$index',
      );
      await tester.pump(const Duration(milliseconds: 700));
      await tester.pumpAndSettle();
    }
    expect(
      {
        for (final name in ['baseOverride', 'developer', 'user'])
          name: api.savedInstructionsSettings?[name],
      },
      {
        'baseOverride': 'document-0',
        'developer': 'document-1',
        'user': 'document-2',
      },
    );
    await tester.tap(find.byKey(const ValueKey('instruction-section-0')));
    await tester.pumpAndSettle();
    expect(find.text('document-0'), findsOneWidget);
  });

  testWidgets(
    'provider draft survives settings navigation and saves the edited result',
    (tester) async {
      _configureSettingsTestView(tester);
      final api = _FakeStudioApi(_stateWithPlannerModels());
      await _pumpSettingsPage(tester, api);
      await tester.tap(find.byKey(StudioDriverKeys.providerRow('deepseek')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.providerEdit));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.widgetWithText(TextFormField, 'Display name'),
        'Preserved draft',
      );
      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('general')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('providers')));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.providerSave).hitTestable(),
        findsOneWidget,
      );
      expect(find.text('Preserved draft'), findsWidgets);
      await tester.tap(find.byKey(StudioDriverKeys.providerSave));
      await tester.pumpAndSettle();
      final providers =
          api.savedProviderSettings?['providers'] as List<Object?>;
      expect(
        providers.cast<Map<String, Object?>>().singleWhere(
          (item) => item['id'] == 'deepseek',
        )['name'],
        'Preserved draft',
      );
    },
  );

  testWidgets(
    'provider editor keeps an unresolvable canonical default model on save',
    (tester) async {
      _configureSettingsTestView(tester);
      final base = _withSettingsFixture(
        _stateWithPlannerModels(),
        providers: const [
          ProviderSettingsView(
            id: 'deepseek',
            templateKind: 'deepseek',
            name: 'DeepSeek',
            baseUrl: 'https://api.deepseek.com',
            defaultModel: 'ghost-model',
            models: [
              ProviderModelView(
                slug: 'deepseek-flash',
                displayName: 'DeepSeek V4.1 Flash',
                reasoningEfforts: [],
              ),
            ],
            status: 'ready',
            usageLabel: '',
          ),
        ],
      );
      final api = _FakeStudioApi(
        base.copyWith(providerCatalog: _testProviderCatalog),
        providerCatalog: _testProviderCatalog,
      );
      await _pumpSettingsPage(tester, api);
      await tester.tap(find.byKey(StudioDriverKeys.providerRow('deepseek')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.providerEdit));
      await tester.pumpAndSettle();

      // canonical default model 不可解析：显示原 slug 并标注 unavailable，
      // 不回退到 models.first。
      expect(find.text('ghost-model (unavailable)'), findsOneWidget);

      await tester.enterText(
        find.widgetWithText(TextFormField, 'Display name'),
        'Renamed',
      );
      await tester.tap(find.byKey(StudioDriverKeys.providerSave));
      await tester.pumpAndSettle();

      final providers =
          api.savedProviderSettings?['providers'] as List<Object?>;
      final saved = providers.cast<Map<String, Object?>>().singleWhere(
        (item) => item['id'] == 'deepseek',
      );
      expect(saved['name'], 'Renamed');
      expect(saved['defaultModel'], 'ghost-model');
    },
  );

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
        deepSeekWebSearch: const DeepSeekWebSearchSettingsView(
          configuredEnabled: true,
          effectiveEnabled: true,
          availability: 'available',
          selected: true,
          providerId: 'deepseek',
          model: 'deepseek-flash',
        ),
      ),
    );
    await _pumpSettingsPage(tester, api);
    await tester.tap(find.text('General'));
    await tester.pumpAndSettle();

    expect(find.text('Web search'), findsOneWidget);
    expect(find.text('DeepSeek native web search'), findsOneWidget);
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

    await tester.ensureVisible(
      find.byKey(const ValueKey('deepseek_web_search_enabled')),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('deepseek_web_search_enabled')));
    await tester.pumpAndSettle();
    expect(api.savedDeepSeekWebSearchSettings?.enabled, isFalse);
  });

  testWidgets('web search shows unknown runtime values instead of guessing', (
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
          configuredMode: 'turbo',
          effectiveMode: 'turbo',
          availability: 'providerDegraded',
          contextSize: 'extreme',
        ),
      ),
    );
    await _pumpSettingsPage(tester, api);
    await tester.tap(find.text('General'));
    await tester.pumpAndSettle();

    // 未知 mode / context size / availability / reason 展示原始值，
    // 不得分别伪装成 cached / medium / missing credential。
    expect(find.text('turbo'), findsWidgets);
    expect(find.text('extreme'), findsWidgets);
    expect(find.text('providerDegraded'), findsWidgets);
    expect(find.text('Cached'), findsNothing);
    expect(find.text('Medium'), findsNothing);
    expect(find.text('Missing credential'), findsNothing);
    expect(tester.takeException(), isNull);
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

      expect(find.text('模型服务商'), findsWidgets);
      expect(find.text('添加模型服务商'), findsOneWidget);
      expect(find.text('DeepSeek'), findsOneWidget);
      await tester.tap(find.byKey(StudioDriverKeys.providerRow('deepseek')));
      await tester.pumpAndSettle();
      expect(find.text('DeepSeek V4.1 Flash'), findsOneWidget);
      final visionCapabilityTags = find.byKey(
        StudioDriverKeys.modelCapabilityTags('deepseek', 'deepseek-flash'),
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
      expect(find.text('描述你的需求…'), findsOneWidget);
      expect(find.text('deepseek-flash'), findsOneWidget);
      expect(find.text('high'), findsOneWidget);
    },
  );
  testWidgets('Agents page contains only child system and user profiles', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(760, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final api = _FakeStudioApi(_stateWithPlannerModels());
    await _pumpSettingsPage(tester, api);

    await tester.tap(find.text('Agents'));
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('main-agent-card')), findsNothing);
    expect(
      find.byKey(const ValueKey('system-agent-enabled-planner')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey('agent-profile-card-planner')),
      findsNothing,
    );
    expect(
      find.byKey(StudioDriverKeys.settingsRoleModel('planner')),
      findsNothing,
    );
    expect(
      find.byKey(StudioDriverKeys.settingsRoleEffort('planner')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey('agent-profile-card-explorer')),
      findsOneWidget,
    );

    await tester.tap(find.byKey(const ValueKey('agent-profile-add')));
    await tester.pumpAndSettle();
    expect(
      find.byKey(const ValueKey('agent-profile-provider')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey('agent-profile-model-deepseek')),
      findsOneWidget,
    );
    expect(
      find.byKey(
        const ValueKey('agent-profile-effort-deepseek-deepseek-flash'),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey('agent-profile-workspace-mode')),
      findsOneWidget,
    );
    expect(find.text('Agent Profiles'), findsOneWidget);
    expect(find.text('Add user profile'), findsOneWidget);
    expect(_dialogText('Add user agent profile'), findsOneWidget);
    expect(_dialogText('Agent ID'), findsOneWidget);
    expect(_dialogText('Display name'), findsOneWidget);
    expect(_dialogText('Description'), findsOneWidget);
    expect(_dialogText('Best for'), findsOneWidget);
    expect(_dialogText('System instructions'), findsOneWidget);
    expect(_dialogText('Provider'), findsOneWidget);
    expect(_dialogText('Model'), findsOneWidget);
    expect(_dialogText('Reasoning effort'), findsOneWidget);
    expect(_dialogText('Workspace mode'), findsOneWidget);
    expect(_dialogText('Enabled'), findsOneWidget);
    expect(
      _dialogText(
        'Directory mode limits built-in file writes to the project only; '
        'it is not an OS sandbox, and shell, Git, and MCP can still '
        'bypass it.',
      ),
      findsOneWidget,
    );
    expect(_dialogText('Cancel'), findsOneWidget);
    expect(_dialogText('Save TOML atomically'), findsOneWidget);
    final effortField = find.byKey(
      const ValueKey('agent-profile-effort-deepseek-deepseek-flash'),
    );
    await tester.ensureVisible(effortField);
    await tester.pumpAndSettle();
    await tester.tap(effortField);
    await tester.pumpAndSettle();
    expect(find.text('Use model default'), findsOneWidget);
    await tester.tapAt(const Offset(10, 10));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();

    for (final role in const [
      'explorer',
      'executor',
      'worktree_executor',
      'reviewer',
    ]) {
      await tester.scrollUntilVisible(
        find.byKey(ValueKey('system-agent-workspace-$role')),
        300,
        scrollable: find.byType(Scrollable).first,
      );
      expect(
        find.byKey(ValueKey('system-agent-workspace-$role')),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.settingsRoleModel(role)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.settingsRoleEffort(role)),
        findsOneWidget,
      );
    }
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('system-agent-workspace-executor')),
      -300,
      scrollable: find.byType(Scrollable).first,
    );
    expect(
      find.descendant(
        of: find.byKey(const ValueKey('system-agent-workspace-executor')),
        matchRoot: true,
        matching: find.text('Directory'),
      ),
      findsOneWidget,
    );
    await tester.scrollUntilVisible(
      find.byKey(const ValueKey('system-agent-workspace-worktree_executor')),
      300,
      scrollable: find.byType(Scrollable).first,
    );
    expect(
      find.descendant(
        of: find.byKey(
          const ValueKey('system-agent-workspace-worktree_executor'),
        ),
        matchRoot: true,
        matching: find.text('Worktree'),
      ),
      findsOneWidget,
    );

    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'en locale projects system profile cards without leaking runtime Chinese',
    (tester) async {
      _configureSettingsTestView(tester);
      final api = _FakeStudioApi(_stateWithPlannerModels())
        ..userAgentProfiles = [_userAgentProfile];
      await _pumpSettingsPage(tester, api);

      await tester.tap(find.text('Agents'));
      await tester.pumpAndSettle();

      const enCardCopy = <(String, String, String)>[
        ('explorer', 'Explorer', 'Explore code and collect context.'),
        ('executor', 'Executor', 'Apply edits and run tools.'),
        (
          'worktree_executor',
          'Worktree executor',
          'Apply edits and run tools in an isolated Git worktree.',
        ),
        ('reviewer', 'Reviewer', 'Review results and verify risk.'),
      ];
      const runtimeChineseMetadata = [
        '探索者',
        '计划者',
        '执行者',
        'Worktree 执行者',
        '审查者',
        '只读探索代码',
        '分析目标并形成',
        '实施明确',
        '在独立 Git worktree',
        '检查实现',
      ];
      for (final (role, name, description) in enCardCopy) {
        final card = find.byKey(ValueKey('agent-profile-card-$role'));
        await _dragUntilBuilt(tester, card);
        expect(
          find.descendant(of: card, matching: find.text(name)),
          findsWidgets,
          reason: 'card title and route row share the localized role name',
        );
        expect(
          find.descendant(of: card, matching: find.textContaining(description)),
          findsWidgets,
        );
        for (final leak in runtimeChineseMetadata) {
          expect(find.textContaining(leak), findsNothing);
        }
      }

      final userCard = find.byKey(
        const ValueKey('agent-profile-card-user-helper'),
      );
      await _dragUntilBuilt(tester, userCard);
      expect(
        find.descendant(of: userCard, matching: find.text('User helper')),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: userCard,
          matching: find.textContaining('User owned helper profile'),
        ),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'zh Hans locale projects system profile cards and keeps user metadata verbatim',
    (tester) async {
      _configureSettingsTestView(tester);
      final api = _FakeStudioApi(_stateWithPlannerModels())
        ..userAgentProfiles = [_userAgentProfile];
      await _pumpSettingsPage(
        tester,
        api,
        locale: const Locale.fromSubtags(
          languageCode: 'zh',
          scriptCode: 'Hans',
        ),
      );

      await tester.tap(find.text('智能体'));
      await tester.pumpAndSettle();

      const zhCardCopy = <(String, String, String)>[
        ('explorer', '探索者', '探索代码并收集上下文'),
        ('executor', '执行者', '落实修改并运行工具'),
        ('worktree_executor', '工作树执行者', '在隔离的 Git 工作树中落实修改并运行工具'),
        ('reviewer', '审查者', '审查结果并验证风险'),
      ];
      const runtimeChineseDescriptions = [
        '只读探索代码',
        '分析目标并形成',
        '实施明确',
        '在独立 Git worktree',
        '检查实现',
      ];
      for (final (role, name, description) in zhCardCopy) {
        final card = find.byKey(ValueKey('agent-profile-card-$role'));
        await _dragUntilBuilt(tester, card);
        expect(
          find.descendant(of: card, matching: find.text(name)),
          findsWidgets,
          reason: 'card title and route row share the localized role name',
        );
        expect(
          find.descendant(of: card, matching: find.textContaining(description)),
          findsWidgets,
        );
        for (final leak in runtimeChineseDescriptions) {
          expect(find.textContaining(leak), findsNothing);
        }
      }

      final userCard = find.byKey(
        const ValueKey('agent-profile-card-user-helper'),
      );
      await _dragUntilBuilt(tester, userCard);
      expect(
        find.descendant(of: userCard, matching: find.text('User helper')),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: userCard,
          matching: find.textContaining('User owned helper profile'),
        ),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'en unknown system profile falls back safely while user metadata stays verbatim',
    (tester) async {
      _configureSettingsTestView(tester);
      const unknownSystemProfile = AgentProfileView(
        id: 'custom-router',
        displayName: '自定义路由',
        description: '运行时自定义说明',
        whenToUse: 'Use for custom routing',
        systemInstructions: 'Route with care.',
        providerId: 'deepseek',
        model: 'deepseek-flash',
        effort: null,
        source: 'studio-builtin',
        revision: 'system-v2',
        contentHash: 'system-custom-router',
        system: true,
        enabled: true,
        workspaceMode: AgentWorkspaceMode.unrestricted,
      );
      final api = _FakeStudioApi(_stateWithPlannerModels())
        ..extraSystemProfiles = [unknownSystemProfile]
        ..userAgentProfiles = [_userAgentProfile];
      await _pumpSettingsPage(tester, api);

      await tester.tap(find.text('Agents'));
      await tester.pumpAndSettle();

      final unknownCard = find.byKey(
        const ValueKey('agent-profile-card-custom-router'),
      );
      await _dragUntilBuilt(tester, unknownCard);
      expect(
        find.descendant(of: unknownCard, matching: find.text('custom-router')),
        findsWidgets,
      );
      expect(
        find.descendant(
          of: unknownCard,
          matching: find.textContaining('Studio role'),
        ),
        findsWidgets,
      );
      expect(find.textContaining('自定义路由'), findsNothing);
      expect(find.textContaining('运行时自定义说明'), findsNothing);

      final userCard = find.byKey(
        const ValueKey('agent-profile-card-user-helper'),
      );
      await _dragUntilBuilt(tester, userCard);
      expect(
        find.descendant(of: userCard, matching: find.text('User helper')),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: userCard,
          matching: find.textContaining('User owned helper profile'),
        ),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'Agents Recovery previews preserved worktree and cleans by revision',
    (tester) async {
      _configureSettingsTestView(tester);
      final state = _stateWithPlannerModels().copyWith(
        recoveryState: _worktreeRecoverySnapshot(),
      );
      final api = _FakeStudioApi(state);
      await _pumpSettingsPage(tester, api);

      await tester.tap(find.text('Agents'));
      await tester.pumpAndSettle();
      expect(find.text('Recovery'), findsOneWidget);
      expect(find.text('pure-agent-child-1'), findsOneWidget);
      expect(find.textContaining('base base-commit'), findsOneWidget);
      expect(find.textContaining('head head-commit'), findsOneWidget);
      expect(find.textContaining('src/agent.rs'), findsOneWidget);
      expect(find.text('Changed files: src/agent.rs'), findsOneWidget);
      final cleanup = find.byKey(const ValueKey('worktree-cleanup-child-1'));
      final childCleanup = find.byKey(
        const ValueKey('worktree-cleanup-child-child-1'),
      );
      expect(find.text('Clean up worktree and branch'), findsOneWidget);
      expect(find.text('Child agent worktree'), findsOneWidget);
      expect(cleanup, findsNothing);
      await tester.ensureVisible(childCleanup);
      await tester.tap(childCleanup);
      await tester.pump();

      expect(api.cleanedWorktree, (
        ownerKind: 'child',
        ownerThreadId: 'child-1',
        expectedLeaseRevision: 9,
      ));
    },
  );

  testWidgets(
    'en agents user profile dialog covers edit title and required validation',
    (tester) async {
      _configureSettingsTestView(tester);
      final api = _FakeStudioApi(_stateWithPlannerModels())
        ..userAgentProfiles = [_userAgentProfile];
      await _pumpSettingsPage(tester, api);

      await tester.tap(find.text('Agents'));
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(const ValueKey('agent-profile-add')));
      await tester.pumpAndSettle();
      expect(_dialogText('Add user agent profile'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('agent-profile-save')));
      await tester.pumpAndSettle();
      expect(_dialogText('Required'), findsNWidgets(5));
      await tester.tap(_dialogText('Cancel'));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('agent-profile-save')), findsNothing);

      final editButton = find.byKey(
        const ValueKey('agent-profile-edit-user-helper'),
      );
      expect(
        (await api.readAgentProfiles()).map((profile) => profile.id),
        contains('user-helper'),
      );
      await _dragUntilBuilt(tester, editButton);
      await tester.tap(editButton);
      await tester.pumpAndSettle();
      expect(_dialogText('Edit user agent profile'), findsOneWidget);
      await tester.tap(_dialogText('Cancel'));
      await tester.pumpAndSettle();
    },
  );

  testWidgets(
    'editing a user profile keeps an unresolvable canonical route on save',
    (tester) async {
      _configureSettingsTestView(tester);
      final api = _FakeStudioApi(_stateWithPlannerModels())
        ..userAgentProfiles = [_danglingUserAgentProfile];
      await _pumpSettingsPage(tester, api);

      await tester.tap(find.text('Agents'));
      await tester.pumpAndSettle();

      final editButton = find.byKey(
        const ValueKey('agent-profile-edit-user-ghost'),
      );
      await _dragUntilBuilt(tester, editButton);
      await tester.tap(editButton);
      await tester.pumpAndSettle();

      // canonical provider/model 不可解析：标记 unavailable，而不是改成
      // options.first 或模型默认 effort。
      expect(_dialogText('ghost-provider (unavailable)'), findsOneWidget);
      expect(_dialogText('ghost-model (unavailable)'), findsOneWidget);

      await tester.enterText(
        find.widgetWithText(TextFormField, 'Description'),
        'Updated description',
      );
      await tester.tap(find.byKey(const ValueKey('agent-profile-save')));
      await tester.pumpAndSettle();

      final saved = api.savedUserAgentProfileDraft!;
      expect(saved.description, 'Updated description');
      expect(saved.providerId, 'ghost-provider');
      expect(saved.model, 'ghost-model');
      expect(saved.effort, 'high');
    },
  );

  testWidgets('en recovery card renders full head line when head is null', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        recoveryState: _worktreeRecoverySnapshotWithoutHead(),
      ),
    );
    await _pumpSettingsPage(tester, api);

    await tester.tap(find.text('Agents'));
    await tester.pumpAndSettle();
    expect(find.text('Recovery'), findsOneWidget);
    expect(find.text('pure-agent-child-2'), findsOneWidget);
    expect(find.textContaining('base base-commit'), findsOneWidget);
    expect(find.textContaining('head unavailable'), findsOneWidget);
    expect(
      find.textContaining('/repo/.anywork/worktrees/thread-2/child-2'),
      findsOneWidget,
    );
    expect(find.textContaining('Changed files'), findsNothing);
  });

  testWidgets(
    'SSH directory dialog serializes browse so a repeated trigger is dropped',
    (tester) async {
      _configureSettingsTestView(tester);
      const server = SshServer(
        alias: 'ssh-arm',
        hostName: '192.168.100.12',
        port: 22,
        username: 'root',
        managed: true,
      );
      final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
      api.remoteDirListings['/workspace'] = const RemoteDirectoryListing(
        path: '/workspace',
        parent: '/',
        entries: [
          RemoteDirectoryEntry(name: 'project', path: '/workspace/project'),
        ],
      );
      await _pumpSettingsPage(tester, api);
      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
      await tester.pumpAndSettle();
      expect(api.browseRemoteCallCount, 1);

      final gate = Completer<void>();
      api.blockedBrowseRemote = gate;
      await tester.enterText(
        find.byKey(StudioDriverKeys.sshDirectoryPathInput),
        '/workspace/project',
      );
      await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
      await tester.pump();
      await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
      await tester.pump();

      // 同一 pending 期间第二次触发被入口 guard 拒绝，仅一次浏览。
      expect(api.browseRemoteCallCount, 2);

      gate.complete();
      await tester.pumpAndSettle();
      expect(api.browseRemoteCallCount, 2);
      expect(
        find.byKey(StudioDriverKeys.sshDirectoryCurrent('/workspace/project')),
        findsOneWidget,
      );
    },
  );

  testWidgets(
    'SSH directory dialog adopts the canonical path returned by the server',
    (tester) async {
      _configureSettingsTestView(tester);
      const server = SshServer(
        alias: 'ssh-arm',
        hostName: '192.168.100.12',
        port: 22,
        username: 'root',
        managed: true,
      );
      final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
      api.selectProjectStates['remote-project'] = _remoteProjectAdoptedState(
        path: '/workspace/project',
      );
      // 输入带尾斜杠，服务器返回 canonical 路径 `/workspace/project`。
      api.remoteDirListings['/workspace/project/'] =
          const RemoteDirectoryListing(
            path: '/workspace/project',
            parent: '/workspace',
            entries: [],
          );
      await _pumpSettingsPage(tester, api);
      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
      await tester.pumpAndSettle();

      await tester.enterText(
        find.byKey(StudioDriverKeys.sshDirectoryPathInput),
        '/workspace/project/',
      );
      await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
      await tester.pumpAndSettle();

      expect(
        find.byKey(StudioDriverKeys.sshDirectoryCurrent('/workspace/project')),
        findsOneWidget,
      );
      await tester.tap(find.text('Open this directory'));
      await tester.pumpAndSettle();
      expect(api.openedRemoteProject, (
        alias: server.alias,
        path: '/workspace/project',
      ));
    },
  );

  testWidgets(
    'SSH directory dialog keeps the window when the controller refuses silently',
    (tester) async {
      _configureSettingsTestView(tester);
      const server = SshServer(
        alias: 'ssh-arm',
        hostName: '192.168.100.12',
        port: 22,
        username: 'root',
        managed: true,
      );
      // 没有让 canonical 采用远端项目：controller 静默返回 false。
      final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
      await _pumpSettingsPage(tester, api);
      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
      await tester.pumpAndSettle();

      await tester.tap(find.text('Open this directory'));
      await tester.pumpAndSettle();

      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);
      expect(find.byKey(StudioDriverKeys.sshDirectoryError), findsOneWidget);
      expect(
        find.text(
          "Couldn't open this directory. Check the server and try again.",
        ),
        findsOneWidget,
      );
      expect(api.openRemoteProjectCallCount, 1);
    },
  );

  testWidgets(
    'SSH directory dialog closes only after the canonical state adopts the project',
    (tester) async {
      _configureSettingsTestView(tester);
      const server = SshServer(
        alias: 'ssh-arm',
        hostName: '192.168.100.12',
        port: 22,
        username: 'root',
        managed: true,
      );
      final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
      api.selectProjectStates['remote-project'] = _remoteProjectAdoptedState();
      await _pumpSettingsPage(tester, api);
      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
      await tester.pumpAndSettle();

      await tester.tap(find.text('Open this directory'));
      await tester.pumpAndSettle();

      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
      expect(api.openRemoteProjectCallCount, 1);
      expect(api.openedRemoteProject, (
        alias: server.alias,
        path: '/workspace',
      ));
    },
  );

  testWidgets('SSH directory dialog blocks cancel and back while opening', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    const server = SshServer(
      alias: 'ssh-arm',
      hostName: '192.168.100.12',
      port: 22,
      username: 'root',
      managed: true,
    );
    final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
    api.selectProjectStates['remote-project'] = _remoteProjectAdoptedState();
    final gate = Completer<void>();
    api.blockedOpenRemoteProject = gate;
    await _pumpSettingsPage(tester, api);
    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
    await tester.pumpAndSettle();

    // 正常态 Cancel 可关闭。
    final cancelIdle = tester.widget<TextButton>(
      find.widgetWithText(TextButton, 'Cancel'),
    );
    expect(cancelIdle.onPressed, isNotNull);

    await tester.tap(find.text('Open this directory'));
    // 同一 pending 期间重复触发被入口 guard 拒绝，仅执行一次打开。
    await tester.tap(find.text('Open this directory'));
    await tester.pump();

    // opening pending：PopScope 关闭被门控，Cancel 被禁用。
    // PopScope 是泛型组件，`find.byType(PopScope)` 按运行时类型等值匹配可能
    // 因类型参数推断不同而落空；用 `is PopScope` 谓词定位更稳定。
    final popScope = tester.widget<PopScope>(
      find.byWidgetPredicate((widget) => widget is PopScope),
    );
    expect(popScope.canPop, isFalse);
    expect(api.openRemoteProjectCallCount, 1);
    final cancelPending = tester.widget<TextButton>(
      find.widgetWithText(TextButton, 'Cancel'),
    );
    expect(cancelPending.onPressed, isNull);
    await tester.binding.handlePopRoute();
    await tester.pump();
    expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);

    gate.complete();
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
    expect(api.openRemoteProjectCallCount, 1);
  });

  testWidgets(
    'SSH directory dialog ignores a pre-open Cancel callback in the same frame as Open',
    (tester) async {
      _configureSettingsTestView(tester);
      final api = await _openSshDirectoryDialog(tester, adoptProject: true);
      final gate = Completer<void>();
      api.blockedOpenRemoteProject = gate;

      // 捕获 idle 态 Cancel 的 onPressed：重建前仍持有旧 closure。
      final idleCancel = tester
          .widget<TextButton>(find.widgetWithText(TextButton, 'Cancel'))
          .onPressed!;

      await tester.tap(find.text('Open this directory'));
      // 同一帧（尚未 rebuild）直接调用旧 Cancel closure：`_cancel` 在执行瞬间
      // 检查 `_opening`，绝不会关闭 pending 窗口。
      idleCancel();
      await tester.pump();

      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);
      expect(api.openRemoteProjectCallCount, 1);

      gate.complete();
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
      expect(api.openRemoteProjectCallCount, 1);
    },
  );

  testWidgets(
    'SSH directory dialog blocks system back in the same frame as Open',
    (tester) async {
      _configureSettingsTestView(tester);
      final api = await _openSshDirectoryDialog(tester, adoptProject: true);
      final gate = Completer<void>();
      api.blockedOpenRemoteProject = gate;

      await tester.tap(find.text('Open this directory'));
      // 不 pump 的同帧系统返回：canPop 恒为 false，pending 期间不会关闭。
      await tester.binding.handlePopRoute();
      await tester.pump();

      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);
      expect(api.openRemoteProjectCallCount, 1);

      gate.complete();
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
    },
  );

  testWidgets(
    'SSH directory dialog blocks ModalBarrier and Escape while opening',
    (tester) async {
      _configureSettingsTestView(tester);
      final api = await _openSshDirectoryDialog(tester, adoptProject: true);
      final gate = Completer<void>();
      api.blockedOpenRemoteProject = gate;

      await tester.tap(find.text('Open this directory'));
      // 遮罩（对话框外的 barrier）与 Escape 都走 maybePop → PopScope guard。
      await tester.tapAt(const Offset(10, 10));
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pump();

      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);
      expect(api.openRemoteProjectCallCount, 1);

      gate.complete();
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
    },
  );

  testWidgets(
    'SSH directory dialog still closes via back, barrier, Escape, and Cancel when idle',
    (tester) async {
      _configureSettingsTestView(tester);
      await _openSshDirectoryDialog(tester);

      // Cancel（idle）关闭。
      await tester.tap(find.widgetWithText(TextButton, 'Cancel'));
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);

      // barrier 关闭。
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen('ssh-arm')));
      await tester.pumpAndSettle();
      await tester.tapAt(const Offset(10, 10));
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);

      // Escape 关闭。
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen('ssh-arm')));
      await tester.pumpAndSettle();
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);

      // 系统返回关闭。
      await tester.tap(find.byKey(StudioDriverKeys.sshOpen('ssh-arm')));
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
    },
  );

  testWidgets(
    'SSH directory dialog stays usable at a narrow viewport with large text',
    (tester) async {
      tester.view.physicalSize = const Size(380, 640);
      tester.view.devicePixelRatio = 1;
      tester.platformDispatcher.textScaleFactorTestValue = 2.0;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      const server = SshServer(
        alias: 'ssh-arm',
        hostName: '192.168.100.12',
        port: 22,
        username: 'root',
        managed: true,
      );
      final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
      api.selectProjectStates['remote-project'] = _remoteProjectAdoptedState(
        path: '/workspace/project',
      );
      api.remoteDirListings['/workspace'] = const RemoteDirectoryListing(
        path: '/workspace',
        parent: '/',
        entries: [
          RemoteDirectoryEntry(name: 'project', path: '/workspace/project'),
        ],
      );
      api.remoteDirListings['/workspace/project'] =
          const RemoteDirectoryListing(
            path: '/workspace/project',
            parent: '/workspace',
            entries: [
              RemoteDirectoryEntry(name: 'src', path: '/workspace/project/src'),
            ],
          );
      await _pumpSshTab(tester, api);
      final open = find.byKey(StudioDriverKeys.sshOpen(server.alias));
      await _dragUntilBuilt(tester, open);
      await tester.tap(open);
      await tester.pumpAndSettle();

      // 无布局溢出（RenderFlex overflow 会通过 takeException 暴露）。
      expect(tester.takeException(), isNull);

      final pathInput = find.byKey(StudioDriverKeys.sshDirectoryPathInput);
      final go = find.byKey(StudioDriverKeys.sshDirectoryGo);
      final up = find.byKey(StudioDriverKeys.sshDirectoryUp);
      final openDir = find.byKey(StudioDriverKeys.sshOpenCurrentDirectory);
      final cancel = find.widgetWithText(TextButton, 'Cancel');

      // 内容可滚动：把 path/Go/Up 依次滚入视口，证明用户能到达并触发控件，
      // 而不是仅“构建存在”。actions 固定在 Dialog 底部，始终可命中。
      await tester.ensureVisible(pathInput);
      await tester.pumpAndSettle();
      expect(pathInput.hitTestable(), findsOneWidget);

      // 几何断言：actions 不与 path 输入重叠（actions 顶边在 path 底边下方）。
      final pathRect = tester.getRect(pathInput);
      final openRect = tester.getRect(openDir);
      final cancelRect = tester.getRect(cancel);
      expect(openRect.top >= pathRect.bottom, isTrue);
      expect(cancelRect.top >= pathRect.bottom, isTrue);

      await tester.ensureVisible(go);
      await tester.pumpAndSettle();
      expect(go.hitTestable(), findsOneWidget);
      await tester.ensureVisible(up);
      await tester.pumpAndSettle();
      expect(up.hitTestable(), findsOneWidget);
      expect(openDir.hitTestable(), findsOneWidget);

      // 实际输入 + Go：导航到子目录。
      await tester.ensureVisible(pathInput);
      await tester.enterText(pathInput, '/workspace/project');
      await tester.pumpAndSettle();
      await tester.ensureVisible(go);
      await tester.tap(go);
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.sshDirectoryCurrent('/workspace/project')),
        findsOneWidget,
      );
      expect(api.browsedRemoteDirectory, (
        alias: server.alias,
        path: '/workspace/project',
      ));

      // Up：回到父目录。
      await tester.ensureVisible(up);
      await tester.tap(up);
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.sshDirectoryCurrent('/workspace')),
        findsOneWidget,
      );

      // 再次进入子目录后 Open 成功并关闭对话框。
      await tester.ensureVisible(pathInput);
      await tester.enterText(pathInput, '/workspace/project');
      await tester.pumpAndSettle();
      await tester.ensureVisible(go);
      await tester.tap(go);
      await tester.pumpAndSettle();
      await tester.ensureVisible(openDir);
      expect(
        tester.widget<FilledButton>(openDir).onPressed,
        isNotNull,
        reason: 'A canonical remote directory must enable the Open action.',
      );
      expect(openDir.hitTestable(), findsOneWidget);
      expect(api.openRemoteProjectCallCount, 0);
      await tester.tap(openDir);
      await tester.pumpAndSettle();
      expect(api.openRemoteProjectCallCount, 1);
      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
      expect(api.openedRemoteProject, (
        alias: server.alias,
        path: '/workspace/project',
      ));
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'persistence diagnostics keep unobserved per-Thread watermarks unknown',
    (tester) async {
      _configureResponsiveView(tester, const Size(1280, 800));
      final api = _PersistenceQueueApi(
        _stateWithPlannerModels(),
        queue: const PersistenceQueueSnapshot(
          pendingOperations: 2,
          pendingBytes: 1024,
          inFlightBytes: 0,
          pressurePaused: true,
          threads: [
            // 只有 checkpoint 回退诊断、没有 writer 的 Thread：水位是未知而不是零。
            ThreadPersistenceSnapshot(
              threadId: 'recovered-thread',
              lastError: 'checkpoint rollback pending',
            ),
            ThreadPersistenceSnapshot(
              threadId: 'live-thread',
              stateDirtyRevision: 3,
              stateDurableRevision: 2,
              historyAdmittedSequence: 9,
              historyDurableSequence: 9,
              callsAdmittedSequence: 4,
              callsDurableSequence: 4,
            ),
          ],
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
        find.byKey(StudioDriverKeys.persistenceQueueDiagnostics),
        findsOneWidget,
      );
      // 未观测的水位显示为未知，不折叠成 0/0。
      expect(
        find.textContaining(
          'recovered-thread · state ?/? · history ?/? · calls ?/?',
        ),
        findsOneWidget,
      );
      // 已观测到的水位按真实值显示（含已观测的零）。
      expect(
        find.textContaining(
          'live-thread · state 2/3 · history 9/9 · calls 4/4',
        ),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    },
  );
}

/// 仅本测试使用：在通用测试替身上补出持久化队列读能力，以验证未知水位的显示。
class _PersistenceQueueApi extends _FakeStudioApi
    implements PersistenceQueueReader {
  _PersistenceQueueApi(super.initialState, {required this.queue});

  final PersistenceQueueSnapshot queue;

  @override
  Future<PersistenceQueueSnapshot> readPersistenceQueue() async => queue;
}

const _sidebarTooltipProjectName =
    'Very Long Canonical Project Name Used For Sidebar Tooltip Coverage';
const _sidebarTooltipProjectPath =
    '/home/dev/opensource/pure-lang-pure/deeper/workspace';
const _sidebarTooltipThreadTitle =
    'A Very Long Session Title Used For Sidebar Tooltip Coverage';
const _sidebarTooltipIssueDetail =
    'Project recovery diagnostic detail for sidebar tooltip coverage';

StudioState _sidebarTooltipNameState() {
  return _emptyState().copyWith(
    projectDirectory: ProjectDirectoryState(
      values: const [
        StudioProject(
          id: 'project-1',
          name: _sidebarTooltipProjectName,
          path: _sidebarTooltipProjectPath,
        ),
      ],
    ),
    threadDirectory: ThreadDirectoryWindow(
      threads: [
        StudioThread(
          id: 'session-1',
          projectId: 'project-1',
          title: _sidebarTooltipThreadTitle,
          mode: ThreadModeId.simple,
          updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
          workspacePath: '.',
        ),
      ],
    ),
  );
}

/// 读取 Tooltip 生效的 hover wait：`RawTooltip.hoverDelay` 已按
/// widget.waitDuration → TooltipTheme → Duration.zero 完成解析。
Duration _tooltipHoverWait(WidgetTester tester, Finder tooltip) {
  return tester.widget<RawTooltip>(tooltip).hoverDelay;
}

/// 悬停事件后按 wait duration 推进时钟，让 overlay 完成显示调度。
Future<void> _pumpTooltipHover(WidgetTester tester, Duration wait) async {
  await tester.pump();
  if (wait > Duration.zero) {
    await tester.pump(wait);
  }
  await tester.pumpAndSettle();
}

void _configureSettingsTestView(WidgetTester tester) {
  tester.view.physicalSize = const Size(1280, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
}

/// Agents 列表按需构建：沿设置内容 pane 固定向下拖动，直到目标进入
/// 元素树并落在当前视口内。
Finder _settingsPaneScrollable() => find
    .descendant(
      of: find.byKey(const ValueKey('settings-pane-scroll')),
      matching: find.byType(Scrollable),
    )
    .first;

Future<void> _dragUntilBuilt(WidgetTester tester, Finder finder) async {
  final paneScrollable = _settingsPaneScrollable();
  final viewportHeight =
      tester.view.physicalSize.height / tester.view.devicePixelRatio;
  bool targetIsVisible() {
    if (finder.evaluate().isEmpty) return false;
    final rect = tester.getRect(finder);
    return rect.top >= 0 && rect.bottom <= viewportHeight;
  }

  for (var drag = 0; drag < 20 && !targetIsVisible(); drag++) {
    await tester.drag(paneScrollable, const Offset(0, -300));
    await tester.pump();
  }
  expect(finder, findsOneWidget, reason: 'target should be built after drags');
}

Future<void> _pumpSettingsPage(
  WidgetTester tester,
  _FakeStudioApi api, {
  Locale locale = const Locale('en'),
}) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: [studioApiProvider.overrideWithValue(api)],
      child: _localizedApp(home: const SettingsPage(), locale: locale),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> _pumpSshTab(WidgetTester tester, _FakeStudioApi api) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: [studioApiProvider.overrideWithValue(api)],
      child: _localizedApp(
        home: Consumer(
          builder: (context, ref, child) {
            ref.watch(studioControllerProvider);
            return const Scaffold(body: SshTab());
          },
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

/// 打开 SSH 设置标签并进入远端目录选择对话框（默认浏览 `/workspace`）。
/// 返回使用的 fake；[adoptProject] 为 true 时让 canonical 采用 `remote-project`。
Future<_FakeStudioApi> _openSshDirectoryDialog(
  WidgetTester tester, {
  bool adoptProject = false,
}) async {
  const server = SshServer(
    alias: 'ssh-arm',
    hostName: '192.168.100.12',
    port: 22,
    username: 'root',
    managed: true,
  );
  final api = _FakeStudioApi(_emptyState())..sshServers = const [server];
  if (adoptProject) {
    api.selectProjectStates['remote-project'] = _remoteProjectAdoptedState();
  }
  await _pumpSettingsPage(tester, api);
  await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(StudioDriverKeys.sshOpen(server.alias)));
  await tester.pumpAndSettle();
  return api;
}

/// Agents tab 内固定文案统一收敛到 AlertDialog 作用域断言，
/// 避免与底层 system profile 卡片的同文案标签混淆。
Finder _dialogText(String text) =>
    find.descendant(of: find.byType(Dialog), matching: find.text(text));

RecoveryStateSnapshot _worktreeRecoverySnapshot() {
  return RecoveryStateSnapshot(
    revision: 7,
    values: const [
      StudioRecoveryIssue(
        id: 'worktree-lease-child-1',
        scope: RecoveryIssueScope.thread,
        category: RecoveryIssueCategory.repository,
        availableActions: [RecoveryIssueAction.cleanupWorktree],
        detail: 'Preserved for review',
        projectId: 'project-1',
        threadId: 'thread-1',
        worktree: WorktreeRecoveryPreview(
          ownerKind: WorktreeOwnerKind.child,
          ownerThreadId: 'child-1',
          leaseRevision: 9,
          state: 'preserved',
          repositoryRoot: '/repo',
          path: '/repo/.anywork/worktrees/thread-1/child-1',
          branch: 'pure-agent-child-1',
          baseCommit: 'base-commit',
          headCommit: 'head-commit',
          dirty: true,
          changedFiles: ['src/agent.rs'],
        ),
      ),
    ],
  );
}

/// head 不可用且无变更文件的 worktree 预览，覆盖 null head 行为。
RecoveryStateSnapshot _worktreeRecoverySnapshotWithoutHead() {
  return RecoveryStateSnapshot(
    revision: 8,
    values: const [
      StudioRecoveryIssue(
        id: 'worktree-lease-child-2',
        scope: RecoveryIssueScope.thread,
        category: RecoveryIssueCategory.repository,
        availableActions: [RecoveryIssueAction.cleanupWorktree],
        detail: 'Preserved for review',
        projectId: 'project-1',
        threadId: 'thread-1',
        worktree: WorktreeRecoveryPreview(
          ownerKind: WorktreeOwnerKind.child,
          ownerThreadId: 'child-2',
          leaseRevision: 11,
          state: 'preserved',
          repositoryRoot: '/repo',
          path: '/repo/.anywork/worktrees/thread-2/child-2',
          branch: 'pure-agent-child-2',
          baseCommit: 'base-commit',
          headCommit: null,
          dirty: false,
          changedFiles: [],
        ),
      ),
    ],
  );
}

/// 用户 Profile fixture：驱动 `agent-profile-edit-<id>` 编辑对话框断言。
const _userAgentProfile = AgentProfileView(
  id: 'user-helper',
  displayName: 'User helper',
  description: 'User owned helper profile',
  whenToUse: 'Use when a helper is needed',
  systemInstructions: 'Follow the user instructions.',
  providerId: 'deepseek',
  model: 'deepseek-flash',
  effort: 'high',
  source: 'user-toml',
  revision: 'user-v1',
  contentHash: 'user-helper-hash',
  system: false,
  enabled: true,
  workspaceMode: AgentWorkspaceMode.directory,
);

/// 用户 Profile fixture：canonical provider/model 当前不可解析，用于验证
/// 编辑保存不会静默改写路由。
const _danglingUserAgentProfile = AgentProfileView(
  id: 'user-ghost',
  displayName: 'Ghost helper',
  description: 'Profile with an unresolvable route',
  whenToUse: 'Use when a helper is needed',
  systemInstructions: 'Follow the user instructions.',
  providerId: 'ghost-provider',
  model: 'ghost-model',
  effort: 'high',
  source: 'user-toml',
  revision: 'user-v2',
  contentHash: 'user-ghost-hash',
  system: false,
  enabled: true,
  workspaceMode: AgentWorkspaceMode.directory,
);

ModelPerformanceSnapshotView _modelPerformanceFixture({
  bool hasUnpricedUsage = false,
  List<RuntimeCostView>? estimatedCosts,
  int revision = 3,
}) {
  return ModelPerformanceSnapshotView(
    revision: revision,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(3000),
    sessionCosts: [
      SessionCostView(
        rootThreadId: 'session-1',
        estimatedCosts:
            estimatedCosts ??
            [
              RuntimeCostView(currency: 'CNY', amount: 0.14),
              RuntimeCostView(currency: 'USD', amount: 0.02),
            ],
        hasUnpricedUsage: hasUnpricedUsage,
      ),
    ],
    summaries: const [
      ModelPerformanceSummaryView(
        providerInstanceId: 'provider-a',
        providerDisplayName: 'DeepSeek',
        model: 'model-a',
        reasoningEffort: 'high',
        sampleCount: 1,
        completionTokens: 150,
        totalTtftMillis: 200,
        totalDecodeMillis: 1250,
        totalResponseMillis: 1450,
        tokensPerSecond: 120,
        averageTtftMillis: 100,
        averageResponseMillis: 725,
      ),
      ModelPerformanceSummaryView(
        providerInstanceId: 'provider-a',
        providerDisplayName: 'DeepSeek',
        model: 'model-a',
        reasoningEffort: 'none',
        sampleCount: 1,
        completionTokens: 30,
        totalTtftMillis: 80,
        totalDecodeMillis: 300,
        totalResponseMillis: 380,
        tokensPerSecond: 100,
        averageTtftMillis: 80,
        averageResponseMillis: 380,
      ),
      ModelPerformanceSummaryView(
        providerInstanceId: 'provider-a',
        providerDisplayName: 'DeepSeek',
        model: 'model-a',
        sampleCount: 1,
        completionTokens: 20,
        totalTtftMillis: 70,
        totalDecodeMillis: 250,
        totalResponseMillis: 320,
        tokensPerSecond: 80,
        averageTtftMillis: 70,
        averageResponseMillis: 320,
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
        configuredModel: 'catalog-model-a',
        sentModel: 'model-a',
        reportedModel: 'provider-other',
        modelMatchState: ModelMatchState.mismatched,
        reasoningEffort: 'high',
        completionTokens: 50,
        ttftMillis: 100,
        decodeMillis: 250,
        totalResponseMillis: 350,
        tokensPerSecond: 200,
      ),
      ModelPerformanceSampleView(
        completedAt: DateTime.fromMillisecondsSinceEpoch(3000),
        providerInstanceId: 'provider-a',
        providerDisplayName: 'DeepSeek',
        model: 'model-a',
        configuredModel: 'model-a',
        sentModel: 'model-a',
        reportedModel: 'model-a',
        modelMatchState: ModelMatchState.matched,
        reasoningEffort: 'high',
        completionTokens: 55,
        ttftMillis: 110,
        decodeMillis: 275,
        totalResponseMillis: 385,
        tokensPerSecond: 180,
      ),
      ModelPerformanceSampleView(
        completedAt: DateTime.fromMillisecondsSinceEpoch(2500),
        providerInstanceId: 'provider-a',
        providerDisplayName: 'DeepSeek',
        model: 'model-a',
        configuredModel: 'catalog-model-a',
        sentModel: 'model-a',
        modelMatchState: ModelMatchState.unreported,
        reasoningEffort: 'none',
        completionTokens: 30,
        ttftMillis: 80,
        decodeMillis: 300,
        totalResponseMillis: 380,
        tokensPerSecond: 100,
      ),
      ModelPerformanceSampleView(
        completedAt: DateTime.fromMillisecondsSinceEpoch(2200),
        providerInstanceId: 'provider-a',
        providerDisplayName: 'DeepSeek',
        model: 'model-a',
        completionTokens: 20,
        ttftMillis: 70,
        decodeMillis: 250,
        totalResponseMillis: 320,
        tokensPerSecond: 80,
      ),
      ModelPerformanceSampleView(
        completedAt: DateTime.fromMillisecondsSinceEpoch(2000),
        providerInstanceId: 'provider-b',
        providerDisplayName: 'OpenAI',
        model: 'model-b',
        configuredModel: 'model-b',
        sentModel: 'model-b',
        reportedModel: 'model-b',
        modelMatchState: ModelMatchState.matched,
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
