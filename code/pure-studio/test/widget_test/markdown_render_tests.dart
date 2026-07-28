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

  test('timeline uses gpt markdown directly without renderer facade', () {
    final timelineSource = File(
      'lib/src/features/timeline/timeline_view.dart',
    ).readAsStringSync();
    final facadeFile = File(
      'lib/src/features/timeline/streaming_markdown.dart',
    );

    expect(timelineSource, contains("package:gpt_markdown/gpt_markdown.dart"));
    expect(timelineSource, isNot(contains("import 'streaming_markdown.dart'")));
    expect(timelineSource, isNot(contains('AgentMarkdown(')));
    expect(facadeFile.existsSync(), isFalse);
  });

  testWidgets('timeline renders streaming markdown blocks', (tester) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'message-1',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: now,
    );
    const parts = [
      TimelinePart(
        id: 'text-1',
        messageId: 'message-1',
        type: TimelinePartType.text,
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
      TimelinePart(
        id: 'reasoning-1',
        messageId: 'message-1',
        type: TimelinePartType.reasoning,
        title: 'Reasoning',
        text: '> hidden raw reasoning\n\n- keep this out of timeline',
        collapsed: true,
      ),
      TimelinePart(
        id: 'plan-1',
        messageId: 'message-1',
        type: TimelinePartType.plan,
        title: 'Plan',
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
              sessionId: 'session-1',
              turnPhase: TurnPhase.completed,
              rows: timelineRowsFromMessages([message], parts: parts),
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
    final message = TimelineMessage(
      id: 'message-agent-text',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(0),
    );
    const parts = [
      TimelinePart(
        id: 'commentary-text',
        messageId: 'message-agent-text',
        type: TimelinePartType.text,
        textChannel: TimelineTextChannel.commentary,
        text: '过程输出',
      ),
      TimelinePart(
        id: 'final-text',
        messageId: 'message-agent-text',
        type: TimelinePartType.text,
        textChannel: TimelineTextChannel.finalAnswer,
        text: '最终输出',
      ),
    ];

    await tester.pumpWidget(
      _localizedApp(
        home: Scaffold(
          body: TimelineView(
            sessionId: 'session-1',
            turnPhase: TurnPhase.completed,
            rows: timelineRowsFromMessages([message], parts: parts),
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

  testWidgets('timeline groups consecutive synthetic commentary rows', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'message-progress',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: now,
    );
    const parts = [
      TimelinePart(
        id: 'progress-1',
        messageId: 'message-progress',
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.text,
        order: 0,
        textChannel: TimelineTextChannel.commentary,
        text: '已接收请求，正在准备上下文。',
        synthetic: true,
      ),
      TimelinePart(
        id: 'progress-2',
        messageId: 'message-progress',
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.text,
        order: 1,
        textChannel: TimelineTextChannel.commentary,
        text: '上下文已整理，准备调用模型。',
        synthetic: true,
      ),
      TimelinePart(
        id: 'tool-progress-1',
        messageId: 'message-progress',
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.text,
        order: 2,
        textChannel: TimelineTextChannel.commentary,
        text: '模型请求调用 3 个工具。',
        synthetic: true,
      ),
      TimelinePart(
        id: 'tool-progress-2',
        messageId: 'message-progress',
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.text,
        order: 3,
        textChannel: TimelineTextChannel.commentary,
        text: '正在执行工具 `exec`。',
        synthetic: true,
      ),
      TimelinePart(
        id: 'tool-progress-3',
        messageId: 'message-progress',
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.text,
        order: 4,
        textChannel: TimelineTextChannel.commentary,
        text: '工具 `exec` 已完成。',
        synthetic: true,
      ),
      TimelinePart(
        id: 'final-1',
        messageId: 'message-progress',
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.text,
        order: 5,
        textChannel: TimelineTextChannel.finalAnswer,
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
              sessionId: 'session-1',
              turnPhase: TurnPhase.completed,
              rows: timelineRowsFromMessages([message], parts: parts),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('上下文已整理，准备调用模型。'), findsOneWidget);
    expect(find.text('已接收请求，正在准备上下文。'), findsNothing);
    expect(find.text('模型请求调用 3 个工具。'), findsNothing);
    expect(find.text('正在执行工具 `exec`。'), findsNothing);
    expect(find.text('工具 `exec` 已完成。'), findsNothing);
    expect(find.text('最终答复保持独立。'), findsOneWidget);

    await tester.tap(find.text('上下文已整理，准备调用模型。'));
    await tester.pump();

    expect(find.text('已接收请求，正在准备上下文。'), findsOneWidget);
    expect(find.text('模型请求调用 3 个工具。'), findsNothing);
    expect(find.text('正在执行工具 `exec`。'), findsNothing);
    expect(find.text('工具 `exec` 已完成。'), findsNothing);
  });

  testWidgets(
    'timeline keeps reasoning expansion state attached to group identity',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final now = DateTime.fromMillisecondsSinceEpoch(0);
      final message = TimelineMessage(
        id: 'message-reasoning-identity',
        sessionId: 'session-1',
        role: 'assistant',
        createdAt: now,
      );

      TimelinePart reasoningPart({
        required String id,
        required String title,
        required String text,
        required int order,
      }) {
        return TimelinePart(
          id: id,
          messageId: message.id,
          type: TimelinePartType.reasoning,
          order: order,
          title: title,
          status: 'completed',
          text: text,
          collapsed: true,
        );
      }

      Widget timelineFor({
        String sessionId = 'session-1',
        required List<TimelinePart> parts,
      }) {
        return _timelineApp(
          home: Scaffold(
            body: SizedBox(
              width: 980,
              height: 820,
              child: TimelineView(
                sessionId: sessionId,
                turnPhase: TurnPhase.completed,
                rows: timelineRowsFromMessages([
                  TimelineMessage(
                    id: message.id,
                    sessionId: sessionId,
                    role: message.role,
                    createdAt: message.createdAt,
                  ),
                ], parts: parts),
              ),
            ),
          ),
        );
      }

      await tester.pumpWidget(
        timelineFor(
          parts: [
            reasoningPart(
              id: 'reasoning-a',
              title: 'Reasoning A',
              text: 'reasoning-text-a',
              order: 0,
            ),
            reasoningPart(
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
          parts: [
            reasoningPart(
              id: 'reasoning-a',
              title: 'Reasoning A',
              text: 'reasoning-text-a',
              order: 0,
            ),
            reasoningPart(
              id: 'reasoning-b',
              title: 'Reasoning B',
              text: 'reasoning-text-b',
              order: 1,
            ),
            reasoningPart(
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
          sessionId: 'session-2',
          parts: [
            reasoningPart(
              id: 'reasoning-a',
              title: 'Reasoning A',
              text: 'reasoning-text-a-session-2',
              order: 0,
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
    final message = TimelineMessage(
      id: 'message-reasoning-active',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: now,
    );
    final part = timelinePartFromSnapshot(
      TimelinePartSnapshot(
        id: 'reasoning-active',
        messageId: message.id,
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.reasoning,
        order: 0,
        revision: 0,
        text: '## 分析调用结果\n\n正在分析调用结果。',
        status: 'streaming',
        createdAt: now,
        updatedAt: now,
      ),
    );

    await tester.pumpWidget(
      _timelineApp(
        locale: const Locale('zh', 'Hans'),
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              sessionId: 'session-1',
              turnPhase: TurnPhase.streaming,
              rows: timelineRowsFromMessages([message], parts: [part]),
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
      final message = TimelineMessage(
        id: 'message-reasoning-delta',
        sessionId: 'session-1',
        role: 'assistant',
        createdAt: now,
      );
      final part = timelinePartFromSnapshot(
        TimelinePartSnapshot(
          id: 'reasoning-delta',
          messageId: message.id,
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.reasoning,
          order: 0,
          revision: 0,
          text: '',
          status: 'streaming',
          createdAt: now,
          updatedAt: now,
        ),
        overlay: const TimelinePartOverlay(
          values: {'reasoning.summary': '正在核对角色设置、状态类型与测试夹具。'},
          lastRevisions: {'reasoning.summary': 1},
        ),
      );

      await tester.pumpWidget(
        _timelineApp(
          locale: const Locale('zh', 'Hans'),
          home: Scaffold(
            body: SizedBox(
              width: 980,
              height: 520,
              child: TimelineView(
                sessionId: 'session-1',
                turnPhase: TurnPhase.streaming,
                rows: timelineRowsFromMessages([message], parts: [part]),
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

      final message = TimelineMessage(
        id: 'message-reasoning-stream',
        sessionId: 'session-1',
        turnId: 'turn-1',
        role: 'assistant',
        createdAt: DateTime.fromMillisecondsSinceEpoch(0),
      );

      TimelinePart reasoning({
        required String id,
        required int order,
        required String text,
        required String status,
      }) {
        return TimelinePart(
          id: id,
          messageId: message.id,
          sessionId: message.sessionId,
          turnId: message.turnId,
          type: TimelinePartType.reasoning,
          order: order,
          text: text,
          status: status,
        );
      }

      Widget timelineFor({
        required List<TimelinePart> parts,
        required TurnPhase phase,
      }) {
        return _timelineApp(
          home: Scaffold(
            body: SizedBox(
              width: 980,
              height: 520,
              child: TimelineView(
                sessionId: message.sessionId,
                rows: timelineRowsFromMessages([message], parts: parts),
                turnPhase: phase,
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
        timelineFor(parts: [first, latest], phase: TurnPhase.streaming),
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
        timelineFor(parts: [first, latest], phase: TurnPhase.streaming),
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
        timelineFor(parts: [first, completed], phase: TurnPhase.completed),
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
    final message = TimelineMessage(
      id: 'message-reasoning-summary',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(0),
    );
    final parts = [
      for (var index = 0; index < 4; index++)
        TimelinePart(
          id: 'reasoning-$index',
          messageId: message.id,
          sessionId: message.sessionId,
          type: TimelinePartType.reasoning,
          order: index,
          text: '## Section ${index + 1}',
        ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              sessionId: message.sessionId,
              rows: timelineRowsFromMessages([message], parts: parts),
              turnPhase: TurnPhase.completed,
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Section 1 · Section 2 · Section 3 · +1'), findsOneWidget);
  });
}
