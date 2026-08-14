part of '../widget_test.dart';

void registerRecoveryIssueTests() {
  test('FRB initialization recovery clears a rejected cached future', () async {
    var attempts = 0;
    FrbStudioApi.debugOverrideInitialization(() async {
      attempts += 1;
      if (attempts == 1) {
        throw StateError('bridge unavailable');
      }
    });
    addTearDown(() => FrbStudioApi.debugOverrideInitialization(null));

    await expectLater(FrbStudioApi.ensureReady(), throwsA(isA<StateError>()));
    await FrbStudioApi.ensureReady();

    expect(attempts, 2);
  });

  testWidgets(
    'recovery issues keep shell ready and block only affected targets',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final state = _recoveryIssueState(includeApplicationIssue: true);
      final api = _FakeStudioApi(state);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('1 recovery issue(s) need attention'), findsOneWidget);
      expect(find.byIcon(Icons.error_outline), findsNWidgets(2));

      await tester.tap(find.text('Broken Project'));
      await tester.pump();
      expect(api.selectedProjectRequest, isNull);

      final subscriptionCount = api.threadSubscriptions.length;
      await tester.tap(find.text('Broken Session'));
      await tester.pump();
      expect(api.threadSubscriptions.length, subscriptionCount);

      await tester.tap(find.text('Project Other'));
      await tester.pump();
      expect(api.selectedProjectRequest, 'project-other');
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('zero-project bootstrap renders a healthy empty shell', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_noProjectState());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('studio-sidebar')), findsOneWidget);
    expect(find.byTooltip('Open project'), findsOneWidget);
    expect(find.text('Pure Studio could not start'), findsNothing);
    expect(find.byIcon(Icons.error_outline), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('cleaning the last broken project returns to empty state', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    const brokenProject = StudioProject(
      id: 'project-broken',
      name: 'Broken Project',
      path: r'C:\missing',
    );
    final state = _noProjectState().copyWith(
      projectDirectory: const ProjectDirectoryState(values: [brokenProject]),
      recoveryState: const RecoveryStateSnapshot(
        values: [
          StudioRecoveryIssue(
            id: 'issue-project',
            scope: RecoveryIssueScope.project,
            category: RecoveryIssueCategory.repository,
            availableActions: [RecoveryIssueAction.removeProject],
            projectId: 'project-broken',
            detail: 'Project workspace is unavailable.',
          ),
        ],
      ),
    );
    final api = _FakeStudioApi(state)
      ..recoveryPreviews['issue-project'] = const RecoveryCleanupPreview(
        issueId: 'issue-project',
        expectedRevision: 'revision-project',
        scope: RecoveryIssueScope.project,
        projectId: 'project-broken',
        detail: 'Remove the unavailable project from Studio.',
        resources: [],
      )
      ..recoveryCleanupState = _noProjectState();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(
      find.byKey(const ValueKey('project-cleanup-project-broken')),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('recovery-cleanup-confirm')));
    await tester.pumpAndSettle();

    expect(api.cleanedRecoveryIssueId, 'issue-project');
    expect(api.cleanupExpectedRevision, 'revision-project');
    expect(find.text('Broken Project'), findsNothing);
    expect(find.byTooltip('Open project'), findsOneWidget);
    expect(find.byIcon(Icons.error_outline), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('recovery cleanup preview can cancel then confirm', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _recoveryIssueState(sessionIssueOnly: true);
    final api = _FakeStudioApi(state);
    api.recoveryPreviews['issue-session'] = const RecoveryCleanupPreview(
      issueId: 'issue-session',
      expectedRevision: 'revision-1',
      scope: RecoveryIssueScope.thread,
      projectId: 'project-current',
      threadId: 'session-broken',
      detail: 'Worktree ownership is incomplete.',
      resources: [
        RecoveryCleanupResource(
          workUnitId: 'work-unit-1',
          path: r'C:\repo\.pure\worktrees\task-1\agent-1',
          branch: 'pure-task-task-1-agent-1',
          presence: RecoveryResourcePresence.complete,
          registrationExists: true,
          pathExists: true,
          branchExists: true,
          dirty: false,
          aheadBy: 1,
          changedFileCount: 7,
        ),
      ],
    );
    api.recoveryCleanupState = state.copyWith(
      recoveryState: const RecoveryStateSnapshot(),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Review safe cleanup'));
    await tester.pumpAndSettle();
    final previewTexts = tester
        .widgetList<Text>(find.byType(Text))
        .map((text) => text.data)
        .whereType<String>()
        .toList();
    expect(
      find.textContaining('unmerged commit'),
      findsOneWidget,
      reason: previewTexts.join(' | '),
    );
    expect(find.textContaining('changed file'), findsOneWidget);

    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(api.cleanedRecoveryIssueId, isNull);
    expect(find.text('Clean up recovery issue?'), findsNothing);

    await tester.tap(find.byTooltip('Review safe cleanup'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('recovery-cleanup-confirm')));
    await tester.pumpAndSettle();

    expect(api.cleanedRecoveryIssueId, 'issue-session');
    expect(api.cleanupExpectedRevision, 'revision-1');
    expect(find.byIcon(Icons.error_outline), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'project close previews all Pure worktrees and remains available while busy',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final state = _twoProjectState(
        selectedProjectId: 'project-a',
        turnState: const StudioTurnState.inProgress(
          StudioTurnActivity.responding,
        ),
      );
      final api = _FakeStudioApi(state);
      api.projectCleanupPreviews['project-a'] = const RecoveryCleanupPreview(
        issueId: 'project-cleanup-project-a',
        expectedRevision: 'project-revision-1',
        scope: RecoveryIssueScope.project,
        projectId: 'project-a',
        detail: 'This fixed diagnostic is intentionally not displayed.',
        resources: [
          RecoveryCleanupResource(
            workUnitId: 'work-unit-a',
            path: r'C:\repo\.pure\worktrees\task-a\agent-a',
            branch: 'pure-task-task-a-agent-a',
            presence: RecoveryResourcePresence.complete,
            registrationExists: true,
            pathExists: true,
            branchExists: true,
            dirty: true,
            aheadBy: 2,
            changedFileCount: 3,
          ),
        ],
      );
      api.projectCleanupState = _twoProjectState(
        selectedProjectId: 'project-b',
        projects: const [
          StudioProject(id: 'project-b', name: 'Project B', path: 'b'),
        ],
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();

      final cleanupButton = find.byKey(
        const ValueKey('project-cleanup-project-a'),
      );
      expect(tester.widget<IconButton>(cleanupButton).onPressed, isNotNull);
      await tester.tap(cleanupButton);
      await tester.pumpAndSettle();

      expect(
        find.text('Remove project and clean up Pure worktrees?'),
        findsOneWidget,
      );
      expect(find.textContaining('main workspace'), findsOneWidget);
      expect(find.textContaining('Uncommitted changes'), findsOneWidget);
      expect(
        find.text('This fixed diagnostic is intentionally not displayed.'),
        findsNothing,
      );

      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();
      expect(api.cleanedProjectId, isNull);

      await tester.tap(cleanupButton);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('project-cleanup-confirm')));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));

      expect(api.cleanedProjectId, 'project-a');
      expect(api.projectCleanupExpectedRevision, 'project-revision-1');
      expect(find.text('Project A'), findsNothing);
      expect(find.text('Project B'), findsWidgets);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'project cleanup confirmation still runs while product state reloads',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final state = _twoProjectState(selectedProjectId: 'project-a');
      final api = _FakeStudioApi(state)
        ..projectCleanupState = _twoProjectState(
          selectedProjectId: 'project-b',
          projects: const [
            StudioProject(id: 'project-b', name: 'Project B', path: 'b'),
          ],
        );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(const ValueKey('project-cleanup-project-a')));
      await tester.pumpAndSettle();

      final blockedReload = Completer<void>();
      api.blockedStudioStateLoad = blockedReload;
      container.invalidate(studioControllerProvider);
      await tester.pump();
      expect(container.read(studioControllerProvider).isLoading, isTrue);

      await tester.tap(find.byKey(const ValueKey('project-cleanup-confirm')));
      await tester.pump();

      expect(api.cleanedProjectId, 'project-a');
      expect(api.projectCleanupExpectedRevision, 'revision-project-a');

      blockedReload.complete();
      api.blockedStudioStateLoad = null;
      await tester.pumpAndSettle();

      expect(
        find.text('Remove project and clean up Pure worktrees?'),
        findsNothing,
      );
      expect(find.text('Project A'), findsNothing);
      expect(find.text('Project B'), findsWidgets);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('failed recovery cleanup can refresh stale preview and retry', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _recoveryIssueState(sessionIssueOnly: true);
    final api = _FakeStudioApi(state)
      ..recoveryCleanupError = StateError('revision changed');
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Review safe cleanup'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('recovery-cleanup-confirm')));
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey('recovery-cleanup-error')),
      findsOneWidget,
    );
    expect(find.textContaining('revision changed'), findsOneWidget);
    expect(find.byIcon(Icons.error_outline), findsOneWidget);
    expect(
      find.byKey(const ValueKey('recovery-cleanup-refresh')),
      findsOneWidget,
    );

    api
      ..recoveryCleanupError = null
      ..recoveryCleanupState = state.copyWith(
        recoveryState: const RecoveryStateSnapshot(),
      )
      ..recoveryPreviews['issue-session'] = const RecoveryCleanupPreview(
        issueId: 'issue-session',
        expectedRevision: 'revision-refreshed',
        scope: RecoveryIssueScope.thread,
        projectId: 'project-current',
        threadId: 'session-broken',
        detail: 'Refreshed recovery cleanup preview',
        resources: [],
      );
    await tester.tap(find.byKey(const ValueKey('recovery-cleanup-refresh')));
    await tester.pumpAndSettle();

    expect(api.previewRecoveryIssueCleanupCount, 2);
    expect(find.text('Refreshed recovery cleanup preview'), findsOneWidget);
    expect(find.byKey(const ValueKey('recovery-cleanup-error')), findsNothing);

    await tester.tap(find.byKey(const ValueKey('recovery-cleanup-confirm')));
    await tester.pumpAndSettle();
    expect(api.cleanupExpectedRevision, 'revision-refreshed');
    expect(find.byIcon(Icons.error_outline), findsNothing);
  });

  testWidgets(
    'merge recovery retry preserves failure then reopens the affected session',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final initial = _recoveryIssueState(sessionIssueOnly: true).copyWith(
        recoveryState: const RecoveryStateSnapshot(
          values: [
            StudioRecoveryIssue(
              id: 'issue-merge',
              scope: RecoveryIssueScope.thread,
              category: RecoveryIssueCategory.merge,
              availableActions: [RecoveryIssueAction.retry],
              projectId: 'project-current',
              threadId: 'session-broken',
              taskRunId: 'task-session',
              detail: 'Planner Git integration needs reconciliation.',
            ),
          ],
        ),
      );
      final api = _FakeStudioApi(initial)
        ..recoveryRetryError = StateError('branch identity changed');
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();

      final retry = find.byKey(
        StudioDriverKeys.retryRecoveryIssue('issue-merge'),
      );
      expect(find.byTooltip('Continue merge recovery'), findsOneWidget);
      await tester.tap(retry);
      await tester.pumpAndSettle();

      expect(find.textContaining('branch identity changed'), findsOneWidget);
      expect(find.byIcon(Icons.error_outline), findsOneWidget);
      expect(api.retriedRecoveryIssueId, isNull);

      api
        ..recoveryRetryError = null
        ..recoveryRetryState = initial.copyWith(
          recoveryState: const RecoveryStateSnapshot(),
          selectedThreadId: 'session-broken',
        );
      await tester.tap(retry);
      await tester.pumpAndSettle();

      expect(api.retriedRecoveryIssueId, 'issue-merge');
      expect(api.threadSubscriptions.last, 'session-broken');
      expect(find.byIcon(Icons.error_outline), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('compact rail keeps recovery warning and cleanup entry', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(560, 720);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_recoveryIssueState(sessionIssueOnly: true));
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
    expect(find.byIcon(Icons.error_outline), findsOneWidget);
    expect(find.byTooltip('Review safe cleanup'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('fatal bootstrap error can retry successfully', (tester) async {
    final api = _FakeStudioApi(_emptyState())
      ..bootstrapError = StateError('database unavailable');
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Pure Studio could not start'), findsOneWidget);
    expect(find.byKey(const ValueKey('runtime-fatal-retry')), findsOneWidget);

    api.bootstrapError = null;
    await tester.tap(find.byKey(const ValueKey('runtime-fatal-retry')));
    await tester.pumpAndSettle();

    expect(api.bootstrapCount, 2);
    expect(find.byKey(const ValueKey('studio-sidebar')), findsOneWidget);
    expect(find.text('Pure Studio could not start'), findsNothing);
  });
}

StudioState _recoveryIssueState({
  bool includeApplicationIssue = false,
  bool sessionIssueOnly = false,
}) {
  final healthySession = StudioThread(
    id: 'session-healthy',
    projectId: 'project-current',
    title: 'Healthy Session',
    mode: StudioMode.simple,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );
  final brokenSession = StudioThread(
    id: 'session-broken',
    projectId: 'project-current',
    title: 'Broken Session',
    mode: StudioMode.task,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );
  return _emptyState().copyWith(
    projectDirectory: const ProjectDirectoryState(
      values: [
        StudioProject(
          id: 'project-current',
          name: 'Project Current',
          path: r'C:\current',
        ),
        StudioProject(
          id: 'project-broken',
          name: 'Broken Project',
          path: r'C:\broken',
        ),
        StudioProject(
          id: 'project-other',
          name: 'Project Other',
          path: r'C:\other',
        ),
      ],
    ),
    threadDirectory: ThreadDirectoryState(
      values: [healthySession, brokenSession],
    ),
    selectedProjectId: 'project-current',
    selectedThreadId: healthySession.id,
    workspacesByThread: {
      for (final thread in [healthySession, brokenSession])
        thread.id: ThreadWorkspace(
          thread: thread,
          revision: 0,
          items: const [],
          interactions: const [],
          runtime: _testRuntime(),
        ),
    },
    workspaceUiByThread: {
      for (final thread in [healthySession, brokenSession])
        thread.id: const WorkspaceUiState(
          syncState: AgentWorkspaceSyncState.ready,
        ),
    },
    recoveryState: RecoveryStateSnapshot(
      values: [
        if (!sessionIssueOnly)
          const StudioRecoveryIssue(
            id: 'issue-project',
            scope: RecoveryIssueScope.project,
            category: RecoveryIssueCategory.repository,
            availableActions: [RecoveryIssueAction.removeProject],
            projectId: 'project-broken',
            taskRunId: 'task-project',
            detail: 'Project Git identity cannot be read.',
          ),
        const StudioRecoveryIssue(
          id: 'issue-session',
          scope: RecoveryIssueScope.thread,
          category: RecoveryIssueCategory.worktree,
          availableActions: [RecoveryIssueAction.cleanupThread],
          projectId: 'project-current',
          threadId: 'session-broken',
          taskRunId: 'task-session',
          detail: 'Worktree ownership is incomplete.',
        ),
        if (includeApplicationIssue)
          const StudioRecoveryIssue(
            id: 'issue-application',
            scope: RecoveryIssueScope.application,
            category: RecoveryIssueCategory.worktree,
            availableActions: [RecoveryIssueAction.retry],
            detail: 'An orphan resource could not be classified.',
          ),
      ],
    ),
  );
}
