part of '../widget_test.dart';

void registerTimelineScrollTests() {
  testWidgets(
    'Plan tail uses natural height when summary wraps at flex boundary',
    (tester) async {
      _configureResponsiveView(tester, const Size(980, 520));
      for (var length = 35; length <= 125; length += 5) {
        final plan = PlanConfirmationView(
          interaction: _planConfirmationInteraction(),
          question: UserQuestionView(
            id: agentSessionPlanConfirmationQuestionId,
            header: 'Plan',
            question: '# Title\n${List.filled(length, 'a').join()}',
            isOther: true,
            isSecret: false,
            options: const [],
          ),
        );
        await tester.pumpWidget(
          _timelineApp(
            home: Scaffold(
              body: Align(
                alignment: Alignment.topLeft,
                child: SizedBox(
                  width: 812,
                  height: 107,
                  child: TimelineView(
                    threadId: 'session-1',
                    rows: const [],
                    turn: null,
                    planConfirmation: plan,
                    onPlanToggle: () {},
                  ),
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(
          tester.takeException(),
          isNull,
          reason: 'summary length $length',
        );
      }
    },
  );

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
      const Offset(0, 900),
    );
    await tester.pumpAndSettle();
    final anchorBeforeAppend = _visibleTimelineAnchor(tester);
    expect(_timelineExtentAfter(tester), greaterThan(80));

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-detached',
        items: _scrollItems('session-detached', 25),
      ),
    );
    await tester.pumpAndSettle();

    expect(
      tester.getTopLeft(find.byKey(anchorBeforeAppend.key)).dy,
      closeTo(anchorBeforeAppend.top, 1),
    );
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
      const Offset(0, 900),
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
      const Offset(0, 900),
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
      const Offset(0, 900),
    );
    await tester.pumpAndSettle();
    final sessionAAnchor = _visibleTimelineAnchor(tester);
    expect(_timelineExtentAfter(tester), greaterThan(80));

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-b',
        items: _scrollItems('session-b', 20),
      ),
    );
    await tester.pumpAndSettle();
    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));

    tester.view.physicalSize = const Size(700, 520);
    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'session-a',
        items: _scrollItems('session-a', 24),
      ),
    );
    await tester.pumpAndSettle();

    expect(
      tester.getTopLeft(find.byKey(sessionAAnchor.key)).dy,
      closeTo(sessionAAnchor.top, 1),
    );
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

  testWidgets(
    'fast page completion permits the next prefetch without a loading frame',
    (tester) async {
      _configureResponsiveView(tester, const Size(980, 520));
      var count = 0;
      var start = 40;
      var items = _scrollItems('fast-history', 20, startIndex: start);
      await tester.pumpWidget(
        _timelineApp(
          home: Scaffold(
            body: StatefulBuilder(
              builder: (context, update) => TimelineView(
                threadId: 'fast-history',
                rows: timelineRowsFromThreadItems(items),
                turn: null,
                onLoadOlder: () => update(() {
                  count++;
                  start -= 20;
                  items = [
                    ..._scrollItems('fast-history', 20, startIndex: start),
                    ...items,
                  ];
                }),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.drag(
        find.byKey(StudioDriverKeys.timeline),
        const Offset(0, 9000),
      );
      await tester.pumpAndSettle();
      expect(count, 1);
      await tester.drag(
        find.byKey(StudioDriverKeys.timeline),
        const Offset(0, 9000),
      );
      await tester.pumpAndSettle();
      expect(count, 2);
    },
  );

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
      findsNothing,
    );
    await tester.pump(const Duration(milliseconds: 151));
    expect(
      find.byKey(const ValueKey('timeline-history-loading')),
      findsOneWidget,
    );

    await tester.pumpWidget(
      _timelineHarness(
        threadId: threadId,
        items: [
          ..._scrollItems(threadId, 8),
          ...recentItems,
          ..._scrollItems(threadId, 1, startIndex: 32, expandedLast: true),
        ],
        onLoadOlder: () => loadCount += 1,
      ),
    );
    await tester.pumpAndSettle();

    expect(loadCount, 1);
    expect(tester.getTopLeft(anchor).dy, closeTo(anchorTopBeforeLoad, 1));
  });

  testWidgets(
    'expanding and collapsing an inline tool image keeps the reading anchor',
    (tester) async {
      _configureResponsiveView(tester, const Size(980, 520));
      const threadId = 'session-image-anchor';
      const attachment = ThreadAttachmentView(
        id: 'anchor-image',
        modality: AttachmentModalityView.image,
        mediaType: 'image/png',
        filename: 'anchor.png',
        byteSize: 68,
        width: 320,
        height: 240,
      );
      final imageItem = _threadItemFixture(
        id: '$threadId-image-row',
        threadId: threadId,
        turnId: '$threadId-turn',
        ordinal: 0,
        kind: ThreadItemKind.toolCall,
        status: 'succeeded',
        tool: const TimelineToolPart(
          toolCallId: 'anchor-image-call',
          callId: 'anchor-image-call',
          name: 'view_image',
          attachments: [attachment],
        ),
      );
      final api = _FakeStudioApi(_emptyState())
        ..threadAttachmentBytes[(
          threadId: threadId,
          attachmentId: 'anchor-image',
        )] = base64Decode(
          'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
        );

      await tester.pumpWidget(
        _timelineHarness(
          threadId: threadId,
          items: [imageItem, ..._scrollItems(threadId, 16, startIndex: 1)],
          api: api,
        ),
      );
      await tester.pumpAndSettle();

      // 离开末尾、回读最旧条目：图片行成为视口顶部的可见阅读锚点。
      await tester.drag(
        find.byKey(StudioDriverKeys.timeline),
        const Offset(0, 2000),
      );
      await tester.pumpAndSettle();

      // 工具调用投影为 tool-group 行，行 id 为 `tool-group:<turnId>:<itemId>`。
      final imageRowId = 'tool-group:${imageItem.turnId}:${imageItem.id}';
      final imageBlock = StudioDriverKeys.timelineBlock(imageRowId);
      expect(find.byKey(imageBlock), findsOneWidget);
      final anchorBefore = _visibleTimelineAnchor(tester);
      expect(anchorBefore.key, imageBlock);
      final anchorHeightBefore = tester.getSize(find.byKey(imageBlock)).height;

      final entryId = StudioDriverKeys.toolImageEntryId(
        'anchor-image-call',
        'anchor-image',
      );

      // 第一次展开：行内新增图片高度，可见锚点行自身位置必须保持。
      await tester.tap(find.byKey(StudioDriverKeys.viewImageToggle(entryId)));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryId)),
        findsOneWidget,
      );
      expect(
        tester.getSize(find.byKey(imageBlock)).height,
        greaterThan(anchorHeightBefore),
      );
      expect(
        tester.getTopLeft(find.byKey(anchorBefore.key)).dy,
        closeTo(anchorBefore.top, 1),
      );

      // 收起回退高度同样不移动可见锚点，阅读位置保持稳定。
      await tester.tap(find.byKey(StudioDriverKeys.viewImageToggle(entryId)));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryId)),
        findsNothing,
      );
      expect(
        tester.getSize(find.byKey(imageBlock)).height,
        closeTo(anchorHeightBefore, 1),
      );
      expect(
        tester.getTopLeft(find.byKey(anchorBefore.key)).dy,
        closeTo(anchorBefore.top, 1),
      );
    },
  );
}

({Key key, double top}) _visibleTimelineAnchor(WidgetTester tester) {
  final viewport = tester.getRect(find.byKey(StudioDriverKeys.timeline));
  final visible =
      find
          .byWidgetPredicate(
            (widget) =>
                widget.key is ValueKey<String> &&
                (widget.key! as ValueKey<String>).value.startsWith(
                  'timeline-block-',
                ),
          )
          .evaluate()
          .where((element) {
            final rect = tester.getRect(find.byKey(element.widget.key!));
            return rect.bottom > viewport.top && rect.top < viewport.bottom;
          })
          .toList()
        ..sort(
          (left, right) => tester
              .getTopLeft(find.byKey(left.widget.key!))
              .dy
              .compareTo(tester.getTopLeft(find.byKey(right.widget.key!)).dy),
        );
  final key = visible.first.widget.key!;
  return (key: key, top: tester.getTopLeft(find.byKey(key)).dy);
}
