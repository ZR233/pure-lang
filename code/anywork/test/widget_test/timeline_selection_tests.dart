part of '../widget_test.dart';

void registerTimelineSelectionTests() {
  Future<String?> dragSelectAndCopy(
    WidgetTester tester, {
    required Offset start,
    required Offset end,
  }) async {
    final clipboardCalls = <Map<dynamic, dynamic>>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, (call) async {
          if (call.method == 'Clipboard.setData') {
            clipboardCalls.add(call.arguments as Map);
          }
          return null;
        });

    final gesture = await tester.startGesture(
      start,
      kind: PointerDeviceKind.mouse,
    );
    await gesture.moveBy(end - start);
    await tester.pump();
    await gesture.up();
    await tester.pump();

    final secondary = await tester.startGesture(
      end,
      kind: PointerDeviceKind.mouse,
      buttons: kSecondaryButton,
    );
    await secondary.up();
    await tester.pump();

    expect(find.byType(AdaptiveTextSelectionToolbar), findsOneWidget);
    await tester.tap(find.text('Copy'));
    await tester.pump();

    return clipboardCalls.isEmpty
        ? null
        : clipboardCalls.single['text'] as String?;
  }

  testWidgets('timeline text selection copies across messages', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;

    const parts = [
      TimelineEntry(
        id: 'text-selection-user',
        groupId: 'message-selection',
        type: TimelineEntryType.text,
        textChannel: TimelineTextChannel.user,
        text: 'Alpha bravo charlie delta echo foxtrot.',
      ),
      TimelineEntry(
        id: 'text-selection-agent',
        groupId: 'message-selection',
        type: TimelineEntryType.text,
        text: 'Golf hotel india juliet kilo lima mike.',
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: TimelineView(
            threadId: 'session-1',
            turn: null,
            rows: timelineRowsFromFixtureParts(parts),
          ),
        ),
      ),
    );
    await tester.pump();

    final copied = await dragSelectAndCopy(
      tester,
      start:
          tester.getTopLeft(find.textContaining('Alpha bravo').hitTestable()) +
          const Offset(8, 8),
      end: tester.getCenter(find.textContaining('Golf hotel').hitTestable()),
    );

    expect(copied, isNotNull);
    expect(copied!, contains('bravo charlie delta'));
    expect(copied, contains('Golf hotel india'));

    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('timeline context menu offers select all without a selection', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;

    const parts = [
      TimelineEntry(
        id: 'text-selection-agent',
        groupId: 'message-selection',
        type: TimelineEntryType.text,
        text: 'Golf hotel india juliet kilo lima mike.',
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: TimelineView(
            threadId: 'session-1',
            turn: null,
            rows: timelineRowsFromFixtureParts(parts),
          ),
        ),
      ),
    );
    await tester.pump();

    final center = tester.getCenter(
      find.textContaining('Golf hotel').hitTestable(),
    );
    final secondary = await tester.startGesture(
      center,
      kind: PointerDeviceKind.mouse,
      buttons: kSecondaryButton,
    );
    await secondary.up();
    await tester.pump();

    expect(find.byType(AdaptiveTextSelectionToolbar), findsOneWidget);
    expect(find.text('Select all'), findsOneWidget);

    debugDefaultTargetPlatformOverride = null;
  });
}
