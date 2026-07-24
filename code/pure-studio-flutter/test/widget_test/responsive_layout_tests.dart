part of '../widget_test.dart';

const _responsiveAgentCountLabel = '3 agents';

const _responsiveViewports = [
  (name: '1280x800', size: Size(1280, 800)),
  (name: '900x700', size: Size(900, 700)),
  (name: '760x720', size: Size(760, 720)),
];

const _activityStressViewports = [
  (name: '900x700', size: Size(900, 700)),
  (name: '760x720', size: Size(760, 720)),
];

const _agentStressLabel = '16 agents';

void registerResponsiveLayoutTests() {
  group('responsive visual regression', () {
    testWidgets('desktop settings navigation renders shared token width', (
      tester,
    ) async {
      _configureResponsiveView(tester, const Size(1280, 800));
      final api = _FakeStudioApi(responsiveVisualState());

      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const SettingsPage()),
        ),
      );
      await tester.pumpAndSettle();

      final settingsNavigation = find.ancestor(
        of: find.text('Back to chat'),
        matching: find.byType(ListView),
      );
      expect(settingsNavigation, findsOneWidget);
      expect(
        tester.getSize(settingsNavigation).width,
        StudioLayout.settingsNavigationWidth,
      );
      expect(tester.takeException(), isNull);
    });

    for (final viewport in _responsiveViewports) {
      testWidgets('chat page remains usable at ${viewport.name}', (
        tester,
      ) async {
        _configureResponsiveView(tester, viewport.size);
        final api = _FakeStudioApi(responsiveVisualState());

        await tester.pumpWidget(
          ProviderScope(
            overrides: [studioApiProvider.overrideWithValue(api)],
            child: _localizedApp(home: const StudioShell()),
          ),
        );
        await tester.pumpAndSettle();

        final titles = tester
            .widgetList<Text>(find.text(responsiveVisualSessionTitle))
            .toList();
        expect(titles, isNotEmpty);
        expect(titles.every((title) => title.maxLines == 1), isTrue);
        expect(
          titles.every((title) => title.overflow == TextOverflow.ellipsis),
          isTrue,
        );
        final titleParagraphs = _renderParagraphs(
          find.text(responsiveVisualSessionTitle),
        );
        expect(titleParagraphs, hasLength(titles.length));
        expect(
          titleParagraphs.every((paragraph) => paragraph.didExceedMaxLines),
          isTrue,
        );
        expect(
          find.byKey(const ValueKey('timeline-scrollable')),
          findsOneWidget,
        );
        final composer = find.byWidgetPredicate(
          (widget) =>
              widget is TextField &&
              widget.decoration?.hintText == 'Describe what you need...',
        );
        expect(composer, findsOneWidget);
        expect(find.byTooltip('Permission mode'), findsOneWidget);
        expect(_rectFitsViewport(tester.getRect(composer)), isTrue);
        expect(tester.takeException(), isNull);
      });

      testWidgets(
        'agent switcher stays clear of its trigger at ${viewport.name}',
        (tester) async {
          _configureResponsiveView(tester, viewport.size);
          final api = _FakeStudioApi(responsiveVisualState());

          await tester.pumpWidget(
            ProviderScope(
              overrides: [studioApiProvider.overrideWithValue(api)],
              child: _localizedApp(home: const StudioShell()),
            ),
          );
          await tester.pumpAndSettle();

          final trigger = find.text(_responsiveAgentCountLabel);
          expect(trigger, findsOneWidget);
          await tester.ensureVisible(trigger);
          await tester.pumpAndSettle();
          final triggerRect = tester.getRect(trigger);
          expect(_rectFitsViewport(triggerRect), isTrue);

          await tester.tapAt(
            Offset(triggerRect.left + 8, triggerRect.center.dy),
          );
          await tester.pumpAndSettle();

          expect(find.text('Planner'), findsOneWidget);
          expect(find.text('Responsive reviewer'), findsOneWidget);
          expect(find.text('Capture worker'), findsOneWidget);
          final reviewerItem = find.byKey(
            const ValueKey('agent-session-session-reviewer'),
          );
          expect(reviewerItem, findsOneWidget);
          final stableTriggerRect = tester.getRect(trigger);
          final reviewerRect = tester.getRect(reviewerItem);
          expect(
            _rectFitsViewport(reviewerRect, inset: 8),
            isTrue,
            reason: 'reviewer=$reviewerRect trigger=$stableTriggerRect',
          );
          expect(reviewerRect.top, greaterThan(stableTriggerRect.bottom));
          expect(reviewerRect.overlaps(stableTriggerRect), isFalse);
          expect(
            (reviewerRect.right - stableTriggerRect.right).abs(),
            lessThanOrEqualTo(32),
            reason: 'reviewer=$reviewerRect trigger=$stableTriggerRect',
          );
          expect(tester.takeException(), isNull);
        },
      );

      testWidgets('provider settings remain usable at ${viewport.name}', (
        tester,
      ) async {
        _configureResponsiveView(tester, viewport.size);
        final providerState = responsiveVisualState();
        final api = _FakeStudioApi(
          providerState,
          providerUsages: responsiveVisualProviderUsages,
        );

        await tester.pumpWidget(
          ProviderScope(
            overrides: [studioApiProvider.overrideWithValue(api)],
            child: _localizedApp(home: const SettingsPage()),
          ),
        );
        await tester.pumpAndSettle();

        expect(find.text('Providers'), findsWidgets);
        expect(find.text('Search providers'), findsOneWidget);
        expect(
          find.byTooltip('Provider actions'),
          findsNWidgets(providerState.providers.length),
        );
        final providerTitle = tester.widget<Text>(
          find.text(responsiveVisualProviderName),
        );
        expect(providerTitle.maxLines, 1);
        expect(providerTitle.overflow, TextOverflow.ellipsis);
        expect(
          _renderParagraphs(
            find.text(responsiveVisualProviderName),
          ).single.didExceedMaxLines,
          isTrue,
        );
        final providerSubtitle = tester.widget<Text>(
          find.text(responsiveVisualProviderSubtitle),
        );
        expect(providerSubtitle.maxLines, 1);
        expect(providerSubtitle.overflow, TextOverflow.ellipsis);
        expect(
          _renderParagraphs(
            find.text(responsiveVisualProviderSubtitle),
          ).single.didExceedMaxLines,
          isTrue,
        );
        final titleRect = tester.getRect(
          find.text(responsiveVisualProviderName),
        );
        final menuRect = tester.getRect(
          find.byTooltip('Provider actions').first,
        );
        expect(titleRect.overlaps(menuRect), isFalse);
        expect(
          tester
              .getRect(find.text(responsiveVisualProviderSubtitle))
              .overlaps(menuRect),
          isFalse,
        );

        final providerScrollView = find.byType(SingleChildScrollView);
        expect(providerScrollView, findsOneWidget);
        final providerScrollable = find.descendant(
          of: providerScrollView,
          matching: find.byType(Scrollable),
        );
        expect(providerScrollable, findsOneWidget);
        final scrollPosition = tester
            .state<ScrollableState>(providerScrollable)
            .position;
        expect(scrollPosition.maxScrollExtent, greaterThan(0));
        final initialScrollOffset = scrollPosition.pixels;

        final bottomQuota = find.text('MCP quota');
        await tester.scrollUntilVisible(
          bottomQuota,
          180,
          scrollable: providerScrollable,
        );
        await tester.pumpAndSettle();
        expect(scrollPosition.pixels, greaterThan(initialScrollOffset));
        final quotaRect = tester.getRect(bottomQuota);
        final quotaBarRect = tester.getRect(
          find.byType(LinearProgressIndicator).last,
        );
        expect(_rectFitsViewport(quotaRect), isTrue);
        expect(_rectFitsViewport(quotaBarRect), isTrue);
        expect(quotaBarRect.top, greaterThan(quotaRect.bottom));
        expect(quotaBarRect.height, 5);

        final targetMenu = find.byTooltip('Provider actions').last;
        final targetMenuRect = tester.getRect(targetMenu);
        expect(_rectFitsViewport(targetMenuRect), isTrue);
        await tester.tap(targetMenu);
        await tester.pumpAndSettle();

        final popupScrollView = find.ancestor(
          of: find.text('Set as default'),
          matching: find.byType(SingleChildScrollView),
        );
        expect(popupScrollView, findsOneWidget);
        expect(
          _rectFitsViewport(tester.getRect(popupScrollView), inset: 8),
          isTrue,
        );
        for (final label in const [
          'Set as default',
          'Refresh usage',
          'Edit provider',
          'Delete provider',
        ]) {
          final entry = find.descendant(
            of: popupScrollView,
            matching: find.text(label),
          );
          expect(entry, findsOneWidget);
          expect(_rectFitsViewport(tester.getRect(entry), inset: 8), isTrue);
        }
        expect(tester.takeException(), isNull);
      });
    }

    for (final viewport in _activityStressViewports) {
      testWidgets('expanded agent switcher is bounded and scrollable at '
          '${viewport.name}', (tester) async {
        _configureResponsiveView(tester, viewport.size);
        final api = _FakeStudioApi(_responsiveActivityStressState());

        await tester.pumpWidget(
          ProviderScope(
            overrides: [studioApiProvider.overrideWithValue(api)],
            child: _localizedApp(home: const StudioShell()),
          ),
        );
        await tester.pumpAndSettle();

        final trigger = find.text(_agentStressLabel);
        expect(trigger, findsOneWidget);
        await tester.ensureVisible(trigger);
        await tester.pumpAndSettle();
        final triggerRect = tester.getRect(trigger);

        await tester.tapAt(Offset(triggerRect.left + 8, triggerRect.center.dy));
        await tester.pumpAndSettle();
        final firstAgent = find.byKey(
          const ValueKey('agent-session-stress-session-1'),
        );
        expect(firstAgent, findsOneWidget);
        final menuScrollable = find.ancestor(
          of: firstAgent,
          matching: find.byType(Scrollable),
        );
        expect(menuScrollable, findsOneWidget);
        final menuRect = tester.getRect(menuScrollable);
        final stableTriggerRect = tester.getRect(trigger);
        expect(
          _rectFitsViewport(menuRect, inset: 8),
          isTrue,
          reason: 'menu=$menuRect trigger=$stableTriggerRect',
        );
        expect(menuRect.top, greaterThan(stableTriggerRect.bottom));
        expect(menuRect.overlaps(stableTriggerRect), isFalse);
        final position = tester.state<ScrollableState>(menuScrollable).position;
        expect(position.maxScrollExtent, greaterThan(0));
        final initialOffset = position.pixels;
        await tester.drag(firstAgent, const Offset(0, -120));
        await tester.pumpAndSettle();
        expect(position.pixels, greaterThan(initialOffset));
        expect(tester.takeException(), isNull);
      });
    }
  });
}

