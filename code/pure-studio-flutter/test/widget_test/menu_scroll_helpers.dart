part of '../widget_test.dart';

Future<void> _expectMenuOpensAboveTrigger({
  required WidgetTester tester,
  required String triggerTooltip,
  required String menuText,
}) async {
  final trigger = find.byTooltip(triggerTooltip);
  expect(trigger, findsOneWidget);
  final triggerRect = tester.getRect(trigger);

  await tester.tap(trigger);
  await tester.pumpAndSettle();

  final menuItem = find.text(menuText).last;
  expect(menuItem, findsOneWidget);
  final menuItemRect = tester.getRect(menuItem);
  expect(menuItemRect.bottom, lessThanOrEqualTo(triggerRect.top - 4));

  await tester.tapAt(const Offset(4, 4));
  await tester.pumpAndSettle();
}

List<_ProjectedMessageFixture> _scrollMessages(
  String sessionId,
  int count, {
  bool expandedLast = false,
}) {
  final now = DateTime.fromMillisecondsSinceEpoch(0);
  return [
    for (var index = 0; index < count; index++)
      _ProjectedMessageFixture(
        message: TimelineMessage(
          id: '$sessionId-message-$index',
          sessionId: sessionId,
          role: index.isEven ? 'assistant' : 'user',
          createdAt: now.add(Duration(seconds: index)),
        ),
        parts: [
          TimelinePart(
            id: '$sessionId-part-$index',
            messageId: '$sessionId-message-$index',
            type: TimelinePartType.text,
            status: index == count - 1 && expandedLast
                ? 'streaming'
                : 'completed',
            text:
                'message $index for $sessionId\n\n'
                '${expandedLast && index == count - 1 ? _streamingGrowthText : _singleBlockText}',
          ),
        ],
      ),
  ];
}

const _singleBlockText =
    'This timeline row has enough text to create a '
    'stable scroll extent without depending on exact font metrics.';

const _streamingGrowthText = '''
streaming line 1
streaming line 2
streaming line 3
streaming line 4
streaming line 5
streaming line 6
streaming line 7
streaming line 8
streaming line 9
streaming line 10
streaming line 11
streaming line 12
streaming line 13
streaming line 14
''';

ScrollPosition _timelinePosition(WidgetTester tester) {
  final listView = tester.widget<ListView>(
    find.byKey(const ValueKey('timeline-scrollable')),
  );
  return listView.controller!.position;
}

double _timelineExtentAfter(WidgetTester tester) {
  return _timelinePosition(tester).extentAfter;
}

double _timelinePixels(WidgetTester tester) {
  return _timelinePosition(tester).pixels;
}
