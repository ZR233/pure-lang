part of '../widget_test.dart';

void registerStatusAccessibilityTests() {
  group('status detail accessibility', () {
    testWidgets('context readout is a keyboard-operable semantic button', (
      tester,
    ) async {
      await tester.pumpWidget(
        _localizedApp(
          home: const Scaffold(
            body: Align(
              alignment: Alignment.bottomLeft,
              child: ContextUsageReadout(runtime: _contextRuntime),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      final contextButton = find.bySemanticsLabel('Context');
      expect(contextButton, findsOneWidget);
      var semantics = tester.getSemantics(contextButton);
      var flags = semantics.getSemanticsData().flagsCollection;
      expect(flags.isButton, isTrue);
      expect(flags.isFocused, isNot(Tristate.none));
      expect(semantics.getSemanticsData().value, '42%');
      expect(
        semantics.getSemanticsData().hasAction(SemanticsAction.tap),
        isTrue,
      );

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      semantics = tester.getSemantics(contextButton);
      flags = semantics.getSemanticsData().flagsCollection;
      expect(flags.isFocused, Tristate.isTrue);

      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(find.text('42 / 100'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.text('42 / 100'), findsNothing);

      await tester.sendKeyEvent(LogicalKeyboardKey.space);
      await tester.pumpAndSettle();
      expect(find.text('42 / 100'), findsOneWidget);
    });

    testWidgets('cache readout exposes aggregate billing details', (
      tester,
    ) async {
      await tester.pumpWidget(
        _localizedApp(
          home: const Scaffold(
            body: Align(
              alignment: Alignment.bottomLeft,
              child: ContextUsageReadout(runtime: _cacheRuntime),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('Cache 40%'), findsNothing);
      final contextButton = find.bySemanticsLabel('Context');
      expect(
        tester.getSemantics(contextButton).getSemanticsData().value,
        '42%',
      );

      await tester.tap(contextButton);
      await tester.pumpAndSettle();

      for (final value in [
        '400',
        '600',
        '50',
        '75',
        '3',
        '40%',
        r'$0.0025 + ￥0.31 · Partially unpriced',
        r'$0.0012',
      ]) {
        expect(find.text(value), findsOneWidget, reason: value);
      }
    });

    testWidgets('context detail shows dash when runtime has no costs', (
      tester,
    ) async {
      await tester.pumpWidget(
        _localizedApp(
          home: const Scaffold(
            body: Align(
              alignment: Alignment.bottomLeft,
              child: ContextUsageReadout(runtime: _contextRuntime),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      final contextButton = find.bySemanticsLabel('Context');
      await tester.tap(contextButton);
      await tester.pumpAndSettle();

      expect(find.text('Cost'), findsOneWidget);
      expect(find.text('-'), findsOneWidget);
      expect(find.text('Partially unpriced'), findsNothing);
    });

    testWidgets('context detail shows dash for fully unpriced usage', (
      tester,
    ) async {
      await tester.pumpWidget(
        _localizedApp(
          home: const Scaffold(
            body: Align(
              alignment: Alignment.bottomLeft,
              child: ContextUsageReadout(runtime: _unpricedRuntime),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      final contextButton = find.bySemanticsLabel('Context');
      await tester.tap(contextButton);
      await tester.pumpAndSettle();

      expect(find.text('Cost'), findsOneWidget);
      expect(find.text('-'), findsOneWidget);
      expect(find.text('Partially unpriced'), findsNothing);
    });

    testWidgets('status bar omits direct cost and cache text readouts', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final base = _emptyState();
      final workspace = base.workspacesByThread[base.selectedThreadId!];
      final state = base.copyWith(
        workspacesByThread: {
          base.selectedThreadId!: workspace!.copyWith(runtime: _cacheRuntime),
        },
      );
      final api = _FakeStudioApi(state);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(
            home: Scaffold(
              body: ThreadStatusBar(workspace: state.selectedAgentWorkspace!),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.byKey(StudioDriverKeys.contextUsage()), findsOneWidget);
      expect(find.textContaining('￥'), findsNothing);
      expect(find.textContaining(r'$'), findsNothing);
      expect(find.textContaining('Cache'), findsNothing);

      await tester.tap(find.byKey(StudioDriverKeys.contextUsage()));
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.contextUsageDetail()), findsOneWidget);
      expect(find.textContaining(r'$0.0025 + ￥0.31'), findsOneWidget);
    });

    testWidgets('context detail keeps hover behavior and shared radius', (
      tester,
    ) async {
      await tester.pumpWidget(
        _localizedApp(
          home: const Scaffold(
            body: Align(
              alignment: Alignment.bottomLeft,
              child: ContextUsageReadout(runtime: _contextRuntime),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      final contextButton = find.bySemanticsLabel('Context');
      final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
      addTearDown(gesture.removePointer);
      await gesture.addPointer();
      await gesture.moveTo(tester.getCenter(contextButton));
      await tester.pumpAndSettle();

      final detailValue = find.text('42 / 100');
      expect(detailValue, findsOneWidget);
      final detailCard = find.ancestor(
        of: detailValue,
        matching: find.byWidgetPredicate(_isLiftedDetailCard),
      );
      expect(detailCard, findsOneWidget);
      final decoration =
          tester.widget<DecoratedBox>(detailCard).decoration as BoxDecoration;
      expect(decoration.borderRadius, BorderRadius.circular(StudioRadii.md));

      await gesture.moveTo(Offset.zero);
      await tester.pump(const Duration(milliseconds: 150));
      expect(detailValue, findsNothing);
    });

    testWidgets('context detail survives quick focus return', (tester) async {
      final contextButton = await _pumpContextWithNextFocus(tester);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.space);
      await tester.pump();
      expect(find.text('42 / 100'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump(const Duration(milliseconds: 60));
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pump();
      expect(
        tester
            .getSemantics(contextButton)
            .getSemanticsData()
            .flagsCollection
            .isFocused,
        Tristate.isTrue,
      );

      await tester.pump(const Duration(milliseconds: 80));
      expect(find.text('42 / 100'), findsOneWidget);
    });

    testWidgets('context detail stays open when focused pointer exits', (
      tester,
    ) async {
      final contextButton = await _pumpContextWithNextFocus(tester);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.space);
      await tester.pump();
      expect(find.text('42 / 100'), findsOneWidget);

      final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
      addTearDown(gesture.removePointer);
      await gesture.addPointer();
      await gesture.moveTo(tester.getCenter(contextButton));
      await tester.pump();
      await gesture.moveTo(Offset.zero);
      await tester.pump(const Duration(milliseconds: 150));

      expect(
        tester
            .getSemantics(contextButton)
            .getSemanticsData()
            .flagsCollection
            .isFocused,
        Tristate.isTrue,
      );
      expect(find.text('42 / 100'), findsOneWidget);
    });

    testWidgets('context detail closes after focus leaves', (tester) async {
      await _pumpContextWithNextFocus(tester);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.space);
      await tester.pump();
      expect(find.text('42 / 100'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump(const Duration(milliseconds: 150));
      expect(find.text('42 / 100'), findsNothing);
    });

    testWidgets('status bar shows LSP activity readout for active servers', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final state = _emptyState();
      final api = _FakeStudioApi(state);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(
            home: Scaffold(
              body: ThreadStatusBar(workspace: state.selectedAgentWorkspace!),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.byKey(StudioDriverKeys.lspActivity()), findsOneWidget);
      expect(find.byKey(StudioDriverKeys.lspActivityDetail()), findsOneWidget);
      expect(find.text('Indexing 40%'), findsOneWidget);

      await tester.tap(find.byKey(StudioDriverKeys.lspActivityDetail()));
      await tester.pumpAndSettle();
      expect(find.text('LSP'), findsWidgets);
      expect(find.textContaining('rust-analyzer'), findsOneWidget);
      expect(
        find.textContaining('Indexing · 40% · Roots Scanned · 166/408'),
        findsOneWidget,
      );
    });

    testWidgets('status bar hides LSP activity readout when idle', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final state = _emptyState().copyWith(
        lspState: LspStateSnapshot(
          servers: [
            LspServerStateView(
              id: 'rust-analyzer',
              displayName: 'rust-analyzer',
              state: LspAvailableState(
                checkedAt: 0,
                diagnosticCount: 0,
                activity: LspIdleActivity(),
              ),
            ),
          ],
        ),
      );
      final api = _FakeStudioApi(state);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(
            home: Scaffold(
              body: ThreadStatusBar(workspace: state.selectedAgentWorkspace!),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.byKey(StudioDriverKeys.lspActivity()), findsNothing);
      expect(find.text('Indexing'), findsNothing);
    });

    testWidgets('status bar keeps LSP activity reachable at narrow widths', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(800, 600);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final state = _emptyState();
      final api = _FakeStudioApi(state);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(
            home: Scaffold(
              body: ThreadStatusBar(workspace: state.selectedAgentWorkspace!),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.byKey(StudioDriverKeys.lspActivity()), findsNothing);
      final overflow = find.byKey(const ValueKey('status-overflow'));
      expect(overflow, findsOneWidget);
      await tester.tap(overflow);
      await tester.pumpAndSettle();
      final overflowItem = find.byKey(StudioDriverKeys.lspActivityOverflow());
      expect(overflowItem, findsOneWidget);
      expect(
        find.textContaining('rust-analyzer · Indexing · 40%'),
        findsOneWidget,
      );

      await tester.tap(overflowItem);
      await tester.pumpAndSettle();
      expect(find.textContaining('rust-analyzer'), findsWidgets);
      expect(
        find.textContaining('Indexing · 40% · Roots Scanned · 166/408'),
        findsOneWidget,
      );
    });

    testWidgets(
      'status bar overflow LSP detail scrolls with many active servers',
      (tester) async {
        tester.view.physicalSize = const Size(800, 600);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);

        final state = _emptyState().copyWith(
          lspState: LspStateSnapshot(
            servers: [
              for (var i = 0; i < 12; i++)
                LspServerStateView(
                  id: 'lsp-server-$i',
                  displayName: 'lsp-server-$i',
                  state: LspAvailableState(
                    checkedAt: 0,
                    diagnosticCount: 0,
                    activity: LspIndexingActivity(percentage: i * 5),
                  ),
                ),
            ],
          ),
        );
        final api = _FakeStudioApi(state);
        await tester.pumpWidget(
          ProviderScope(
            overrides: [studioApiProvider.overrideWithValue(api)],
            child: _localizedApp(
              home: Scaffold(
                body: ThreadStatusBar(workspace: state.selectedAgentWorkspace!),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();

        expect(find.byKey(StudioDriverKeys.lspActivity()), findsNothing);
        final overflow = find.byKey(const ValueKey('status-overflow'));
        expect(overflow, findsOneWidget);
        await tester.tap(overflow);
        await tester.pumpAndSettle();
        final overflowItem = find.byKey(StudioDriverKeys.lspActivityOverflow());
        expect(overflowItem, findsOneWidget);

        await tester.tap(overflowItem);
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);

        final lastServer = find.textContaining('lsp-server-11');
        expect(lastServer, findsOneWidget);
        await tester.scrollUntilVisible(
          lastServer,
          200,
          scrollable: find.descendant(
            of: find.byType(Dialog),
            matching: find.byType(Scrollable),
          ),
        );
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(lastServer.hitTestable(), findsOneWidget);
      },
    );
  });
}

Future<Finder> _pumpContextWithNextFocus(WidgetTester tester) async {
  await tester.pumpWidget(
    _localizedApp(
      home: Scaffold(
        body: Align(
          alignment: Alignment.bottomLeft,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const ContextUsageReadout(
                key: _contextFocusTargetKey,
                runtime: _contextRuntime,
              ),
              TextButton(onPressed: () {}, child: const Text('Next focus')),
            ],
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
  final contextButton = find.descendant(
    of: find.byKey(_contextFocusTargetKey),
    matching: find.bySemanticsLabel('Context'),
  );
  expect(contextButton, findsOneWidget);
  return contextButton;
}

const _contextFocusTargetKey = ValueKey('context-focus-target');

const _contextRuntime = ThreadRuntimeView(
  model: 'planner/local',
  contextTokens: 42,
  contextWindow: 100,
  totalTokens: 128,
  costLabel: '',
  activeSkills: [],
  activeMcpServers: [],
  activeLspServers: [],
  agentCount: 0,
);

const _unpricedRuntime = ThreadRuntimeView(
  model: 'planner/local',
  contextTokens: 42,
  contextWindow: 100,
  totalTokens: 128,
  costLabel: '',
  activeSkills: [],
  activeMcpServers: [],
  activeLspServers: [],
  agentCount: 0,
  hasUnpricedUsage: true,
);

const _cacheRuntime = ThreadRuntimeView(
  model: 'gpt-5.6-sol',
  contextTokens: 42,
  contextWindow: 100,
  totalTokens: 1200,
  costLabel: r'$0.0025',
  activeSkills: [],
  activeMcpServers: [],
  activeLspServers: [],
  agentCount: 0,
  promptTokens: 1000,
  completionTokens: 200,
  cachedPromptTokens: 400,
  cacheWriteTokens: 50,
  cacheMissTokens: 600,
  reasoningTokens: 75,
  inferenceCount: 3,
  cacheHitRate: 0.4,
  estimatedCosts: [
    RuntimeCostView(currency: 'USD', amount: 0.0025),
    RuntimeCostView(currency: 'CNY', amount: 0.31),
  ],
  estimatedCacheSavings: [RuntimeCostView(currency: 'USD', amount: 0.0012)],
  hasUnpricedUsage: true,
  promptGeneration: 2,
  promptCachePolicy: 'openAiPromptCacheKey',
  prefixChangedReason: 'contextAppended',
);
