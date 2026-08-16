part of '../widget_test.dart';

void registerAppLifecycleTests() {
  testWidgets('switching Threads renders the addressed workspace', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(1440, 900));
    await _pumpStudioApp(tester, DriverDemoStudioApi());

    expect(find.byKey(StudioDriverKeys.shell), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.timeline), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.threadRow('thread-alt')));
    await tester.pumpAndSettle();
    expect(
      find.text('Riverpod selector boundary is isolated.'),
      findsOneWidget,
    );

    await tester.tap(find.byKey(StudioDriverKeys.threadRow('thread-main')));
    await tester.pumpAndSettle();
    expect(find.text('Riverpod selector boundary is isolated.'), findsNothing);
    expect(tester.takeException(), isNull);
    await _disposeStudioApp(tester);
  });

  testWidgets('unmounting the app tree keeps dispose-time shutdown safe', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(1440, 900));
    await _pumpStudioApp(tester, DemoStudioApi());

    // 卸载整棵树会触发 StudioLifecycleCoordinator.dispose 的默认关机路径；
    // 该路径不得读取已卸载 widget 的 ref，也不得留下未完成的关机计时器。
    await _disposeStudioApp(tester);
    expect(tester.takeException(), isNull);
  });
}

Future<void> _pumpStudioApp(WidgetTester tester, StudioApi api) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        studioApiProvider.overrideWithValue(api),
        studioUpdateEnabledProvider.overrideWithValue(false),
      ],
      child: const PureStudioApp(),
    ),
  );
  await tester.pumpAndSettle();
  expect(tester.takeException(), isNull);
}

Future<void> _disposeStudioApp(WidgetTester tester) async {
  await tester.pumpWidget(const SizedBox());
  // demo shutdownRuntime 按阶段延时推进（DriverDemo 每 400ms × 8 段）；
  // 分步推进假时钟让关机 future 完成。
  for (var index = 0; index < 12; index++) {
    await tester.pump(const Duration(milliseconds: 400));
  }
}
