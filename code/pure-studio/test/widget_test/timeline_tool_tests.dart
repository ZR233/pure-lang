part of '../widget_test.dart';

void registerTimelineToolTests() {
  testWidgets('timeline renders dedicated web search action and result links', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(900, 620);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final message = TimelineMessage(
      id: 'message-web-search',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(0),
    );
    final part = _toolTimelinePart(
      id: 'web-search-1',
      messageId: message.id,
      turnId: 'turn-web-search',
      name: 'web_search',
      status: 'streaming',
      arguments: jsonEncode({
        'type': 'find_in_page',
        'url': 'https://example.com/page',
        'pattern': 'needle',
      }),
      outputArtifacts: const [
        {
          'kind': 'webSearch',
          'results': [
            {
              'url': 'https://example.com/result',
              'unknownFutureField': {'rank': 1},
            },
          ],
        },
      ],
    );

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 720,
            height: 480,
            child: TimelineView(
              sessionId: 'session-1',
              turnPhase: TurnPhase.completed,
              rows: timelineRowsFromMessages([message], parts: [part]),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('web_search running'), findsOneWidget);
    expect(find.text('running'), findsNothing);
    expect(find.text('Finding text on a page'), findsNothing);
    expect(find.textContaining('https://example.com/page'), findsNothing);
    expect(find.text('Result links'), findsNothing);
    expect(find.text('Tool activity'), findsNothing);

    await tester.tap(find.text('web_search running'));
    await tester.pump();

    expect(find.text('Finding text on a page'), findsOneWidget);
    expect(find.textContaining('https://example.com/page'), findsOneWidget);
    expect(find.textContaining('needle'), findsOneWidget);
    expect(find.text('Result links'), findsOneWidget);
    expect(find.text('https://example.com/result'), findsOneWidget);
  });

  testWidgets('todo panel renders the latest flat checklist', (tester) async {
    tester.view.physicalSize = const Size(900, 620);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    const todo = TimelineTodoListUpdate(
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
    );

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 304,
            height: 480,
            child: const SessionTodoPanel(todo: todo),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('Implementation checklist'), findsOneWidget);
    expect(find.text('Read existing timeline projection'), findsOneWidget);
    expect(find.text('Wire update_todo_list through bridge'), findsOneWidget);
    expect(
      find.text('Run the focused Rust and Flutter tests before handoff'),
      findsOneWidget,
    );
    expect(find.byIcon(Icons.check_circle_outline), findsOneWidget);
    expect(find.byIcon(Icons.radio_button_checked), findsOneWidget);
    expect(find.byIcon(Icons.radio_button_unchecked), findsOneWidget);
    expect(find.textContaining('focused Rust and Flutter'), findsOneWidget);
    expect(tester.takeException(), isNull);
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
          name: 'exec',
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
              turnPhase: TurnPhase.completed,
              rows: timelineRowsFromMessages(messages, parts: [part]),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('exec completed'), findsOneWidget);
    final summary = find.byKey(const ValueKey('timeline-tool-group-summary'));
    expect(tester.widget<Material>(summary).color, Colors.transparent);
    expect(tester.getSize(summary).height, greaterThanOrEqualTo(32));
    final summarySemantics = find.bySemanticsLabel('exec completed');
    expect(summarySemantics, findsOneWidget);
    var semanticsData = tester
        .getSemantics(summarySemantics)
        .getSemanticsData();
    expect(semanticsData.flagsCollection.isButton, isTrue);
    expect(semanticsData.flagsCollection.isExpanded, Tristate.isFalse);
    expect(semanticsData.hasAction(SemanticsAction.tap), isTrue);
    expect(
      find.byKey(const ValueKey('timeline-tool-group-details')),
      findsNothing,
    );
    expect(find.textContaining('cargo test -p pl-model'), findsNothing);
    expect(find.textContaining('D:/work/project'), findsNothing);
    expect(find.textContaining('"command"'), findsNothing);

    await tester.tap(find.text('exec completed'));
    await tester.pump();

    expect(
      find.byKey(const ValueKey('timeline-tool-group-details')),
      findsOneWidget,
    );
    semanticsData = tester.getSemantics(summarySemantics).getSemanticsData();
    expect(semanticsData.flagsCollection.isExpanded, Tristate.isTrue);
    expect(find.textContaining('cargo test -p pl-model'), findsOneWidget);
    expect(find.textContaining('D:/work/project'), findsOneWidget);
    expect(find.textContaining('pl-core'), findsNothing);

    await tester.tap(find.text('exec completed').first);
    await tester.pump();

    expect(
      find.byKey(const ValueKey('timeline-tool-group-details')),
      findsNothing,
    );
    expect(find.textContaining('cargo test -p pl-model'), findsNothing);
  });

  testWidgets('timeline merges adjacent mixed tool types in order', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final message = TimelineMessage(
      id: 'message-mixed-tools',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(0),
    );
    final parts = [
      _toolTimelinePart(
        id: 'tool-edit',
        messageId: message.id,
        turnId: 'turn-mixed-tools',
        name: 'edit_file',
        arguments: jsonEncode({'path': 'lib/timeline.dart'}),
      ),
      _toolTimelinePart(
        id: 'tool-read',
        messageId: message.id,
        turnId: 'turn-mixed-tools',
        order: 1,
        name: 'read_file',
        arguments: jsonEncode({'path': 'test/timeline_test.dart'}),
      ),
      _toolTimelinePart(
        id: 'tool-exec',
        messageId: message.id,
        turnId: 'turn-mixed-tools',
        order: 2,
        name: 'exec',
        arguments: jsonEncode({'command': 'flutter test'}),
        workingDirectory: 'code/pure-studio',
      ),
    ];
    final rows = timelineRowsFromMessages([message], parts: parts);

    expect(rows, hasLength(1));
    expect(rows.single.type, TimelineRowType.toolGroup);
    expect(
      rows.single.toolGroup!.items.map((item) => item.name),
      orderedEquals(['edit_file', 'read_file', 'exec']),
    );

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              sessionId: 'session-1',
              rows: rows,
              turnPhase: TurnPhase.completed,
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    const summary =
        'edit_file completed · read_file completed · exec completed';
    expect(find.text(summary), findsOneWidget);
    expect(
      find.byKey(const ValueKey('timeline-tool-group-summary')),
      findsOneWidget,
    );
    expect(find.text('edit_file completed'), findsNothing);
    expect(find.text('read_file completed'), findsNothing);
    expect(find.textContaining('lib/timeline.dart'), findsNothing);
    expect(find.textContaining('flutter test'), findsNothing);

    await tester.tap(find.text(summary));
    await tester.pump();

    expect(
      find.byKey(const ValueKey('timeline-tool-group-details')),
      findsOneWidget,
    );
    expect(find.text('edit_file completed'), findsOneWidget);
    expect(find.text('read_file completed'), findsOneWidget);
    expect(find.text('exec completed'), findsOneWidget);
    expect(find.textContaining('lib/timeline.dart'), findsOneWidget);
    expect(find.textContaining('test/timeline_test.dart'), findsOneWidget);
    expect(find.textContaining('flutter test'), findsOneWidget);
    expect(find.textContaining('code/pure-studio'), findsOneWidget);
    expect(tester.takeException(), isNull);
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
        name: 'exec',
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
              turnPhase: TurnPhase.completed,
              rows: timelineRowsFromMessages([message], parts: parts),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(
      find.text('read_file completed · search_files completed'),
      findsOneWidget,
    );
    expect(find.text('exec completed'), findsOneWidget);
    expect(find.text('先读取相关文件。'), findsOneWidget);
    expect(find.text('再跑一下测试。'), findsOneWidget);
    expect(find.textContaining('lib/a.dart'), findsNothing);
    expect(find.textContaining('flutter test'), findsNothing);

    await tester.tap(find.text('read_file completed · search_files completed'));
    await tester.pump();

    expect(find.text('read_file completed'), findsOneWidget);
    expect(find.text('search_files completed'), findsOneWidget);
    expect(find.text('exec completed'), findsOneWidget);
    expect(find.textContaining('lib/a.dart'), findsOneWidget);
    expect(find.textContaining('activityGroupId'), findsOneWidget);
    expect(find.textContaining('flutter test'), findsNothing);

    await tester.tap(find.text('exec completed'));
    await tester.pump();

    expect(find.text('exec completed'), findsNWidgets(2));
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
        name: 'exec',
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
              turnPhase: TurnPhase.completed,
              rows: timelineRowsFromMessages([message], parts: parts),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(
      find.text(
        'exec awaiting approval · read_file failed · '
        'search_files running · file missing',
      ),
      findsOneWidget,
    );
    expect(find.text('awaitingApproval'), findsNothing);

    await tester.tap(
      find.text(
        'exec awaiting approval · read_file failed · '
        'search_files running · file missing',
      ),
    );
    await tester.pump();

    expect(find.text('exec awaiting approval'), findsOneWidget);
    expect(find.text('read_file failed'), findsOneWidget);
    expect(find.textContaining('cargo test -p pl-core'), findsOneWidget);
    expect(find.textContaining('lib/main.dart'), findsOneWidget);
    expect(find.textContaining('exit code 2'), findsOneWidget);
    expect(
      find.text('lib/main.dart\nexit code 2\nfile missing'),
      findsOneWidget,
    );
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
              turnPhase: TurnPhase.completed,
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
              turnPhase: TurnPhase.completed,
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
              turnPhase: TurnPhase.completed,
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

  testWidgets('timeline gives the current tool priority over reasoning', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final message = TimelineMessage(
      id: 'message-current-tool',
      sessionId: 'session-1',
      turnId: 'turn-1',
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(0),
    );
    final reasoning = TimelinePart(
      id: 'reasoning-current',
      messageId: message.id,
      sessionId: message.sessionId,
      turnId: message.turnId,
      type: TimelinePartType.reasoning,
      order: 0,
      text: '## Inspecting the implementation',
      status: 'streaming',
    );

    Widget timelineFor(TimelinePart tool, TurnPhase phase) {
      return _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              sessionId: message.sessionId,
              rows: timelineRowsFromMessages(
                [message],
                parts: [reasoning, tool],
              ),
              turnPhase: phase,
            ),
          ),
        ),
      );
    }

    final runningTool = _toolTimelinePart(
      id: 'tool-current',
      messageId: message.id,
      turnId: message.turnId,
      order: 1,
      name: 'exec',
      status: 'running',
      arguments: jsonEncode({'command': 'flutter test test/widget_test.dart'}),
    );
    await tester.pumpWidget(timelineFor(runningTool, TurnPhase.runningTool));
    await tester.pumpAndSettle();

    final currentActivity = find.byKey(
      const ValueKey('timeline-current-activity'),
    );
    expect(currentActivity, findsOneWidget);
    expect(
      find.descendant(
        of: currentActivity,
        matching: find.textContaining('flutter test test/widget_test.dart'),
      ),
      findsOneWidget,
    );
    expect(find.text('Inspecting the implementation'), findsOneWidget);

    final completedTool = _toolTimelinePart(
      id: 'tool-current',
      messageId: message.id,
      turnId: message.turnId,
      order: 1,
      name: 'exec',
      status: 'completed',
      arguments: jsonEncode({'command': 'flutter test test/widget_test.dart'}),
      result: 'passed',
    );
    await tester.pumpWidget(timelineFor(completedTool, TurnPhase.streaming));
    await tester.pumpAndSettle();

    expect(currentActivity, findsOneWidget);
    expect(
      find.descendant(
        of: currentActivity,
        matching: find.text('Inspecting the implementation'),
      ),
      findsOneWidget,
    );
    expect(find.text('exec completed'), findsOneWidget);
  });
}
