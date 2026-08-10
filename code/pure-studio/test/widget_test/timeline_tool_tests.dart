part of '../widget_test.dart';

void registerTimelineToolTests() {
  testWidgets('task_complete rejection exposes its stable code and message', (
    tester,
  ) async {
    final part = _toolTimelinePart(
      id: 'task-complete-1',
      groupId: 'task-complete-group',
      turnId: 'turn-task-complete',
      name: 'task_complete',
      status: 'failed',
      result: jsonEncode({
        'status': 'rejected',
        'code': 'reviewMissing',
        'recoverable': true,
        'message': 'Latest integrated review must pass',
      }),
    );

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: TimelineView(
            threadId: 'session-1',
            turn: null,
            rows: timelineRowsFromFixtureParts([part]),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('timeline-tool-group-summary')));
    await tester.pump();

    expect(
      find.text('reviewMissing\nLatest integrated review must pass'),
      findsOneWidget,
    );
  });

  testWidgets('completed task_complete hides its result payload', (
    tester,
  ) async {
    final part = _toolTimelinePart(
      id: 'task-complete-2',
      groupId: 'task-complete-completed-group',
      turnId: 'turn-task-completed',
      name: 'task_complete',
      status: 'completed',
      result: jsonEncode({
        'status': 'completed',
        'run': {'id': 'task-run-hidden', 'phase': 'completed'},
      }),
    );

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: TimelineView(
            threadId: 'session-1',
            turn: null,
            rows: timelineRowsFromFixtureParts([part]),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('timeline-tool-group-summary')));
    await tester.pump();

    expect(find.textContaining('task-run-hidden'), findsNothing);
  });

  testWidgets('timeline renders dedicated web search action and result links', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(900, 620);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final part = _toolTimelinePart(
      id: 'web-search-1',
      groupId: 'message-web-search',
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
              threadId: 'session-1',
              turn: null,
              rows: timelineRowsFromFixtureParts([part]),
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
            child: const TodoPanel(todo: todo),
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

    final part = TimelineEntry(
      id: 'tool-part-1',
      groupId: 'message-tool',
      threadId: 'session-1',
      turnId: 'turn-tool',
      type: TimelineEntryType.tool,
      order: 0,
      revision: 0,
      text: '',
      status: 'completed',
      createdAt: DateTime.fromMillisecondsSinceEpoch(0),
      updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
      tool: TimelineToolPart(
        toolCallId: 'tool-call-1',
        name: 'exec',
        arguments: jsonEncode({
          'command': 'cargo test -p pl-model\ncargo test -p pl-core',
        }),
        workingDirectory: 'D:/work/project',
        result: 'ok',
      ),
    );

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              threadId: 'session-1',
              turn: null,
              rows: timelineRowsFromFixtureParts([part]),
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

    final parts = [
      _toolTimelinePart(
        id: 'tool-edit',
        groupId: 'message-mixed-tools',
        turnId: 'turn-mixed-tools',
        name: 'edit_file',
        arguments: jsonEncode({'path': 'lib/timeline.dart'}),
      ),
      _toolTimelinePart(
        id: 'tool-read',
        groupId: 'message-mixed-tools',
        turnId: 'turn-mixed-tools',
        order: 1,
        name: 'read_file',
        arguments: jsonEncode({'path': 'test/timeline_test.dart'}),
      ),
      _toolTimelinePart(
        id: 'tool-exec',
        groupId: 'message-mixed-tools',
        turnId: 'turn-mixed-tools',
        order: 2,
        name: 'exec',
        arguments: jsonEncode({'command': 'flutter test'}),
        workingDirectory: 'code/pure-studio',
      ),
    ];
    final rows = timelineRowsFromFixtureParts(parts);

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
            child: TimelineView(threadId: 'session-1', rows: rows, turn: null),
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

    final parts = [
      TimelineEntry(
        id: 'text-before',
        groupId: 'turn-1:assistant',
        threadId: 'session-1',
        turnId: 'turn-1',
        type: TimelineEntryType.text,
        text: '先读取相关文件。',
        textChannel: TimelineTextChannel.commentary,
        order: 0,
      ),
      _toolTimelinePart(
        id: 'tool-a',
        groupId: 'turn-1:assistant',
        turnId: 'turn-1',
        order: 1,
        name: 'read_file',
        arguments: jsonEncode({'path': 'lib/a.dart'}),
      ),
      _toolTimelinePart(
        id: 'tool-b',
        groupId: 'turn-1:assistant',
        turnId: 'turn-1',
        order: 2,
        name: 'search_files',
        arguments: jsonEncode({'query': 'TimelineToolGroup'}),
      ),
      TimelineEntry(
        id: 'text-middle',
        groupId: 'turn-1:assistant',
        threadId: 'session-1',
        turnId: 'turn-1',
        type: TimelineEntryType.text,
        text: '再跑一下测试。',
        textChannel: TimelineTextChannel.commentary,
        order: 3,
      ),
      _toolTimelinePart(
        id: 'tool-c',
        groupId: 'turn-1:assistant',
        turnId: 'turn-1',
        order: 4,
        name: 'exec',
        arguments: jsonEncode({'command': 'flutter test'}),
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 620,
            child: TimelineView(
              threadId: 'session-1',
              turn: null,
              rows: timelineRowsFromFixtureParts(parts),
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
    expect(find.textContaining('TimelineToolGroup'), findsOneWidget);
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

    final parts = [
      _toolTimelinePart(
        id: 'tool-awaiting',
        groupId: 'message-tool',
        turnId: 'turn-tool',
        status: 'awaitingApproval',
        name: 'exec',
        arguments: jsonEncode({'command': 'cargo test -p pl-core'}),
      ),
      _toolTimelinePart(
        id: 'tool-failed',
        groupId: 'message-tool',
        turnId: 'turn-tool',
        order: 1,
        status: 'failed',
        name: 'read_file',
        arguments: jsonEncode({'path': 'lib/main.dart'}),
        result: 'file missing',
        exitCode: 2,
      ),
      _toolTimelinePart(
        id: 'tool-running',
        groupId: 'message-tool',
        turnId: 'turn-tool',
        order: 2,
        status: 'running',
        name: 'search_files',
        arguments: jsonEncode({'query': 'TimelineToolGroup'}),
      ),
    ];

    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              threadId: 'session-1',
              turn: null,
              rows: timelineRowsFromFixtureParts(parts),
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

    const parts = [
      TimelineEntry(
        id: 'plan-inline-fence',
        groupId: 'message-inline-fence',
        type: TimelineEntryType.plan,
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
              threadId: 'session-1',
              turn: null,
              rows: timelineRowsFromFixtureParts(parts),
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

    const parts = [
      TimelineEntry(
        id: 'text-markdown-chrome',
        groupId: 'message-markdown-chrome',
        type: TimelineEntryType.text,
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
              threadId: 'session-1',
              turn: null,
              rows: timelineRowsFromFixtureParts(parts),
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

    const parts = [
      TimelineEntry(
        id: 'plan-agent-markdown',
        groupId: 'message-agent-markdown',
        type: TimelineEntryType.plan,
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
              threadId: 'session-1',
              turn: null,
              rows: timelineRowsFromFixtureParts(parts),
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

    const threadId = 'session-1';
    const turnId = 'turn-1';
    final reasoning = _threadItemFixture(
      id: 'reasoning-current',
      threadId: threadId,
      turnId: turnId,
      ordinal: 0,
      kind: ThreadItemKind.reasoning,
      channel: null,
      reasoningSummary: const ['## Inspecting the implementation'],
      status: 'streaming',
    );

    ThreadItemView toolItem({required String status, String? result}) {
      return _threadItemFixture(
        id: 'tool-current',
        threadId: threadId,
        turnId: turnId,
        ordinal: 1,
        kind: ThreadItemKind.toolCall,
        channel: null,
        status: status,
        tool: TimelineToolPart(
          toolCallId: 'tool-current',
          name: 'exec',
          arguments: jsonEncode({
            'command': 'flutter test test/widget_test.dart',
          }),
          result: result,
        ),
      );
    }

    Widget timelineFor(ThreadItemView tool, StudioTurnActivity activity) {
      return _timelineApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 520,
            child: TimelineView(
              threadId: threadId,
              rows: timelineRowsFromThreadItems([reasoning, tool]),
              turn: _testTurn(
                threadId: threadId,
                turnId: turnId,
                state: StudioTurnState.inProgress(activity),
              ),
            ),
          ),
        ),
      );
    }

    final runningTool = toolItem(status: 'running');
    await tester.pumpWidget(
      timelineFor(runningTool, StudioTurnActivity.runningTool),
    );
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

    final completedTool = toolItem(status: 'completed', result: 'passed');
    await tester.pumpWidget(
      timelineFor(completedTool, StudioTurnActivity.thinking),
    );
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
