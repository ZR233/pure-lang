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
    expect(find.text('flutter-ui-polish'), findsNothing);

    await tester.tap(find.widgetWithText(FilledButton, 'Discover'));
    await tester.pumpAndSettle();

    expect(api.discoverProjectId, 'project-1');
    expect(find.text('flutter-ui-polish'), findsOneWidget);
    expect(find.text('runtime-review'), findsOneWidget);
  });
}
