part of '../widget_test.dart';

void registerTimelineModelTests() {
  test('timeline row render version changes for same length replacements', () {
    final now = DateTime.fromMillisecondsSinceEpoch(0);
    TimelineRow rowForPart(String text) {
      return timelineRowsFromMessages(
        [
          TimelineMessage(
            id: 'message-1',
            sessionId: 'session-1',
            role: 'assistant',
            createdAt: now,
          ),
        ],
        parts: [
          TimelinePart(
            id: 'part-1',
            messageId: 'message-1',
            type: TimelinePartType.text,
            text: text,
            status: 'streaming',
          ),
        ],
      ).single;
    }

    expect('alpha'.length, 'bravo'.length);
    expect(
      rowForPart('alpha').renderVersion,
      isNot(rowForPart('bravo').renderVersion),
    );
  });

  test('timeline reasoning group preserves text and render version', () {
    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'message-1',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: now,
    );

    TimelineRow rowForReasoning(String text) {
      return timelineRowsFromMessages(
        [message],
        parts: [
          timelinePartFromSnapshot(
            TimelinePartSnapshot(
              id: 'reasoning-1',
              messageId: message.id,
              sessionId: 'session-1',
              turnId: 'turn-1',
              type: TimelinePartType.reasoning,
              order: 0,
              revision: 0,
              text: text,
              status: 'streaming',
              createdAt: now,
              updatedAt: now,
            ),
          ),
        ],
      ).single;
    }

    final first = rowForReasoning('alpha');
    final second = rowForReasoning('bravo');
    expect(first.reasoningGroup!.details, 'alpha');
    expect(first.reasoningGroup!.latestSummary, 'alpha');
    expect(first.renderVersion, isNot(second.renderVersion));
  });

  test('timeline only groups adjacent reasoning parts', () {
    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      turnId: 'turn-1',
      role: 'assistant',
      createdAt: now,
    );

    TimelinePart reasoning({
      required String id,
      required int order,
      required String text,
      String status = 'completed',
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

    final rows = timelineRowsFromMessages(
      [message],
      parts: [
        reasoning(id: 'reasoning-a', order: 0, text: '## 检查输入'),
        reasoning(id: 'reasoning-b', order: 1, text: '**确认边界**'),
        _toolTimelinePart(
          id: 'tool-a',
          messageId: message.id,
          turnId: message.turnId,
          order: 2,
          name: 'read_file',
        ),
        reasoning(
          id: 'reasoning-c',
          order: 3,
          text: '<!-- -->',
          status: 'streaming',
        ),
        reasoning(id: 'reasoning-d', order: 4, text: '- 继续分析'),
        TimelinePart(
          id: 'final-a',
          messageId: message.id,
          sessionId: message.sessionId,
          turnId: message.turnId,
          type: TimelinePartType.text,
          order: 5,
          text: 'answer',
          textChannel: TimelineTextChannel.finalAnswer,
        ),
      ],
    );

    expect(rows.map((row) => row.type), [
      TimelineRowType.reasoningSummary,
      TimelineRowType.toolGroup,
      TimelineRowType.reasoningSummary,
      TimelineRowType.finalAnswer,
    ]);
    final firstGroup = rows.first.reasoningGroup!;
    expect(
      firstGroup.id,
      'reasoning-group:session-1:turn-1:assistant:reasoning-a',
    );
    expect(firstGroup.parts.map((part) => part.id), [
      'reasoning-a',
      'reasoning-b',
    ]);
    expect(firstGroup.summaries, ['检查输入', '确认边界']);
    final secondGroup = rows[2].reasoningGroup!;
    expect(secondGroup.parts.map((part) => part.id), [
      'reasoning-c',
      'reasoning-d',
    ]);
    expect(secondGroup.summaries, ['继续分析']);
    expect(secondGroup.latestSummary, '继续分析');
    expect(secondGroup.isActive, isTrue);
  });

  test('timeline only groups adjacent tool parts', () {
    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      turnId: 'turn-1',
      role: 'assistant',
      createdAt: now,
      sequence: 1,
    );
    final rows = timelineRowsFromMessages(
      [message],
      parts: [
        _toolTimelinePart(
          id: 'tool-a',
          messageId: message.id,
          turnId: 'turn-1',
          order: 2,
          sequence: 3,
          name: 'read_file',
          activityGroupId: 'legacy-whole-turn-group',
          result: 'ok',
        ),
        _toolTimelinePart(
          id: 'tool-b',
          messageId: message.id,
          turnId: 'turn-1',
          order: 3,
          sequence: 4,
          name: 'search_files',
          activityGroupId: 'another-legacy-group',
          result: 'matches',
        ),
        TimelinePart(
          id: 'final-a',
          messageId: message.id,
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.text,
          text: 'answer',
          textChannel: TimelineTextChannel.finalAnswer,
          order: 5,
          sequence: 5,
        ),
        _toolTimelinePart(
          id: 'tool-c',
          messageId: message.id,
          turnId: 'turn-1',
          order: 6,
          sequence: 7,
          name: 'write_file',
          activityGroupId: 'legacy-whole-turn-group',
          status: 'denied',
        ),
      ],
    );

    final toolGroups = rows
        .where((row) => row.type == TimelineRowType.toolGroup)
        .toList();
    expect(toolGroups, hasLength(2));
    expect(toolGroups.first.id, 'tool-group:session-1:turn-1:assistant:tool-a');
    expect(toolGroups.first.order, 2);
    expect(toolGroups.first.sequence, 4);
    expect(toolGroups.first.toolGroup!.items.map((item) => item.id), [
      'tool-a',
      'tool-b',
    ]);
    expect(toolGroups.first.toolGroup!.status, 'completed');
    expect(toolGroups[1].id, 'tool-group:session-1:turn-1:assistant:tool-c');
    expect(toolGroups[1].toolGroup!.status, 'denied');
  });

  test(
    'timeline keeps different activityGroupId separate inside one message',
    () {
      final now = DateTime.fromMillisecondsSinceEpoch(0);
      final message = TimelineMessage(
        id: 'turn-1:assistant',
        sessionId: 'session-1',
        turnId: 'turn-1',
        role: 'assistant',
        createdAt: now,
      );
      final rows = timelineRowsFromMessages(
        [message],
        parts: [
          TimelinePart(
            id: 'text-before',
            messageId: message.id,
            sessionId: 'session-1',
            turnId: 'turn-1',
            type: TimelinePartType.text,
            text: '先看一下。',
            textChannel: TimelineTextChannel.commentary,
            order: 0,
          ),
          _toolTimelinePart(
            id: 'tool-a',
            messageId: message.id,
            turnId: 'turn-1',
            order: 1,
            name: 'read_file',
            activityGroupId: 'tool-group:turn-1:1',
          ),
          _toolTimelinePart(
            id: 'tool-b',
            messageId: message.id,
            turnId: 'turn-1',
            order: 2,
            name: 'search_files',
            activityGroupId: 'tool-group:turn-1:1',
          ),
          TimelinePart(
            id: 'text-middle',
            messageId: message.id,
            sessionId: 'session-1',
            turnId: 'turn-1',
            type: TimelinePartType.text,
            text: '我再查一个点。',
            textChannel: TimelineTextChannel.commentary,
            order: 3,
          ),
          _toolTimelinePart(
            id: 'tool-c',
            messageId: message.id,
            turnId: 'turn-1',
            order: 4,
            name: 'exec',
            activityGroupId: 'tool-group:turn-1:4',
          ),
        ],
      );

      expect(rows.map((row) => row.type), [
        TimelineRowType.commentary,
        TimelineRowType.toolGroup,
        TimelineRowType.commentary,
        TimelineRowType.toolGroup,
      ]);
      expect(rows[1].id, 'tool-group:session-1:turn-1:assistant:tool-a');
      expect(rows[1].toolGroup!.items.map((item) => item.id), [
        'tool-a',
        'tool-b',
      ]);
      expect(rows[3].id, 'tool-group:session-1:turn-1:assistant:tool-c');
      expect(rows[3].toolGroup!.items.single.id, 'tool-c');
    },
  );

  test('timeline groups adjacent tools without activityGroupId', () {
    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      turnId: 'turn-1',
      role: 'assistant',
      createdAt: now,
    );
    final rows = timelineRowsFromMessages(
      [message],
      parts: [
        _toolTimelinePart(
          id: 'tool-a',
          messageId: message.id,
          turnId: 'turn-1',
          order: 1,
          name: 'read_file',
          activityGroupId: null,
        ),
        _toolTimelinePart(
          id: 'tool-b',
          messageId: message.id,
          turnId: 'turn-1',
          order: 2,
          name: 'search_files',
          activityGroupId: null,
        ),
      ],
    );

    expect(rows, hasLength(1));
    expect(rows.single.toolGroup!.items.map((item) => item.id), [
      'tool-a',
      'tool-b',
    ]);
  });

  test('timeline tool group render version tracks tool result changes', () {
    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final message = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      turnId: 'turn-1',
      role: 'assistant',
      createdAt: now,
    );

    TimelineRow rowForResult(String result) {
      return timelineRowsFromMessages(
        [message],
        parts: [
          _toolTimelinePart(
            id: 'tool-a',
            messageId: message.id,
            turnId: 'turn-1',
            name: 'exec',
            result: result,
          ),
        ],
      ).single;
    }

    expect(
      rowForResult('alpha').renderVersion,
      isNot(rowForResult('bravo').renderVersion),
    );
  });

  test('agent timeline events stay outside message snapshots', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      _agentTimelineEvent(
        sessionId: 'session-1',
        event: {
          'eventId': 'agent-event-1',
          'sessionId': 'session-1',
          'sequence': 1,
          'createdAt': 1,
          'kind': {
            'type': 'subAgentActivity',
            'callId': 'call-1',
            'path': 'root/reviewer',
            'parentPath': 'root',
            'kind': 'spawned',
            'status': 'queued',
            'message': 'check',
            'timedOut': false,
          },
        },
      ),
    );
    api.emitSession(
      _agentTimelineEvent(
        sessionId: 'session-1',
        event: {
          'eventId': 'agent-event-2',
          'sessionId': 'session-1',
          'sequence': 2,
          'createdAt': 2,
          'kind': {
            'type': 'subAgentActivity',
            'callId': 'call-1',
            'path': 'root/reviewer',
            'parentPath': 'root',
            'kind': 'spawned',
            'status': 'completed',
            'message': 'check',
            'timedOut': false,
          },
        },
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.messagesBySession['session-1'], isEmpty);
    expect(state.agentTimelineEventsBySession['session-1']!.keys, {
      'agent-event-1',
      'agent-event-2',
    });
    expect(state.selectedMessages, isEmpty);
    final row = state.selectedTimelineRows.single;
    expect(row.messageId, isNull);
    expect(row.part, isNull);
    expect(row.id, 'agent-activity:call-1');
    expect(row.agentEvent!.callId, 'call-1');
    expect(row.agentEvent!.eventId, 'agent-event-2');
    expect(row.agentEvent!.title, 'agentTimeline.spawn');
    expect(row.agentEvent!.text, contains('root/reviewer'));
    expect(row.agentEvent!.status, 'completed');
    expect(row.agentEvent!.payload, isA<TimelineSubAgentActivity>());
  });

  test('todo list updates stay out of the timeline projection', () {
    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final rows = timelineRowsFromMessages(
      const [],
      agentEvents: [
        TimelineAgentEvent(
          eventId: 'todo-event-1',
          sessionId: 'session-1',
          sequence: 1,
          createdAt: now,
          payload: const TimelineTodoListUpdate(
            callId: 'call-1',
            path: '/root',
            explanation: 'First pass',
            items: [
              TimelineTodoItem(step: 'Read code', status: 'completed'),
              TimelineTodoItem(step: 'Patch code', status: 'inProgress'),
            ],
          ),
        ),
        TimelineAgentEvent(
          eventId: 'todo-event-2',
          sessionId: 'session-1',
          sequence: 2,
          createdAt: now.add(const Duration(seconds: 1)),
          payload: const TimelineTodoListUpdate(
            callId: 'call-1',
            path: '/root',
            explanation: 'Second pass',
            items: [
              TimelineTodoItem(step: 'Read code', status: 'completed'),
              TimelineTodoItem(step: 'Patch code', status: 'completed'),
              TimelineTodoItem(step: 'Run tests', status: 'pending'),
            ],
          ),
        ),
      ],
    );

    expect(rows, isEmpty);
  });

  test('agent snapshots update status state without timeline rows', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      _canonicalSessionEvent(
        sessionId: 'session-1',
        eventId: 'agent-snapshot-1',
        kind: {
          'type': 'agentChanged',
          'agent': {
            'id': 'agent-1',
            'sessionId': 'session-1',
            'path': 'root/reviewer',
            'role': 'reviewer',
            'task': 'Audit timeline',
            'status': 'running',
            'depth': 1,
            'updatedAt': 1,
          },
        },
      ),
    );
    api.emitSession(
      _canonicalSessionEvent(
        sessionId: 'session-1',
        eventId: 'agent-snapshot-2',
        sequence: 2,
        emittedAt: 2,
        kind: {
          'type': 'agentChanged',
          'agent': {
            'id': 'agent-1',
            'sessionId': 'session-1',
            'path': 'root/reviewer',
            'role': 'reviewer',
            'task': 'Audit timeline',
            'status': 'completed',
            'summary': 'done',
            'depth': 1,
            'updatedAt': 2,
          },
        },
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedTimelineRows, isEmpty);
    expect(state.runtime.agentCount, 1);
    expect(state.selectedAgents, hasLength(1));
    expect(state.selectedAgents.single.status, 'completed');
    expect(state.selectedAgents.single.summary, 'done');
    expect(
      state.agentsBySession['session-1']!['agent-1']!.path,
      'root/reviewer',
    );
  });

  test(
    'canonical session snapshot restores agent timeline events for projection',
    () {
      final state = applyCanonicalSessionSnapshot(_emptyState(), {
        'sessionId': 'session-1',
        'throughSequence': 7,
        'timelineEvents': [
          {
            'eventId': 'agent-event-1',
            'sessionId': 'session-1',
            'sequence': 7,
            'createdAt': 3,
            'kind': {
              'type': 'subAgentActivity',
              'callId': 'call-2',
              'path': 'root/worker',
              'parentPath': 'root',
              'kind': 'messageQueued',
              'status': 'waiting',
              'message': 'status',
              'timedOut': false,
            },
          },
        ],
      });

      expect(state.agentTimelineEventsBySession['session-1']!.keys, {
        'agent-event-1',
      });
      expect(state.selectedMessages, isEmpty);
      final row = state.selectedTimelineRows.single;
      expect(row.messageId, isNull);
      expect(row.part, isNull);
      expect(row.agentEvent!.callId, 'call-2');
      expect(row.agentEvent!.title, 'agentTimeline.message');
      expect(row.agentEvent!.text, contains('root/worker'));
      expect(row.agentEvent!.text, contains('status'));
      expect(row.agentEvent!.payload, isA<TimelineSubAgentActivity>());
    },
  );

  test('canonical session snapshot restores the latest todo list', () {
    final state = applyCanonicalSessionSnapshot(_emptyState(), {
      'sessionId': 'session-1',
      'throughSequence': 8,
      'timelineEvents': [
        {
          'eventId': 'todo-event-1',
          'sessionId': 'session-1',
          'sequence': 8,
          'createdAt': 4,
          'kind': {
            'type': 'todoListChanged',
            'snapshot': {
              'callId': 'call-3',
              'path': '/root/worker',
              'parentPath': '/root',
              'explanation': 'Todo restore',
              'items': [
                {'step': 'Restore payload', 'status': 'completed'},
                {'step': 'Render row', 'status': 'pending'},
              ],
            },
          },
        },
      ],
    });

    expect(state.selectedTimelineRows, isEmpty);
    final update = state.selectedTodoList;
    expect(update, isNotNull);
    expect(update!.callId, 'call-3');
    expect(update.explanation, 'Todo restore');
    expect(update.items.map((item) => item.step), [
      'Restore payload',
      'Render row',
    ]);
  });

  test('canonical session snapshot restores agent status state', () {
    final state = applyCanonicalSessionSnapshot(_emptyState(), {
      'sessionId': 'session-1',
      'throughSequence': 1,
      'agents': [
        {
          'id': 'agent-1',
          'sessionId': 'session-1',
          'path': 'root/worker',
          'role': 'worker',
          'task': 'Implement',
          'status': 'running',
          'summary': 'halfway',
          'depth': 1,
          'updatedAt': 4,
        },
      ],
    });

    expect(state.runtime.agentCount, 1);
    expect(state.selectedTimelineRows, isEmpty);
    expect(state.selectedAgents.single.id, 'agent-1');
    expect(state.selectedAgents.single.path, 'root/worker');
    expect(state.selectedAgents.single.summary, 'halfway');
  });

  test('unknown agent timeline kinds fail protocol projection', () {
    expect(
      () => timelineAgentEventFromPayload({
        'eventId': 'agent-event-unknown',
        'sessionId': 'session-1',
        'sequence': 1,
        'createdAt': 1,
        'kind': {'type': 'mystery', 'callId': 'call-1'},
      }),
      throwsA(isA<FormatException>()),
    );
  });
}