void _configureResponsiveView(WidgetTester tester, Size size) {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
}

bool _isLiftedDetailCard(Widget widget) {
  if (widget is! DecoratedBox) {
    return false;
  }
  final decoration = widget.decoration;
  return decoration is BoxDecoration &&
      decoration.border != null &&
      (decoration.boxShadow?.isNotEmpty ?? false);
}

List<RenderParagraph> _renderParagraphs(Finder finder) {
  return [
    for (final element in finder.evaluate())
      if (element.renderObject case final RenderParagraph paragraph) paragraph,
  ];
}

StudioState _responsiveActivityStressState() {
  final state = responsiveVisualState();
  final updatedAt = DateTime.fromMillisecondsSinceEpoch(1735689600000);
  final root = state.selectedRootSession!;
  final agentSessions = [
    for (var index = 0; index < 15; index++)
      StudioSession(
        id: 'stress-session-${index + 1}',
        projectId: root.projectId,
        title: 'reviewer ${index + 1}',
        mode: root.mode,
        createdAt: updatedAt.add(Duration(seconds: index + 1)),
        updatedAt: updatedAt.add(Duration(seconds: index + 1)),
        parentSessionId: root.id,
        rootSessionId: root.id,
        sessionKind: StudioSessionKind.agent,
        ownerAgentId: 'stress-agent-${index + 1}',
        ownerRole: index.isEven ? 'reviewer' : 'worker',
        agentStatus: index.isEven ? 'running' : 'completed',
      ),
  ];
  return state.copyWith(
    sessions: [root, ...agentSessions],
    runtime: state.runtime.copyWith(agentCount: agentSessions.length),
  );
}

bool _rectFitsViewport(Rect rect, {double inset = 0}) {
  final size = TestWidgetsFlutterBinding
      .instance
      .platformDispatcher
      .views
      .single
      .physicalSize;
  return rect.left >= inset &&
      rect.top >= inset &&
      rect.right <= size.width - inset &&
      rect.bottom <= size.height - inset;
}
