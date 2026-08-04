part of '../widget_test.dart';

void registerTimelineScrollTests() {
  testWidgets('timeline follows appended messages from the bottom', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-scroll',
        messages: _scrollMessages('session-scroll', 18),
      ),
    );
    await tester.pumpAndSettle();
    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-scroll',
        messages: _scrollMessages('session-scroll', 19),
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
        sessionId: 'session-detached',
        messages: _scrollMessages('session-detached', 24),
      ),
    );
    await tester.pumpAndSettle();

    await tester.drag(find.byType(ListView), const Offset(0, 260));
    await tester.pumpAndSettle();
    final offsetBeforeAppend = _timelinePixels(tester);
    expect(_timelineExtentAfter(tester), greaterThan(80));

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-detached',
        messages: _scrollMessages('session-detached', 25),
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
        sessionId: 'session-jump',
        messages: _scrollMessages('session-jump', 24),
      ),
    );
    await tester.pumpAndSettle();

    await tester.drag(find.byType(ListView), const Offset(0, 260));
    await tester.pumpAndSettle();
    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-jump',
        messages: _scrollMessages('session-jump', 25),
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
        sessionId: 'session-streaming',
        messages: _scrollMessages('session-streaming', 12),
      ),
    );
    await tester.pumpAndSettle();

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-streaming',
        messages: _scrollMessages('session-streaming', 12, expandedLast: true),
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
        sessionId: 'session-stream-count',
        messages: _scrollMessages('session-stream-count', 24),
      ),
    );
    await tester.pumpAndSettle();
    await tester.drag(find.byType(ListView), const Offset(0, 260));
    await tester.pumpAndSettle();
    expect(
      find.byKey(const ValueKey('timeline-jump-to-latest:0')),
      findsOneWidget,
    );

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-stream-count',
        messages: _scrollMessages(
          'session-stream-count',
          24,
          expandedLast: true,
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      find.byKey(const ValueKey('timeline-jump-to-latest:0')),
      findsOneWidget,
    );

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-stream-count',
        messages: _scrollMessages('session-stream-count', 25),
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
        sessionId: 'session-a',
        messages: _scrollMessages('session-a', 24),
      ),
    );
    await tester.pumpAndSettle();
    await tester.drag(find.byType(ListView), const Offset(0, 260));
    await tester.pumpAndSettle();
    final sessionAOffset = _timelinePixels(tester);
    expect(_timelineExtentAfter(tester), greaterThan(80));

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-b',
        messages: _scrollMessages('session-b', 20),
      ),
    );
    await tester.pumpAndSettle();
    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-a',
        messages: _scrollMessages('session-a', 24),
      ),
    );
    await tester.pumpAndSettle();

    expect(_timelinePixels(tester), closeTo(sessionAOffset, 1));
    expect(find.byTooltip('Jump to latest'), findsOneWidget);
  });

  testWidgets('loading older history preserves the visible timeline anchor', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    const sessionId = 'session-history';
    var loadCount = 0;
    final recentMessages = _scrollMessages(sessionId, 24, startIndex: 8);
    await tester.pumpWidget(
      _timelineHarness(
        sessionId: sessionId,
        messages: recentMessages,
        onLoadOlder: () => loadCount += 1,
      ),
    );
    await tester.pumpAndSettle();

    await tester.drag(find.byType(ListView), const Offset(0, 5000));
    await tester.pumpAndSettle();
    expect(loadCount, 1);
    final anchor = find.textContaining('message 8 for $sessionId');
    final anchorTopBeforeLoad = tester.getTopLeft(anchor).dy;

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: sessionId,
        messages: recentMessages,
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
        sessionId: sessionId,
        messages: [..._scrollMessages(sessionId, 8), ...recentMessages],
        onLoadOlder: () => loadCount += 1,
      ),
    );
    await tester.pumpAndSettle();

    expect(loadCount, 1);
    expect(tester.getTopLeft(anchor).dy, closeTo(anchorTopBeforeLoad, 1));
  });
}
