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

    testWidgets('status bar keeps an unstarted Thread Mode lightweight', (
      tester,
    ) async {
      final state = _emptyState();
      await _pumpThreadStatusBar(tester, state);

      expect(find.byKey(StudioDriverKeys.sessionMode), findsOneWidget);
      expect(_workflowRuntimeFinder(), findsNothing);
      _expectNoWorkflowInspectorOrMutationUi();
    });

    testWidgets('status bar shows the canonical active workflow state', (
      tester,
    ) async {
      final base = _emptyState();
      final state = _withSelectedRuntime(
        base,
        base.runtime.copyWith(
          workflow: _workflowRuntime(stateId: 'planning', terminal: false),
        ),
      );
      await _pumpThreadStatusBar(tester, state);

      expect(
        find.byKey(const ValueKey('workflow-runtime-run-1')),
        findsOneWidget,
      );
      expect(
        find.byKey(const ValueKey('workflow-state-planning')),
        findsOneWidget,
      );
      expect(find.text('planning'), findsOneWidget);
      expect(
        find.byTooltip(
          'Session mode cannot change while the session is running or a workflow is active',
        ),
        findsOneWidget,
      );
      _expectNoWorkflowInspectorOrMutationUi();
    });

    testWidgets('status bar shows the canonical terminal workflow state', (
      tester,
    ) async {
      final base = _emptyState();
      final state = _withSelectedRuntime(
        base,
        base.runtime.copyWith(
          workflow: _workflowRuntime(stateId: 'completed', terminal: true),
        ),
      );
      await _pumpThreadStatusBar(tester, state);

      expect(
        find.byKey(const ValueKey('workflow-runtime-run-1')),
        findsOneWidget,
      );
      expect(
        find.byKey(const ValueKey('workflow-state-completed')),
        findsOneWidget,
      );
      expect(
        find.byKey(const ValueKey('workflow-state-planning')),
        findsNothing,
      );
      expect(find.byTooltip('Session mode'), findsOneWidget);
      _expectNoWorkflowInspectorOrMutationUi();
    });

    testWidgets('mode selector renders the reloaded canonical mode', (
      tester,
    ) async {
      final state = _emptyState();
      final api = await _pumpThreadStatusBar(tester, state);

      await tester.tap(find.byKey(StudioDriverKeys.sessionMode));
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(StudioDriverKeys.sessionModeOption(ThreadModeId.task.name)),
      );
      await tester.pumpAndSettle();

      final container = ProviderScope.containerOf(
        tester.element(find.byType(ThreadStatusBar)),
      );
      expect(api.modeUpdate, (threadId: 'session-1', mode: ThreadModeId.task));
      expect(
        container.read(studioControllerProvider).value!.selectedThread!.mode,
        ThreadModeId.task,
      );
      expect(find.text('Task'), findsOneWidget);
      expect(_workflowRuntimeFinder(), findsNothing);
      _expectNoWorkflowInspectorOrMutationUi();
    });

    testWidgets(
      'capability detail exposes every active Skill without truncation',
      (tester) async {
        tester.view.physicalSize = const Size(1280, 600);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);

        final base = _emptyState();
        final threadId = base.selectedThreadId!;
        final workspace = base.workspacesByThread[threadId]!;
        final skills = [
          for (var index = 0; index < 40; index++) 'skill-$index',
        ];
        final state = base.copyWith(
          workspacesByThread: {
            threadId: workspace.copyWith(
              runtime: workspace.runtime.copyWith(activeSkills: skills),
            ),
          },
        );
        final api = _FakeStudioApi(state);
        await tester.pumpWidget(
          ProviderScope(
            overrides: [studioApiProvider.overrideWithValue(api)],
            child: _localizedApp(
              home: Scaffold(
                body: Align(
                  alignment: Alignment.bottomLeft,
                  child: ThreadStatusBar(
                    workspace: state.selectedAgentWorkspace!,
                  ),
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();

        final capabilities = find.bySemanticsLabel('Active capabilities');
        expect(capabilities, findsOneWidget);
        await tester.tap(capabilities);
        await tester.pumpAndSettle();

        for (final skill in skills) {
          expect(
            find.byKey(StudioDriverKeys.statusActiveSkill(skill)),
            findsOneWidget,
          );
        }
        final scrollable = find.ancestor(
          of: find.byType(StatusDetailPanel),
          matching: find.byType(Scrollable),
        );
        expect(scrollable, findsOneWidget);
        await tester.scrollUntilVisible(
          find.byKey(StudioDriverKeys.statusActiveSkill(skills.last)),
          180,
          scrollable: scrollable,
        );
        await tester.pumpAndSettle();
        expect(
          find
              .byKey(StudioDriverKeys.statusActiveSkill(skills.last))
              .hitTestable(),
          findsOneWidget,
        );
      },
    );

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

Future<_FakeStudioApi> _pumpThreadStatusBar(
  WidgetTester tester,
  StudioState state,
) async {
  tester.view.physicalSize = const Size(1280, 800);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  final api = _FakeStudioApi(state);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [studioApiProvider.overrideWithValue(api)],
      child: _localizedApp(
        home: Scaffold(
          body: Consumer(
            builder: (context, ref, child) {
              final current = ref.watch(studioControllerProvider).value;
              if (current == null) return const SizedBox.shrink();
              return ThreadStatusBar(
                workspace: current.selectedAgentWorkspace!,
              );
            },
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
  return api;
}

WorkflowRuntimeView _workflowRuntime({
  required String stateId,
  required bool terminal,
}) => WorkflowRuntimeView(
  revision: terminal ? 8 : 2,
  currentRun: WorkflowRunView(
    lineageId: 'lineage-1',
    runId: 'run-1',
    modeId: ThreadModeId.task.id,
    graphRevision: 1,
    graphHash: 'graph-hash',
    currentStateId: stateId,
    terminal: terminal,
    startedAt: DateTime.fromMillisecondsSinceEpoch(1000),
    updatedAt: DateTime.fromMillisecondsSinceEpoch(2000),
  ),
);

Finder _workflowRuntimeFinder() => find.byWidgetPredicate(
  (widget) =>
      widget.key is ValueKey<String> &&
      (widget.key! as ValueKey<String>).value.startsWith('workflow-runtime-'),
);

void _expectNoWorkflowInspectorOrMutationUi() {
  for (final key in [
    'workflow-graph',
    'workflow-history',
    'workflow-transition-control',
  ]) {
    expect(find.byKey(ValueKey(key)), findsNothing);
  }
}

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
