part of '../widget_test.dart';

void registerMarkdownRenderTests() {
  test(
    'agent markdown repair keeps incomplete streaming blocks provisional',
    () {
      expect(
        repairAgentMarkdownForDisplay('```dart\nvoid main() {}'),
        '```dart\nvoid main() {}',
      );
      expect(
        repairAgentMarkdownForDisplay('| Name | State |\n| ---'),
        '| Name | State |\n| ---',
      );
    },
  );

  test('agent markdown repair recovers agent heading and fence boundaries', () {
    expect(
      repairAgentMarkdownForDisplay(
        '###整体层级```\n'
        '└──<html>\n'
        'CSS组织```\n'
        'body { margin: 0; }',
      ),
      '### 整体层级\n'
      '```\n'
      '└──<html>\n'
      'CSS组织\n'
      '```\n'
      'body { margin: 0; }',
    );
    expect(
      repairAgentMarkdownForDisplay(
        '```text\n'
        'WttrResponse ├ weather: Vec<WeatherDay>```\n\n'
        '## 依赖选型\n\n'
        '| 依赖 | 用途 |\n'
        '| --- | --- |\n'
        '| serde | JSON |',
      ),
      '```text\n'
      'WttrResponse ├ weather: Vec<WeatherDay>\n'
      '```\n\n'
      '## 依赖选型\n\n'
      '| 依赖 | 用途 |\n'
      '| --- | --- |\n'
      '| serde | JSON |',
    );
  });

  testWidgets('timeline renders streaming markdown blocks', (tester) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final items = [
      _threadItemFixture(
        id: 'text-1',
        threadId: 'session-1',
        turnId: 'turn-1',
        ordinal: 0,
        channel: AgentMessageChannel.commentary,
        text:
            '# Build result\n'
            '- **Compile** runtime\n'
            '- Render `timeline`\n\n'
            '| File | State |\n'
            '| --- | --- |\n'
            '| app.dart | ready |\n\n'
            '```dart\n'
            'void main() {\n'
            "  print('ok');\n"
            '}',
        status: 'streaming',
      ),
      _threadItemFixture(
        id: 'reasoning-1',
        threadId: 'session-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.reasoning,
        channel: null,
        reasoningSummary: const ['Reasoning'],
        reasoningContent: const [
          '> hidden raw reasoning\n\n- keep this out of timeline',
        ],
      ),
      _threadItemFixture(
        id: 'plan-1',
        threadId: 'session-1',
        turnId: 'turn-1',
        ordinal: 2,
        kind: ThreadItemKind.plan,
        channel: null,
        text: '## Next steps\n1. Analyze\n2. Ship',
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        locale: const Locale('zh', 'Hans'),
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 820,
            child: TimelineView(
              threadId: 'session-1',
              turn: null,
              rows: timelineRowsFromThreadItems(items),
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    expect(find.textContaining('Build result'), findsOneWidget);
    expect(find.textContaining('Compile'), findsOneWidget);
    expect(find.textContaining('File'), findsOneWidget);
    expect(find.textContaining('app.dart'), findsOneWidget);
    expect(find.textContaining("print('ok')"), findsOneWidget);
    expect(find.text('Reasoning'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('timeline-reasoning-group-details')),
      findsNothing,
    );
    await tester.tap(find.text('Reasoning'));
    await tester.pumpAndSettle();
    expect(
      find.byKey(const ValueKey('timeline-reasoning-group-details')),
      findsOneWidget,
    );
    expect(find.textContaining('Next steps'), findsOneWidget);
    expect(find.textContaining('Analyze'), findsOneWidget);
  });

  testWidgets('timeline renders all agent text without bubble decoration', (
    tester,
  ) async {
    const parts = [
      TimelineEntry(
        id: 'commentary-text',
        groupId: 'message-agent-text',
        type: TimelineEntryType.text,
        textChannel: TimelineTextChannel.commentary,
        text: '过程输出',
      ),
      TimelineEntry(
        id: 'final-text',
        groupId: 'message-agent-text',
        type: TimelineEntryType.text,
        textChannel: TimelineTextChannel.finalAnswer,
        text: '最终输出',
      ),
    ];

    await tester.pumpWidget(
      _localizedApp(
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

    for (final part in parts) {
      final bubble = find.byKey(ValueKey(part.id));
      final decoratedBox = tester.widget<DecoratedBox>(
        find.descendant(of: bubble, matching: find.byType(DecoratedBox)).first,
      );
      final decoration = decoratedBox.decoration as BoxDecoration;
      final padding = tester.widget<Padding>(
        find.descendant(of: bubble, matching: find.byType(Padding)).first,
      );

      expect(decoration.color, Colors.transparent);
      expect(decoration.border, isNull);
      expect(padding.padding, EdgeInsets.zero);
    }
  });

  testWidgets('timeline renders typed commentary Items without filtering', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final items = [
      for (final (index, text) in [
        '已接收请求，正在准备上下文。',
        '上下文已整理，准备调用模型。',
        '模型请求调用 3 个工具。',
        '正在执行工具 `exec`。',
        '工具 `exec` 已完成。',
      ].indexed)
        _threadItemFixture(
          id: 'commentary-$index',
          threadId: 'session-1',
          turnId: 'turn-1',
          ordinal: index,
          channel: AgentMessageChannel.commentary,
          text: text,
        ),
      _threadItemFixture(
        id: 'final-1',
        threadId: 'session-1',
        turnId: 'turn-1',
        ordinal: 5,
        text: '最终答复保持独立。',
      ),
    ];

    await tester.pumpWidget(
      _localizedApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 820,
            child: TimelineView(
              threadId: 'session-1',
              turn: null,
              rows: timelineRowsFromThreadItems(items),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('已接收请求，正在准备上下文。'), findsOneWidget);
    expect(find.text('上下文已整理，准备调用模型。'), findsOneWidget);
    expect(find.text('模型请求调用 3 个工具。'), findsOneWidget);
    expect(find.textContaining('正在执行工具'), findsOneWidget);
    expect(find.textContaining('已完成'), findsOneWidget);
    expect(find.text('最终答复保持独立。'), findsOneWidget);
  });

  testWidgets(
    'timeline keeps reasoning expansion state attached to group identity',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      ThreadItemView reasoningItem({
        required String id,
        required String title,
        required String text,
        required int order,
        String threadId = 'session-1',
      }) {
        return _threadItemFixture(
          id: id,
          threadId: threadId,
          turnId: 'turn-reasoning-identity',
          ordinal: order,
          kind: ThreadItemKind.reasoning,
          channel: null,
          reasoningSummary: [title],
          reasoningContent: [text],
        );
      }

      Widget timelineFor({
        String threadId = 'session-1',
        required List<ThreadItemView> items,
      }) {
        return _timelineApp(
          home: Scaffold(
            body: SizedBox(
              width: 980,
              height: 820,
              child: TimelineView(
                threadId: threadId,
                turn: null,
                rows: timelineRowsFromThreadItems(items),
              ),
            ),
          ),
        );
      }

      await tester.pumpWidget(
        timelineFor(
          items: [
            reasoningItem(
              id: 'reasoning-a',
              title: 'Reasoning A',
              text: 'reasoning-text-a',
              order: 0,
            ),
            reasoningItem(
              id: 'reasoning-b',
              title: 'Reasoning B',
              text: 'reasoning-text-b',
              order: 1,
            ),
          ],
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('Reasoning A · Reasoning B'), findsOneWidget);
      expect(
        find.byKey(const ValueKey('timeline-reasoning-group-details')),
        findsNothing,
      );

      await tester.tap(find.text('Reasoning A · Reasoning B'));
      await tester.pumpAndSettle();

      expect(find.textContaining('reasoning-text-a'), findsOneWidget);
      expect(find.textContaining('reasoning-text-b'), findsOneWidget);

      await tester.pumpWidget(
        timelineFor(
          items: [
            reasoningItem(
              id: 'reasoning-a',
              title: 'Reasoning A',
              text: 'reasoning-text-a',
              order: 0,
            ),
            reasoningItem(
              id: 'reasoning-b',
              title: 'Reasoning B',
              text: 'reasoning-text-b',
              order: 1,
            ),
            reasoningItem(
              id: 'reasoning-c',
              title: 'Reasoning C',
              text: 'reasoning-text-c',
              order: 2,
            ),
          ],
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.text('Reasoning A · Reasoning B · Reasoning C'),
        findsOneWidget,
      );
      expect(find.textContaining('reasoning-text-a'), findsOneWidget);
      expect(find.textContaining('reasoning-text-b'), findsOneWidget);
      expect(find.textContaining('reasoning-text-c'), findsOneWidget);

      await tester.pumpWidget(
        timelineFor(
          threadId: 'session-2',
          items: [
            reasoningItem(
              id: 'reasoning-a',
              title: 'Reasoning A',
              text: 'reasoning-text-a-session-2',
              order: 0,
              threadId: 'session-2',
            ),
          ],
        ),
      );
      await tester.pumpAndSettle();

      expect(find.textContaining('reasoning-text-a-session-2'), findsNothing);
    },
  );

  testWidgets('timeline reasoning row shows active state while streaming', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final part = TimelineEntry(
      id: 'reasoning-active',
      groupId: 'reasoning-active',
      threadId: 'session-1',
      turnId: 'turn-1',
      type: TimelineEntryType.reasoning,
      order: 0,
      revision: 0,
      text: '## 分析调用结果',
      reasoningSummary: const ['## 分析调用结果'],
      reasoningContent: const ['正在分析调用结果。'],
      status: 'streaming',
      createdAt: now,
      updatedAt: now,
    );

    await tester.pumpWidget(
      _timelineApp(
        locale: const Locale('zh', 'Hans'),
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              threadId: 'session-1',
              turn: _testTurn(
                threadId: 'session-1',
                turnId: 'turn-1',
                state: const StudioTurnState.inProgress(
                  StudioTurnActivity.thinking,
                ),
              ),
              rows: timelineRowsFromFixtureParts([part]),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('分析调用结果'), findsOneWidget);
    expect(find.textContaining('正在分析调用结果'), findsNothing);

    await tester.tap(find.text('分析调用结果'));
    await tester.pumpAndSettle();

    expect(find.textContaining('正在分析调用结果'), findsOneWidget);
  });

  testWidgets(
    'timeline shows streaming reasoning content instead of active placeholder',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 700);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final now = DateTime.fromMillisecondsSinceEpoch(0);
      final part = TimelineEntry(
        id: 'reasoning-delta',
        groupId: 'reasoning-delta',
        threadId: 'session-1',
        turnId: 'turn-1',
        type: TimelineEntryType.reasoning,
        order: 0,
        revision: 1,
        text: '正在核对角色设置、状态类型与测试夹具。',
        reasoningSummary: const ['正在核对角色设置、状态类型与测试夹具。'],
        status: 'streaming',
        createdAt: now,
        updatedAt: now,
      );

      await tester.pumpWidget(
        _timelineApp(
          locale: const Locale('zh', 'Hans'),
          home: Scaffold(
            body: SizedBox(
              width: 980,
              height: 520,
              child: TimelineView(
                threadId: 'session-1',
                turn: _testTurn(
                  threadId: 'session-1',
                  turnId: 'turn-1',
                  state: const StudioTurnState.inProgress(
                    StudioTurnActivity.thinking,
                  ),
                ),
                rows: timelineRowsFromFixtureParts([part]),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('正在核对角色设置、状态类型与测试夹具。'), findsOneWidget);
      expect(find.text('思考中'), findsNothing);
    },
  );

  testWidgets(
    'timeline groups consecutive reasoning and refreshes one current summary',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 700);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      const threadId = 'session-1';
      const turnId = 'turn-1';

      ThreadItemView reasoning({
        required String id,
        required int order,
        required String text,
        required String status,
      }) {
        final sections = text.split('\n\n');
        return _threadItemFixture(
          id: id,
          threadId: threadId,
          turnId: turnId,
          ordinal: order,
          status: status,
          kind: ThreadItemKind.reasoning,
          channel: null,
          reasoningSummary: [sections.first],
          reasoningContent: [sections.skip(1).join('\n\n')],
        );
      }

      Widget timelineFor({
        required List<ThreadItemView> items,
        required StudioTurnState? turnState,
      }) {
        return _timelineApp(
          home: Scaffold(
            body: SizedBox(
              width: 980,
              height: 520,
              child: TimelineView(
                threadId: threadId,
                rows: timelineRowsFromThreadItems(items),
                turn: turnState == null
                    ? null
                    : _testTurn(
                        threadId: threadId,
                        turnId: turnId,
                        state: turnState,
                      ),
              ),
            ),
          ),
        );
      }

      final first = reasoning(
        id: 'reasoning-a',
        order: 0,
        text: '## Inspecting files\n\nfirst detail',
        status: 'completed',
      );
      var latest = reasoning(
        id: 'reasoning-b',
        order: 1,
        text: '## Comparing projection\n\nsecond detail',
        status: 'streaming',
      );
      await tester.pumpWidget(
        timelineFor(
          items: [first, latest],
          turnState: const StudioTurnState.inProgress(
            StudioTurnActivity.thinking,
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.byKey(const ValueKey('timeline-current-activity')),
        findsOneWidget,
      );
      expect(find.text('Comparing projection'), findsOneWidget);
      expect(find.textContaining('first detail'), findsNothing);
      expect(find.textContaining('second detail'), findsNothing);

      latest = reasoning(
        id: 'reasoning-b',
        order: 1,
        text: '## Updating projection\n\nsecond detail updated',
        status: 'streaming',
      );
      await tester.pumpWidget(
        timelineFor(
          items: [first, latest],
          turnState: const StudioTurnState.inProgress(
            StudioTurnActivity.thinking,
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.byKey(const ValueKey('timeline-current-activity')),
        findsOneWidget,
      );
      expect(find.text('Updating projection'), findsOneWidget);
      expect(find.text('Comparing projection'), findsNothing);

      final completed = reasoning(
        id: 'reasoning-b',
        order: 1,
        text: '## Updating projection\n\nsecond detail updated',
        status: 'completed',
      );
      await tester.pumpWidget(
        timelineFor(items: [first, completed], turnState: null),
      );
      await tester.pumpAndSettle();

      expect(
        find.byKey(const ValueKey('timeline-current-activity')),
        findsNothing,
      );
      expect(
        find.text('Inspecting files · Updating projection'),
        findsOneWidget,
      );
      await tester.tap(find.text('Inspecting files · Updating projection'));
      await tester.pumpAndSettle();
      expect(find.textContaining('first detail'), findsOneWidget);
      expect(find.textContaining('second detail updated'), findsOneWidget);
    },
  );

  testWidgets('timeline reasoning history summarizes three sections', (
    tester,
  ) async {
    const threadId = 'session-1';
    final items = [
      for (var index = 0; index < 4; index++)
        _threadItemFixture(
          id: 'reasoning-$index',
          threadId: threadId,
          turnId: 'turn-reasoning-summary',
          ordinal: index,
          kind: ThreadItemKind.reasoning,
          channel: null,
          reasoningSummary: ['## Section ${index + 1}'],
        ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              threadId: threadId,
              rows: timelineRowsFromThreadItems(items),
              turn: null,
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Section 1 · Section 2 · Section 3 · +1'), findsOneWidget);
  });
}
