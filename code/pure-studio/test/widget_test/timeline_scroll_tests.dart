part of '../widget_test.dart';

void registerTimelineScrollTests() {
  testWidgets('short timeline anchors current activity above its bottom edge', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(980, 520));
    const threadId = 'session-short-activity';

    await tester.pumpWidget(
      _timelineHarness(
        threadId: threadId,
        items: _scrollItems(threadId, 1),
        turnState: const RunningStudioTurnState(
          startedAt: 1,
          activity: StudioTurnActivity.thinking,
        ),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));

    final timelineRect = tester.getRect(find.byKey(StudioDriverKeys.timeline));
    final activityRect = tester.getRect(
      find.byKey(const ValueKey('timeline-current-activity')),
    );
    expect(activityRect.left, closeTo(timelineRect.left + 24, 0.1));
    expect(activityRect.width, lessThanOrEqualTo(700));
    expect(activityRect.bottom, closeTo(timelineRect.bottom - 14, 0.1));
    expect(
      find.byKey(const ValueKey('timeline-current-activity-pulse')),
      findsOneWidget,
    );
  });

  testWidgets('timeline follows appended messages from the bottom', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-scroll',
        items: _scrollItems('session-scroll', 18),
      ),
    );
    await tester.pumpAndSettle();
    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-scroll',
        items: _scrollItems('session-scroll', 19),
      ),
    );
    await tester.pumpAndSettle();

    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));
    expect(find.textContaining('message 18'), findsOneWidget);
  });

  testWidgets('timeline does not steal scroll when user reads older messages', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-detached',
        items: _scrollItems('session-detached', 24),
      ),
    );
    await tester.pumpAndSettle();

    await tester.drag(
      find.byKey(StudioDriverKeys.timeline),
      const Offset(0, 260),
    );
    await tester.pumpAndSettle();
    final offsetBeforeAppend = _timelinePixels(tester);
    expect(_timelineExtentAfter(tester), greaterThan(80));

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-detached',
        items: _scrollItems('session-detached', 25),
      ),
    );
    await tester.pumpAndSettle();

    expect(_timelinePixels(tester), closeTo(offsetBeforeAppend, 1));
    expect(find.byTooltip('Jump to latest'), findsOneWidget);
  });

  testWidgets('jump to latest button restores bottom following', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-jump',
        items: _scrollItems('session-jump', 24),
      ),
    );
    await tester.pumpAndSettle();

    await tester.drag(
      find.byKey(StudioDriverKeys.timeline),
      const Offset(0, 260),
    );
    await tester.pumpAndSettle();
    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-jump',
        items: _scrollItems('session-jump', 25),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Jump to latest'));
    await tester.pumpAndSettle();

    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));
    expect(find.byTooltip('Jump to latest'), findsNothing);
  });

  testWidgets('timeline follows streaming content growth near the bottom', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-streaming',
        items: _scrollItems('session-streaming', 12),
      ),
    );
    await tester.pumpAndSettle();

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-streaming',
        items: _scrollItems('session-streaming', 12, expandedLast: true),
      ),
    );
    await tester.pumpAndSettle();

    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));
  });

  testWidgets('streaming replacements do not count as new detached events', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-stream-count',
        items: _scrollItems('session-stream-count', 24),
      ),
    );
    await tester.pumpAndSettle();
    await tester.drag(
      find.byKey(StudioDriverKeys.timeline),
      const Offset(0, 260),
    );
    await tester.pumpAndSettle();
    expect(
      find.byKey(const ValueKey('timeline-jump-to-latest:0')),
      findsOneWidget,
    );

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-stream-count',
        items: _scrollItems('session-stream-count', 24, expandedLast: true),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      find.byKey(const ValueKey('timeline-jump-to-latest:0')),
      findsOneWidget,
    );

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-stream-count',
        items: _scrollItems('session-stream-count', 25),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      find.byKey(const ValueKey('timeline-jump-to-latest:1')),
      findsOneWidget,
    );
  });

  testWidgets('timeline keeps scroll state isolated per session', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-a',
        items: _scrollItems('session-a', 24),
      ),
    );
    await tester.pumpAndSettle();
    await tester.drag(
      find.byKey(StudioDriverKeys.timeline),
      const Offset(0, 260),
    );
    await tester.pumpAndSettle();
    final sessionAOffset = _timelinePixels(tester);
    expect(_timelineExtentAfter(tester), greaterThan(80));

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-b',
        items: _scrollItems('session-b', 20),
      ),
    );
    await tester.pumpAndSettle();
    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-a',
        items: _scrollItems('session-a', 24),
      ),
    );
    await tester.pumpAndSettle();

    expect(_timelinePixels(tester), closeTo(sessionAOffset, 1));
    expect(find.byTooltip('Jump to latest'), findsOneWidget);
  });

  testWidgets('timeline repairs an out-of-range offset after metrics shrink', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    const threadId = 'session-metrics-shrink';
    final rows = timelineRowsFromThreadItems(_scrollItems(threadId, 24));
    Widget harness(double width) {
      return _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: width,
            height: 520,
            child: TimelineView(
              threadId: threadId,
              rows: rows,
              turn: _testTurn(
                threadId: threadId,
                state: const CompletedStudioTurnState(
                  startedAt: 1,
                  completedAt: 2,
                  completion: StudioTurnCompletion.normal,
                ),
              ),
            ),
          ),
        ),
      );
    }

    await tester.pumpWidget(harness(520));
    await tester.pumpAndSettle();
    final previousExtent = _timelinePosition(tester).maxScrollExtent;

    await tester.pumpWidget(harness(1500));
    await tester.pumpAndSettle();

    final position = _timelinePosition(tester);
    expect(position.maxScrollExtent, lessThan(previousExtent));
    expect(position.pixels, lessThanOrEqualTo(position.maxScrollExtent));
    expect(position.extentAfter, lessThanOrEqualTo(80));
  });

  testWidgets('loading older history preserves the visible timeline anchor', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    const threadId = 'session-history';
    var loadCount = 0;
    final recentItems = _scrollItems(threadId, 24, startIndex: 8);
    await tester.pumpWidget(
      _timelineHarness(
        threadId: threadId,
        items: recentItems,
        onLoadOlder: () => loadCount += 1,
      ),
    );
    await tester.pumpAndSettle();

    await tester.drag(
      find.byKey(StudioDriverKeys.timeline),
      const Offset(0, 5000),
    );
    await tester.pumpAndSettle();
    expect(loadCount, 1);
    final anchor = find.textContaining('message 8 for $threadId');
    final anchorTopBeforeLoad = tester.getTopLeft(anchor).dy;

    await tester.pumpWidget(
      _timelineHarness(
        threadId: threadId,
        items: recentItems,
        onLoadOlder: () => loadCount += 1,
        isLoadingOlder: true,
      ),
    );
    await tester.pump();
    expect(
      find.byKey(const ValueKey('timeline-history-loading')),
      findsOneWidget,
    );

    await tester.pumpWidget(
      _timelineHarness(
        threadId: threadId,
        items: [..._scrollItems(threadId, 8), ...recentItems],
        onLoadOlder: () => loadCount += 1,
      ),
    );
    await tester.pumpAndSettle();

    expect(loadCount, 1);
    expect(tester.getTopLeft(anchor).dy, closeTo(anchorTopBeforeLoad, 1));
  });
}
