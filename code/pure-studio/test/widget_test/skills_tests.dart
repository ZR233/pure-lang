part of '../widget_test.dart';

SkillSummaryView _settingsSkillSummary(String name, String description) {
  return SkillSummaryView(
    name: name,
    description: description,
    source: 'project',
    providerId: 'fake',
    modelInvocable: true,
    userInvocable: true,
    resourceBase: const SkillResourceBaseView(
      SkillResourceBaseKind.directory,
      'skills',
    ),
  );
}

void registerSkillsTests() {
  testWidgets('skills tab entry reads catalog without discovering', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    api.discoveredSkills = const ['flutter-ui-polish', 'runtime-review'];
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    expect(api.readSkillsCallCount, 0);
    await tester.tap(find.text('Skills'));
    await tester.pumpAndSettle();

    expect(api.readSkillsCallCount, 1);
    expect(api.discoverCallCount, 0);
    // The catalog read on entry is visible without tapping Discover.
    expect(find.text('flutter-ui-polish'), findsOneWidget);
    expect(find.text('runtime-review'), findsOneWidget);
  });

  testWidgets('skills discover loads project skill catalog', (tester) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    api.discoveredSkills = const ['flutter-ui-polish', 'runtime-review'];
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Skills'));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Discover'));
    await tester.pumpAndSettle();

    expect(api.discoverCallCount, 1);
    expect(api.discoverProjectId, 'project-1');
    expect(find.text('flutter-ui-polish'), findsOneWidget);
    expect(find.text('runtime-review'), findsOneWidget);
  });

  testWidgets('skills tab re-entry refreshes the catalog on read', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    api.discoveredSkills = const ['flutter-ui-polish', 'runtime-review'];
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Skills'));
    await tester.pumpAndSettle();
    expect(api.discoverCallCount, 0);
    await tester.tap(find.text('Discover'));
    await tester.pumpAndSettle();
    expect(api.discoverCallCount, 1);
    expect(find.text('flutter-ui-polish'), findsOneWidget);
    expect(find.text('runtime-review'), findsOneWidget);

    // Leave the Skills tab.
    await tester.tap(find.text('Providers'));
    await tester.pumpAndSettle();

    // Simulate a changed skill catalog on the backend.
    api.discoveredSkills = const ['new-skill-a', 'new-skill-b'];

    // Re-entry reads the canonical catalog without discovering.
    await tester.tap(find.text('Skills'));
    await tester.pumpAndSettle();
    expect(api.readSkillsCallCount, 2);
    expect(api.discoverCallCount, 1);
    expect(find.text('flutter-ui-polish'), findsNothing);
    expect(find.text('runtime-review'), findsNothing);
    expect(find.text('new-skill-a'), findsOneWidget);
    expect(find.text('new-skill-b'), findsOneWidget);

    // Explicit discover still replaces the whole catalog.
    api.discoveredSkills = const ['final-skill'];
    await tester.tap(find.text('Discover'));
    await tester.pumpAndSettle();

    expect(api.discoverCallCount, 2);
    expect(find.text('new-skill-a'), findsNothing);
    expect(find.text('new-skill-b'), findsNothing);
    expect(find.text('final-skill'), findsOneWidget);
  });

  testWidgets('discovered skills survive tab switching without re-discovery', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    api.discoveredSkills = const ['flutter-ui-polish', 'runtime-review'];
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Skills'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Discover'));
    await tester.pumpAndSettle();
    expect(api.discoverCallCount, 1);
    expect(find.text('flutter-ui-polish'), findsOneWidget);
    expect(find.text('runtime-review'), findsOneWidget);

    // Leave and re-enter the Skills tab.
    await tester.tap(find.text('Providers'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Skills'));
    await tester.pumpAndSettle();

    expect(api.readSkillsCallCount, 2);
    expect(api.discoverCallCount, 1);
    expect(find.text('flutter-ui-polish'), findsOneWidget);
    expect(find.text('runtime-review'), findsOneWidget);
  });

  testWidgets(
    'skills search uses backend description order and clear restores catalog',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final api = _FakeStudioApi(_stateWithPlannerModels());
      api.discoveredSkills = const [
        'rust-formatting',
        'release-build-triage',
        'slide-deck-authoring',
      ];
      api.discoveredSkillSummaries = [
        _settingsSkillSummary(
          'release-build-triage',
          'Diagnose Rust release linker and Cargo profile failures.',
        ),
        _settingsSkillSummary('rust-formatting', 'Format Rust source files.'),
        _settingsSkillSummary(
          'slide-deck-authoring',
          'Create presentations and speaker notes.',
        ),
      ];
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const SettingsPage()),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Skills'));
      await tester.pumpAndSettle();

      expect(
        find.textContaining(
          'Diagnose Rust release linker and Cargo profile failures.',
        ),
        findsOneWidget,
      );
      await tester.enterText(find.byType(TextField).first, 'linker');
      await tester.pump(const Duration(milliseconds: 149));
      expect(api.searchSkillsCallCount, 0);
      await tester.pump(const Duration(milliseconds: 1));
      await tester.pump();

      expect(api.searchSkillsCallCount, 1);
      expect(api.discoverCallCount, 0);
      expect(find.text('release-build-triage'), findsOneWidget);
      expect(find.text('rust-formatting'), findsNothing);

      await tester.tap(find.byType(Switch).first);
      await tester.pumpAndSettle();
      expect(
        (api.savedSkillsSettings!['disabled'] as List<String>),
        contains('release-build-triage'),
      );

      await tester.enterText(find.byType(TextField).first, '');
      await tester.pump();
      expect(find.text('release-build-triage'), findsOneWidget);
      expect(find.text('rust-formatting'), findsOneWidget);
      expect(find.text('slide-deck-authoring'), findsOneWidget);
      expect(
        tester.getTopLeft(find.text('release-build-triage')).dy,
        lessThan(tester.getTopLeft(find.text('rust-formatting')).dy),
      );
    },
  );

  testWidgets('skills search discards late responses', (tester) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    api.discoveredSkills = const [
      'release-build-triage',
      'slide-deck-authoring',
    ];
    api.discoveredSkillSummaries = [
      _settingsSkillSummary('release-build-triage', 'Rust release linker.'),
      _settingsSkillSummary('slide-deck-authoring', 'Presentation slides.'),
    ];
    final first = Completer<SkillSearchResultView>();
    final second = Completer<SkillSearchResultView>();
    api.blockedSkillSearchResponses.addAll([first, second]);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('Skills'));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField).first, 'Rust');
    await tester.pump(const Duration(milliseconds: 150));
    await tester.enterText(find.byType(TextField).first, 'presentation');
    await tester.pump(const Duration(milliseconds: 150));
    expect(api.searchSkillsCallCount, 2);

    second.complete(
      SkillSearchResultView(
        projectId: 'project-1',
        catalogRevision: api._skillsCatalogRevision,
        matches: [api.discoveredSkillSummaries[1]],
        truncated: false,
      ),
    );
    await tester.pump();
    expect(find.text('slide-deck-authoring'), findsOneWidget);

    first.complete(
      SkillSearchResultView(
        projectId: 'project-1',
        catalogRevision: api._skillsCatalogRevision,
        matches: [api.discoveredSkillSummaries[0]],
        truncated: false,
      ),
    );
    await tester.pump();
    expect(find.text('slide-deck-authoring'), findsOneWidget);
    expect(find.text('release-build-triage'), findsNothing);
    expect(api.discoverCallCount, 0);
  });

  test('bootstrap activates the selected healthy project once', () async {
    StudioController.resetStartupProjectActivation();
    addTearDown(StudioController.resetStartupProjectActivation);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    final container = ProviderContainer.test(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await Future<void>.delayed(Duration.zero);
    await container.pump();

    expect(api.activateCallCount, 1);
    expect(api.activatedProjectId, 'project-1');
  });

  test('bootstrap skips activation for a blocked project', () async {
    StudioController.resetStartupProjectActivation();
    addTearDown(StudioController.resetStartupProjectActivation);

    final blockedState = _emptyState().copyWith(
      recoveryState: RecoveryStateSnapshot(
        values: const [
          StudioRecoveryIssue(
            id: 'issue-project',
            scope: RecoveryIssueScope.project,
            category: RecoveryIssueCategory.repository,
            availableActions: [RecoveryIssueAction.removeProject],
            projectId: 'project-1',
            detail: 'Project workspace is unavailable.',
          ),
        ],
      ),
    );
    final api = _FakeStudioApi(blockedState);
    final container = ProviderContainer.test(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await Future<void>.delayed(Duration.zero);
    await container.pump();

    expect(api.activateCallCount, 0);
    expect(api.activatedProjectId, isNull);
  });

  test('controller rebuild does not repeat startup activation', () async {
    StudioController.resetStartupProjectActivation();
    addTearDown(StudioController.resetStartupProjectActivation);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    final container = ProviderContainer.test(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await Future<void>.delayed(Duration.zero);
    await container.pump();
    expect(api.activateCallCount, 1);

    container.invalidate(studioControllerProvider);
    await container.read(studioControllerProvider.future);
    await Future<void>.delayed(Duration.zero);
    await container.pump();

    expect(api.bootstrapCount, 2);
    expect(api.activateCallCount, 1);
  });
}
