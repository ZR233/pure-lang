part of '../widget_test.dart';

const _responsiveSessionTitle =
    'Responsive layout audit for an intentionally long Pure Studio session title';
const _responsiveProviderName =
    'DeepSeek Enterprise Provider With An Intentionally Long Display Name';
const _responsiveProviderSubtitle =
    'Primary workspace credential for a long-running production environment';
const _responsiveActivityLabel = '1 skill · 1 MCP · 1 LSP · 2 agents';

const _responsiveViewports = [
  (name: '1280x800', size: Size(1280, 800)),
  (name: '900x700', size: Size(900, 700)),
  (name: '760x720', size: Size(760, 720)),
];

void registerResponsiveLayoutTests() {
  group('responsive visual regression', () {
    for (final viewport in _responsiveViewports) {
      testWidgets('chat page remains usable at ${viewport.name}', (
        tester,
      ) async {
        _configureResponsiveView(tester, viewport.size);
        final api = _FakeStudioApi(_responsiveShellState());

        await tester.pumpWidget(
          ProviderScope(
            overrides: [studioApiProvider.overrideWithValue(api)],
            child: _localizedApp(home: const StudioShell()),
          ),
        );
        await tester.pumpAndSettle();

        final titles = tester
            .widgetList<Text>(find.text(_responsiveSessionTitle))
            .toList();
        expect(titles, isNotEmpty);
        expect(titles.every((title) => title.maxLines == 1), isTrue);
        expect(
          titles.every((title) => title.overflow == TextOverflow.ellipsis),
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
        'activity popover stays clear of its trigger at ${viewport.name}',
        (tester) async {
          _configureResponsiveView(tester, viewport.size);
          final api = _FakeStudioApi(_responsiveShellState());

          await tester.pumpWidget(
            ProviderScope(
              overrides: [studioApiProvider.overrideWithValue(api)],
              child: _localizedApp(home: const StudioShell()),
            ),
          );
          await tester.pumpAndSettle();

          final trigger = find.text(_responsiveActivityLabel);
          expect(trigger, findsOneWidget);
          await tester.ensureVisible(trigger);
          await tester.pumpAndSettle();
          final triggerRect = tester.getRect(trigger);
          expect(_rectFitsViewport(triggerRect), isTrue);

          await tester.tapAt(
            Offset(triggerRect.left + 8, triggerRect.center.dy),
          );
          await tester.pumpAndSettle();

          expect(find.text('ACTIVE CAPABILITIES'), findsOneWidget);
          expect(find.textContaining('Skills · flutter-ui'), findsOneWidget);
          expect(find.text('SUBAGENTS'), findsOneWidget);
          expect(find.text('reviewer'), findsOneWidget);
          expect(find.text('worker'), findsOneWidget);
          final detailCard = find.ancestor(
            of: find.text('ACTIVE CAPABILITIES'),
            matching: find.byWidgetPredicate(_isLiftedDetailCard),
          );
          expect(detailCard, findsOneWidget);
          final detailRect = tester.getRect(detailCard);
          expect(_rectFitsViewport(detailRect, inset: 8), isTrue);
          expect(detailRect.bottom, lessThanOrEqualTo(triggerRect.top - 8));
          expect(detailRect.overlaps(triggerRect), isFalse);
          expect(tester.takeException(), isNull);
        },
      );

      testWidgets('provider settings remain usable at ${viewport.name}', (
        tester,
      ) async {
        _configureResponsiveView(tester, viewport.size);
        final api = _FakeStudioApi(
          _responsiveProviderState(),
          providerUsages: _providerListUsages,
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
        expect(find.byTooltip('Provider actions'), findsNWidgets(2));
        final providerTitle = tester.widget<Text>(
          find.text(_responsiveProviderName),
        );
        expect(providerTitle.maxLines, 1);
        expect(providerTitle.overflow, TextOverflow.ellipsis);
        final providerSubtitle = tester.widget<Text>(
          find.text(_responsiveProviderSubtitle),
        );
        expect(providerSubtitle.maxLines, 1);
        expect(providerSubtitle.overflow, TextOverflow.ellipsis);
        final titleRect = tester.getRect(find.text(_responsiveProviderName));
        final menuRect = tester.getRect(
          find.byTooltip('Provider actions').first,
        );
        expect(titleRect.overlaps(menuRect), isFalse);

        final zhipu = find.text('Zhipu Coding Plan');
        await tester.ensureVisible(zhipu);
        await tester.pumpAndSettle();
        expect(_rectFitsViewport(tester.getRect(zhipu)), isTrue);
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

StudioState _responsiveShellState() {
  final history = _sessionHistoryState(
    projectId: 'project-1',
    sessionId: 'session-1',
    text:
        'Responsive viewport checks keep the active conversation readable '
        'without hiding timeline content.',
  );
  final planner = _stateWithPlannerModels();
  final updatedAt = DateTime.fromMillisecondsSinceEpoch(1);
  return history.copyWith(
    projects: const [
      StudioProject(
        id: 'project-1',
        name: 'pure-lang-responsive-workspace',
        path:
            r'C:\Users\zhoudongsheng\Documents\opensource\pure-lang\.worktrees\gui-refactor',
      ),
    ],
    sessions: [
      history.sessions.single.copyWith(title: _responsiveSessionTitle),
    ],
    providers: planner.providers,
    defaultProviderId: 'deepseek',
    roles: planner.roles,
    runtime: const SessionRuntimeView(
      model: 'planner/local',
      contextTokens: 42000,
      contextWindow: 100000,
      totalTokens: 128000,
      costLabel: 'CNY 12.34',
      activeSkills: ['flutter-ui'],
      activeMcpServers: ['dart'],
      activeLspServers: ['rust-analyzer'],
      agentCount: 2,
    ),
    agentsBySession: {
      'session-1': {
        'agent-reviewer': StudioAgentView(
          id: 'agent-reviewer',
          sessionId: 'session-1',
          path: 'root/reviewer',
          role: 'reviewer',
          task: 'Audit responsive layout and visual geometry',
          status: 'running',
          summary: 'Checking the activity popover against its trigger.',
          updatedAt: updatedAt,
        ),
        'agent-worker': StudioAgentView(
          id: 'agent-worker',
          sessionId: 'session-1',
          path: 'root/worker',
          role: 'worker',
          task: 'Capture responsive screenshots',
          status: 'completed',
          updatedAt: updatedAt,
        ),
      },
    },
  );
}

StudioState _responsiveProviderState() {
  final state = _providerListState();
  return state.copyWith(
    providers: [
      state.providers.first.copyWith(
        name: _responsiveProviderName,
        subtitle: _responsiveProviderSubtitle,
      ),
      state.providers.last,
    ],
  );
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
