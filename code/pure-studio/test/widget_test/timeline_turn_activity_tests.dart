part of '../widget_test.dart';

void registerTimelineTurnActivityTests() {
  testWidgets('timeline renders exactly one localized row for every activity', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    const cases = [
      (StudioTurnState.queued(), '排队中'),
      (StudioTurnState.inProgress(StudioTurnActivity.preparing), '准备上下文'),
      (StudioTurnState.inProgress(StudioTurnActivity.thinking), '思考中'),
      (StudioTurnState.inProgress(StudioTurnActivity.responding), '回复中'),
      (StudioTurnState.inProgress(StudioTurnActivity.planning), '规划中'),
      (StudioTurnState.inProgress(StudioTurnActivity.runningTool), '运行工具'),
      (
        StudioTurnState.inProgress(StudioTurnActivity.waitingForApproval),
        '等待工具授权',
      ),
      (
        StudioTurnState.inProgress(StudioTurnActivity.waitingForUserInput),
        '等待输入',
      ),
      (
        StudioTurnState.inProgress(
          StudioTurnActivity.waitingForPlanConfirmation,
        ),
        '等待计划确认',
      ),
      (StudioTurnState.inProgress(StudioTurnActivity.persisting), '保存本轮结果'),
    ];

    for (final (state, label) in cases) {
      await tester.pumpWidget(
        _timelineApp(
          locale: const Locale('zh', 'Hans'),
          home: Scaffold(
            body: TimelineView(
              sessionId: 'session-1',
              rows: const [],
              turn: _testTurn(sessionId: 'session-1', state: state),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.byKey(const ValueKey('timeline-current-activity')),
        findsOneWidget,
        reason: label,
      );
      expect(find.text(label), findsOneWidget, reason: label);
    }

    for (final state in const [
      StudioTurnState.completed(),
      StudioTurnState.failed('failed'),
      StudioTurnState.cancelled('cancelled'),
    ]) {
      await tester.pumpWidget(
        _timelineApp(
          home: Scaffold(
            body: TimelineView(
              sessionId: 'session-1',
              rows: const [],
              turn: _testTurn(sessionId: 'session-1', state: state),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('timeline-current-activity')),
        findsNothing,
      );
    }
  });

  testWidgets('thinking keeps raw details but shows one muted summary row', (
    tester,
  ) async {
    final now = DateTime.fromMillisecondsSinceEpoch(1);
    final message = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      turnId: 'turn-1',
      role: 'assistant',
      status: 'streaming',
      createdAt: now,
    );
    final reasoning = timelinePartFromSnapshot(
      TimelinePartSnapshot(
        id: 'reasoning-1',
        messageId: message.id,
        sessionId: message.sessionId,
        turnId: message.turnId,
        type: TimelinePartType.reasoning,
        order: 0,
        revision: 2,
        text: '',
        reasoningSummary: const ['核对状态机'],
        reasoningContent: const ['raw reasoning detail'],
        status: 'streaming',
        createdAt: now,
        updatedAt: now,
      ),
    );

    await tester.pumpWidget(
      _timelineApp(
        locale: const Locale('zh', 'Hans'),
        home: Scaffold(
          body: TimelineView(
            sessionId: message.sessionId,
            rows: timelineRowsFromMessages([message], parts: [reasoning]),
            turn: _testTurn(
              sessionId: message.sessionId,
              turnId: message.turnId,
              state: const StudioTurnState.inProgress(
                StudioTurnActivity.thinking,
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey('timeline-current-activity')),
      findsOneWidget,
    );
    expect(find.text('核对状态机'), findsOneWidget);
    expect(find.textContaining('raw reasoning detail'), findsNothing);
    final summary = tester.widget<Text>(find.text('核对状态机'));
    final context = tester.element(find.byType(TimelineView));
    expect(summary.style?.color, context.studioInkSoft);

    await tester.tap(find.text('核对状态机'));
    await tester.pumpAndSettle();
    expect(find.textContaining('raw reasoning detail'), findsOneWidget);
  });

  testWidgets('responding keeps growing text beside its lightweight status', (
    tester,
  ) async {
    final message = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      turnId: 'turn-1',
      role: 'assistant',
      status: 'streaming',
      createdAt: DateTime.fromMillisecondsSinceEpoch(1),
    );
    const part = TimelinePart(
      id: 'response-1',
      messageId: 'turn-1:assistant',
      sessionId: 'session-1',
      turnId: 'turn-1',
      type: TimelinePartType.text,
      textChannel: TimelineTextChannel.finalAnswer,
      text: '正在增长的正文',
      status: 'streaming',
    );

    await tester.pumpWidget(
      _timelineApp(
        locale: const Locale('zh', 'Hans'),
        home: Scaffold(
          body: TimelineView(
            sessionId: message.sessionId,
            rows: timelineRowsFromMessages([message], parts: const [part]),
            turn: _testTurn(
              sessionId: message.sessionId,
              turnId: message.turnId,
              state: const StudioTurnState.inProgress(
                StudioTurnActivity.responding,
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('正在增长的正文'), findsOneWidget);
    expect(find.text('回复中'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('timeline-current-activity')),
      findsOneWidget,
    );
  });

  testWidgets(
    'interaction waiting replaces the active tool with one typed row',
    (tester) async {
      final message = TimelineMessage(
        id: 'turn-1:assistant',
        sessionId: 'session-1',
        turnId: 'turn-1',
        role: 'assistant',
        status: 'streaming',
        createdAt: DateTime.fromMillisecondsSinceEpoch(1),
      );
      final tool = _toolTimelinePart(
        id: 'tool-current',
        messageId: message.id,
        turnId: message.turnId,
        name: 'request_user_input',
        status: 'running',
      );
      const cases = [
        (StudioTurnActivity.waitingForApproval, '等待工具授权'),
        (StudioTurnActivity.waitingForUserInput, '等待输入'),
        (StudioTurnActivity.waitingForPlanConfirmation, '等待计划确认'),
      ];

      for (final (activity, label) in cases) {
        await tester.pumpWidget(
          _timelineApp(
            locale: const Locale('zh', 'Hans'),
            home: Scaffold(
              body: TimelineView(
                sessionId: message.sessionId,
                rows: timelineRowsFromMessages([message], parts: [tool]),
                turn: _testTurn(
                  sessionId: message.sessionId,
                  turnId: message.turnId,
                  state: StudioTurnState.inProgress(activity),
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();

        expect(find.text(label), findsOneWidget, reason: label);
        expect(find.textContaining('request_user_input'), findsNothing);
        expect(
          find.byKey(const ValueKey('timeline-current-activity')),
          findsOneWidget,
        );
      }
    },
  );

  testWidgets(
    'non-user rows have no generic avatar and terminal errors persist',
    (tester) async {
      final message = TimelineMessage(
        id: 'turn-1:assistant',
        sessionId: 'session-1',
        turnId: 'turn-1',
        role: 'assistant',
        status: 'failed',
        error: 'provider failed',
        createdAt: DateTime.fromMillisecondsSinceEpoch(1),
      );
      const failure = TimelinePart(
        id: 'turn-1:terminal-result',
        messageId: 'turn-1:assistant',
        sessionId: 'session-1',
        turnId: 'turn-1',
        type: TimelinePartType.text,
        textChannel: TimelineTextChannel.finalAnswer,
        text: 'provider failed',
        status: 'failed',
        error: 'provider failed',
      );

      await tester.pumpWidget(
        _timelineApp(
          home: Scaffold(
            body: TimelineView(
              sessionId: message.sessionId,
              rows: timelineRowsFromMessages([message], parts: const [failure]),
              turn: _testTurn(
                sessionId: message.sessionId,
                turnId: message.turnId,
                state: const StudioTurnState.failed('provider failed'),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('provider failed'), findsOneWidget);
      expect(find.byIcon(Icons.auto_awesome), findsNothing);
      expect(
        find.byKey(const ValueKey('timeline-current-activity')),
        findsNothing,
      );
      final errorText = tester.widget<Text>(find.text('provider failed'));
      final timelineContext = tester.element(find.byType(TimelineView));
      expect(
        errorText.textSpan?.style?.color,
        Theme.of(timelineContext).colorScheme.error,
      );
    },
  );
}
