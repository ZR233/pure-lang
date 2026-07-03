part of '../widget_test.dart';

void registerTimelineToolTests() {
  testWidgets('timeline renders todo list update rows', (tester) async {
    tester.view.physicalSize = const Size(900, 620);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final row = TimelineRow.agentActivity(
      TimelineAgentEvent(
        eventId: 'todo-event-1',
        sessionId: 'session-1',
        sequence: 1,
        createdAt: DateTime.fromMillisecondsSinceEpoch(0),
        payload: const TimelineTodoListUpdate(
          callId: 'call-1',
          explanation: 'Implementation checklist',
          items: [
            TimelineTodoItem(
              step: 'Read existing timeline projection',
              status: 'completed',
            ),
            TimelineTodoItem(
              step: 'Wire update_todo_list through bridge',
              status: 'inProgress',
            ),
            TimelineTodoItem(
              step: 'Run the focused Rust and Flutter tests before handoff',
              status: 'pending',
            ),
          ],
        ),
      ),
    );

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 720,
            height: 480,
            child: TimelineView(sessionId: 'session-1', rows: [row]),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('Implementation checklist'), findsOneWidget);
    expect(find.text('Completed'), findsOneWidget);
    expect(find.text('In progress'), findsOneWidget);
    expect(find.text('Pending'), findsOneWidget);
    expect(find.textContaining('update_todo_list'), findsOneWidget);
    expect(find.textContaining('focused Rust and Flutter'), findsOneWidget);
  });

  testWidgets('timeline tool group defaults collapsed and expands details', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final part = timelinePartFromSnapshot(
      TimelinePartSnapshot(
        id: 'tool-part-1',
        messageId: 'message-tool',
        sessionId: 'session-1',
        turnId: 'turn-tool',
        type: TimelinePartType.tool,
        order: 0,
        revision: 0,
        text: '',
        status: 'completed',
        createdAt: DateTime.fromMillisecondsSinceEpoch(0),
        updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
        activityGroupId: 'tool-group:turn-tool:0',
        tool: TimelineToolPart(
          toolCallId: 'tool-call-1',
          name: 'bash',
          arguments: jsonEncode({
            'command': 'cargo test -p pl-model\ncargo test -p pl-core',
          }),
          workingDirectory: 'D:/work/project',
          result: 'ok',
        ),
      ),
    );
    final messages = [
      TimelineMessage(
        id: 'message-tool',
        sessionId: 'session-1',
        role: 'assistant',
        createdAt: DateTime.fromMillisecondsSinceEpoch(0),
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              sessionId: 'session-1',
              rows: timelineRowsFromMessages(messages, parts: [part]),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('Tool activity'), findsOneWidget);
    expect(find.text('1 tools'), findsOneWidget);
    expect(find.textContaining('cargo test -p pl-model'), findsNothing);
    expect(find.textContaining('D:/work/project'), findsNothing);
    expect(find.textContaining('"command"'), findsNothing);

    await tester.tap(find.text('Tool activity'));
    await tester.pump();

    expect(find.textContaining('cargo test -p pl-model'), findsOneWidget);
    expect(find.textContaining('D:/work/project'), findsOneWidget);
    expect(find.textContaining('pl-core'), findsNothing);
  });

  testWidgets('timeline renders separate tool groups around assistant text', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 760);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      turnId: 'turn-1',
      role: 'assistant',
      createdAt: now,
    );
    final parts = [
      TimelinePart(
        id: 'text-before',
        messageId: message.id,
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.text,
        text: '先读取相关文件。',
        textChannel: TimelineTextChannel.commentary,
        order: 0,
      ),
      _toolTimelinePart(
        id: 'tool-a',
        messageId: message.id,
        turnId: 'turn-1',
        order: 1,
        name: 'read_file',
        arguments: jsonEncode({'path': 'lib/a.dart'}),
        activityGroupId: 'tool-group:turn-1:1',
      ),
      _toolTimelinePart(
        id: 'tool-b',
        messageId: message.id,
        turnId: 'turn-1',
        order: 2,
        name: 'search_files',
        arguments: jsonEncode({'query': 'activityGroupId'}),
        activityGroupId: 'tool-group:turn-1:1',
      ),
      TimelinePart(
        id: 'text-middle',
        messageId: message.id,
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.text,
        text: '再跑一下测试。',
        textChannel: TimelineTextChannel.commentary,
        order: 3,
      ),
      _toolTimelinePart(
        id: 'tool-c',
        messageId: message.id,
        turnId: 'turn-1',
        order: 4,
        name: 'bash',
        arguments: jsonEncode({'command': 'flutter test'}),
        activityGroupId: 'tool-group:turn-1:4',
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 620,
            child: TimelineView(
              sessionId: 'session-1',
              rows: timelineRowsFromMessages([message], parts: parts),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('Tool activity'), findsNWidgets(2));
    expect(find.text('先读取相关文件。'), findsOneWidget);
    expect(find.text('再跑一下测试。'), findsOneWidget);
    expect(find.textContaining('lib/a.dart'), findsNothing);
    expect(find.textContaining('flutter test'), findsNothing);

    await tester.tap(find.text('Tool activity').first);
    await tester.pump();

    expect(find.text('read_file completed'), findsOneWidget);
    expect(find.text('search_files completed'), findsOneWidget);
    expect(find.text('bash completed'), findsNothing);
    expect(find.textContaining('lib/a.dart'), findsOneWidget);
    expect(find.textContaining('activityGroupId'), findsOneWidget);
    expect(find.textContaining('flutter test'), findsNothing);

    await tester.tap(find.text('Tool activity').last);
    await tester.pump();

    expect(find.text('bash completed'), findsOneWidget);
    expect(find.textContaining('flutter test'), findsOneWidget);
  });

  testWidgets('timeline tool group summarizes running and issue states', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final message = TimelineMessage(
      id: 'message-tool',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(0),
    );
    final parts = [
      _toolTimelinePart(
        id: 'tool-awaiting',
        messageId: message.id,
        turnId: 'turn-tool',
        status: 'awaitingApproval',
        name: 'bash',
        arguments: jsonEncode({'command': 'cargo test -p pl-core'}),
        activityGroupId: 'tool-group:turn-tool:0',
      ),
      _toolTimelinePart(
        id: 'tool-failed',
        messageId: message.id,
        turnId: 'turn-tool',
        order: 1,
        status: 'failed',
        name: 'read_file',
        arguments: jsonEncode({'path': 'lib/main.dart'}),
        result: 'file missing',
        exitCode: 2,
        activityGroupId: 'tool-group:turn-tool:0',
      ),
      _toolTimelinePart(
        id: 'tool-running',
        messageId: message.id,
        turnId: 'turn-tool',
        order: 2,
        status: 'running',
        name: 'search_files',
        arguments: jsonEncode({'query': 'TimelineToolGroup'}),
        activityGroupId: 'tool-group:turn-tool:0',
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              sessionId: 'session-1',
              rows: timelineRowsFromMessages([message], parts: parts),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('Tool activity'), findsOneWidget);
    expect(find.text('3 tools, 1 running, 1 need attention'), findsOneWidget);
    expect(find.text('awaitingApproval'), findsOneWidget);

    await tester.tap(find.text('Tool activity'));
    await tester.pump();

    expect(find.text('bash awaiting approval'), findsOneWidget);
    expect(find.text('read_file failed'), findsOneWidget);
    expect(find.textContaining('cargo test -p pl-core'), findsOneWidget);
    expect(find.textContaining('lib/main.dart'), findsOneWidget);
    expect(find.textContaining('exit code 2'), findsOneWidget);
    expect(find.textContaining('file missing'), findsOneWidget);
  });

  testWidgets('timeline renders markdown after inline code fence closure', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'message-inline-fence',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: now,
    );
    const parts = [
      TimelinePart(
        id: 'plan-inline-fence',
        messageId: 'message-inline-fence',
        type: TimelinePartType.plan,
        title: 'Plan',
        text:
            '```text\n'
            'WttrResponse ├ weather: Vec<WeatherDay>```\n\n'
            '## 依赖选型\n\n'
            '| 依赖 | 用途 |\n'
            '| --- | --- |\n'
            '| serde | JSON |',
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 820,
            child: TimelineView(
              sessionId: 'session-1',
              rows: timelineRowsFromMessages([message], parts: parts),
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    expect(find.textContaining('依赖选型'), findsOneWidget);
    expect(find.textContaining('serde'), findsOneWidget);
    expect(find.textContaining('JSON'), findsOneWidget);
    expect(find.textContaining('## 依赖选型'), findsNothing);
    expect(find.textContaining('| serde | JSON |'), findsNothing);
  });

  testWidgets('timeline renders inline code and quotes with studio chrome', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'message-markdown-chrome',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: now,
    );
    const parts = [
      TimelinePart(
        id: 'text-markdown-chrome',
        messageId: 'message-markdown-chrome',
        type: TimelinePartType.text,
        text:
            '项目使用 `std::env::args()` 读取参数。\n\n'
            '> 这是一段引用\n'
            '> 包含 `inline` 代码',
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 820,
            child: TimelineView(
              sessionId: 'session-1',
              rows: timelineRowsFromMessages([message], parts: parts),
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    final inlineCode = find.text('std::env::args()');
    expect(inlineCode, findsOneWidget);
    expect(
      find.ancestor(
        of: inlineCode,
        matching: find.byKey(const ValueKey('studio-markdown-inline-code')),
      ),
      findsOneWidget,
    );
    expect(find.byKey(const ValueKey('studio-markdown-quote')), findsOneWidget);
    expect(find.textContaining('这是一段引用'), findsOneWidget);
    expect(find.textContaining('> 这是一段引用'), findsNothing);
  });

  testWidgets('timeline renders agent markdown with tight CJK headings', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'message-agent-markdown',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: now,
    );
    const parts = [
      TimelinePart(
        id: 'plan-agent-markdown',
        messageId: 'message-agent-markdown',
        type: TimelinePartType.plan,
        title: 'Plan',
        text:
            'glm-intro.html代码结构单文件 HTML（~850行），GLM产品介绍落地页。\n\n'
            '###整体层级```\n'
            '└──<html>\n'
            '├──<head>\n'
            '│ └──<style> → 全部 CSS\n'
            'CSS组织```\n'
            'hero { display: grid; }\n\n'
            '###实现计划\n'
            '- 拆分结构\n'
            '- 保持动效',
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 820,
            child: TimelineView(
              sessionId: 'session-1',
              rows: timelineRowsFromMessages([message], parts: parts),
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    expect(find.textContaining('整体层级'), findsOneWidget);
    expect(find.textContaining('实现计划'), findsOneWidget);
    expect(find.textContaining('└──<html>'), findsOneWidget);
    expect(find.textContaining('###整体层级```'), findsNothing);
    expect(find.textContaining('CSS组织```'), findsNothing);
  });
}
