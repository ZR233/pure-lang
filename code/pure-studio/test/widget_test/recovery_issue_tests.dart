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

      final subscriptionCount = api.sessionSubscriptions.length;
      await tester.tap(find.text('Broken Session'));
      await tester.pump();
      expect(api.sessionSubscriptions.length, subscriptionCount);

      await tester.tap(find.text('Project Other'));
      await tester.pump();
      expect(api.selectedProjectRequest, 'project-other');
      expect(tester.takeException(), isNull);
    },
  );

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
      scope: RecoveryIssueScope.session,
      projectId: 'project-current',
      sessionId: 'session-broken',
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
    api.recoveryCleanupState = state.copyWith(recoveryIssues: const []);
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
        turnPhase: TurnPhase.streaming,
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
      await tester.pumpAndSettle();

      expect(api.cleanedProjectId, 'project-a');
      expect(api.projectCleanupExpectedRevision, 'project-revision-1');
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
      ..recoveryCleanupState = state.copyWith(recoveryIssues: const [])
      ..recoveryPreviews['issue-session'] = const RecoveryCleanupPreview(
        issueId: 'issue-session',
        expectedRevision: 'revision-refreshed',
        scope: RecoveryIssueScope.session,
        projectId: 'project-current',
        sessionId: 'session-broken',
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
  final healthySession = StudioSession(
    id: 'session-healthy',
    projectId: 'project-current',
    title: 'Healthy Session',
    mode: StudioMode.simple,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );
  final brokenSession = StudioSession(
    id: 'session-broken',
    projectId: 'project-current',
    title: 'Broken Session',
    mode: StudioMode.task,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );
  return _emptyState().copyWith(
    projects: const [
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
    sessions: [healthySession, brokenSession],
    selectedProjectId: 'project-current',
    selectedSessionId: healthySession.id,
    selectedRootSessionId: healthySession.id,
    messagesBySession: {
      healthySession.id: const [],
      brokenSession.id: const [],
    },
    recoveryIssues: [
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
        scope: RecoveryIssueScope.session,
        category: RecoveryIssueCategory.worktree,
        availableActions: [RecoveryIssueAction.cleanupSession],
        projectId: 'project-current',
        sessionId: 'session-broken',
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
  );
}
