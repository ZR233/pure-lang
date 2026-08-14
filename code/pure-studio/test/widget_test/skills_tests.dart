part of '../widget_test.dart';

void registerSkillsTests() {
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

    expect(api.discoverCallCount, 0);
    await tester.tap(find.text('Discover'));
    await tester.pumpAndSettle();

    expect(api.discoverProjectId, 'project-1');
    expect(find.text('flutter-ui-polish'), findsOneWidget);
    expect(find.text('runtime-review'), findsOneWidget);
  });

  testWidgets('skills tab re-entry clears stale skills and refreshes', (
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

    // Re-entering is pure read; explicit discover applies the changed catalog.
    await tester.tap(find.text('Skills'));
    await tester.pumpAndSettle();
    expect(api.discoverCallCount, 1);
    await tester.tap(find.text('Discover'));
    await tester.pumpAndSettle();

    expect(api.discoverCallCount, 2);
    // Stale entries replaced by the fresh snapshot.
    expect(find.text('flutter-ui-polish'), findsNothing);
    expect(find.text('runtime-review'), findsNothing);
    expect(find.text('new-skill-a'), findsOneWidget);
    expect(find.text('new-skill-b'), findsOneWidget);
  });
}
