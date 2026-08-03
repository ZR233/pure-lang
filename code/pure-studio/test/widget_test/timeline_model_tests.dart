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
          result: 'ok',
        ),
        _toolTimelinePart(
          id: 'tool-b',
          messageId: message.id,
          turnId: 'turn-1',
          order: 3,
          sequence: 4,
          name: 'search_files',
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

  test('timeline commentary separates adjacent tool groups', () {
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
        ),
        _toolTimelinePart(
          id: 'tool-b',
          messageId: message.id,
          turnId: 'turn-1',
          order: 2,
          name: 'search_files',
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
        event: TimelineAgentEvent(
          eventId: 'agent-event-1',
          sessionId: 'session-1',
          sequence: 1,
          createdAt: _fixtureDate(1),
          payload: const TimelineSubAgentActivity(
            callId: 'call-1',
            path: 'root/reviewer',
            parentPath: 'root',
            kind: 'spawned',
            statusValue: 'queued',
            message: 'check',
            timedOut: false,
          ),
        ),
      ),
    );
    api.emitSession(
      _agentTimelineEvent(
        sessionId: 'session-1',
        event: TimelineAgentEvent(
          eventId: 'agent-event-2',
          sessionId: 'session-1',
          sequence: 2,
          createdAt: _fixtureDate(2),
          payload: const TimelineSubAgentActivity(
            callId: 'call-1',
            path: 'root/reviewer',
            parentPath: 'root',
            kind: 'spawned',
            statusValue: 'completed',
            message: 'check',
            timedOut: false,
          ),
        ),
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
      StudioBridgeEvent(
        eventId: 'agent-snapshot-1',
        sessionId: 'session-1',
        sequence: BigInt.one,
        createdAt: _fixtureDate(1),
        payload: AgentChangedPayload(
          agent: StudioAgentView(
            id: 'agent-1',
            sessionId: 'session-1',
            path: 'root/reviewer',
            role: 'reviewer',
            task: 'Audit timeline',
            status: 'running',
            depth: 1,
            updatedAt: _fixtureDate(1),
          ),
        ),
      ),
    );
    api.emitSession(
      StudioBridgeEvent(
        eventId: 'agent-snapshot-2',
        sessionId: 'session-1',
        sequence: BigInt.two,
        createdAt: _fixtureDate(2),
        payload: AgentChangedPayload(
          agent: StudioAgentView(
            id: 'agent-1',
            sessionId: 'session-1',
            path: 'root/reviewer',
            role: 'reviewer',
            task: 'Audit timeline',
            status: 'completed',
            summary: 'done',
            depth: 1,
            updatedAt: _fixtureDate(2),
          ),
        ),
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

  test('background agent snapshots stay normalized by their own session', () {
    final state = reduceStudioEvent(
      _emptyState(),
      StudioBridgeEvent(
        eventId: 'background-agent-snapshot',
        sessionId: 'session-2',
        sequence: BigInt.one,
        createdAt: _fixtureDate(1),
        payload: AgentChangedPayload(
          agent: StudioAgentView(
            id: 'agent-background',
            sessionId: 'session-2',
            path: 'root/executor',
            role: 'executor',
            task: 'Implement in background',
            status: 'running',
            updatedAt: _fixtureDate(1),
          ),
        ),
      ),
    ).state;

    expect(state.selectedSessionId, 'session-1');
    expect(state.selectedAgents, isEmpty);
    expect(
      state.agentsBySession['session-2']!['agent-background']!.status,
      'running',
    );
    expect(state.runtimesBySession['session-2']!.agentCount, 1);
  });

  test(
    'agent directory product event updates root and canonical sessions',
    () async {
      final api = _FakeStudioApi(_emptyState());
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      api.emitGlobal(
        StudioBridgeEvent.fromProduct(
          frb.BridgeProductEventEnvelope(
            eventId: 'agent-directory-1',
            projectId: 'project-1',
            sequence: BigInt.from(12),
            createdAt: 12,
            payload: frb.BridgeProductEventPayload.agentDirectoryChanged(
              rootSessionId: 'session-1',
              agent: frb.BridgeAgentDirectoryEntryDto(
                id: 'agent-background',
                sessionId: 'session-2',
                rootSessionId: 'session-1',
                path: 'root/executor',
                role: 'executor',
                task: 'Implement in background',
                status: 'running',
                summary: 'Implemented the runtime boundary',
                depth: 1,
                lifecycle: 'active',
                activity: 'waiting',
                progress: frb.BridgeAgentProgressDto(
                  stage: 'readyForReview',
                  summary: 'Implementation complete',
                  nextStep: 'Await delivery review',
                  revision: BigInt.two,
                  updatedAt: 10,
                ),
                updatedAt: 10,
                summaryAgeSeconds: BigInt.two,
              ),
            ),
          ),
        ),
      );
      await pumpEventQueue();

      final state = container.read(studioControllerProvider).requireValue;
      final rootAgent =
          state.agentsBySession['session-1']!['agent-background']!;
      final canonicalAgent =
          state.agentsBySession['session-2']!['agent-background']!;
      expect(state.selectedSessionId, 'session-1');
      expect(identical(rootAgent, canonicalAgent), isTrue);
      expect(rootAgent.rootSessionId, 'session-1');
      expect(rootAgent.lifecycle, 'active');
      expect(rootAgent.activity, 'waiting');
      expect(rootAgent.progress?.stage, 'readyForReview');
      expect(rootAgent.progress?.revision, 2);
      expect(rootAgent.summaryAgeSeconds, 2);
      expect(state.runtimesBySession['session-1']!.agentCount, 1);
      expect(state.runtimesBySession['session-2']!.agentCount, 1);
      expect(state.eventCursorsBySession['session-2'], isNull);
    },
  );

  test(
    'canonical session snapshot restores agent timeline events for projection',
    () {
      final state = applyCanonicalSessionSnapshot(
        _emptyState(),
        StudioSessionSnapshot(
          sessionId: 'session-1',
          throughSequence: 7,
          messages: const [],
          parts: const {},
          interactions: const [],
          agents: const {},
          timelineEvents: {
            'agent-event-1': TimelineAgentEvent(
              eventId: 'agent-event-1',
              sessionId: 'session-1',
              sequence: 7,
              createdAt: _fixtureDate(3),
              payload: const TimelineSubAgentActivity(
                callId: 'call-2',
                path: 'root/worker',
                parentPath: 'root',
                kind: 'messageQueued',
                statusValue: 'waiting',
                message: 'status',
                timedOut: false,
              ),
            ),
          },
          runtime: null,
          turn: null,
        ),
      );

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
    final state = applyCanonicalSessionSnapshot(
      _emptyState(),
      StudioSessionSnapshot(
        sessionId: 'session-1',
        throughSequence: 8,
        messages: const [],
        parts: const {},
        interactions: const [],
        agents: const {},
        timelineEvents: {
          'todo-event-1': TimelineAgentEvent(
            eventId: 'todo-event-1',
            sessionId: 'session-1',
            sequence: 8,
            createdAt: _fixtureDate(4),
            payload: const TimelineTodoListUpdate(
              callId: 'call-3',
              path: '/root/worker',
              parentPath: '/root',
              explanation: 'Todo restore',
              items: [
                TimelineTodoItem(step: 'Restore payload', status: 'completed'),
                TimelineTodoItem(step: 'Render row', status: 'pending'),
              ],
            ),
          ),
        },
        runtime: null,
        turn: null,
      ),
    );

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
    final state = applyCanonicalSessionSnapshot(
      _emptyState(),
      StudioSessionSnapshot(
        sessionId: 'session-1',
        throughSequence: 1,
        messages: const [],
        parts: const {},
        interactions: const [],
        agents: {
          'agent-1': StudioAgentView(
            id: 'agent-1',
            sessionId: 'session-1',
            path: 'root/worker',
            role: 'worker',
            task: 'Implement',
            status: 'running',
            summary: 'halfway',
            depth: 1,
            updatedAt: _fixtureDate(4),
          ),
        },
        timelineEvents: const {},
        runtime: null,
        turn: null,
      ),
    );

    expect(state.runtime.agentCount, 1);
    expect(state.selectedTimelineRows, isEmpty);
    expect(state.selectedAgents.single.id, 'agent-1');
    expect(state.selectedAgents.single.path, 'root/worker');
    expect(state.selectedAgents.single.summary, 'halfway');
  });

  test('canonical snapshot updates a background agent session', () {
    final state = applyCanonicalSessionSnapshot(
      _emptyState(),
      StudioSessionSnapshot(
        sessionId: 'session-background',
        throughSequence: 9,
        messages: const [],
        parts: const {},
        interactions: const [],
        agents: {
          'agent-background': StudioAgentView(
            id: 'agent-background',
            sessionId: 'session-background',
            path: '/root/executor',
            role: 'executor',
            task: 'Implement',
            status: 'running',
            summary: 'working',
            depth: 1,
            updatedAt: _fixtureDate(5),
          ),
        },
        timelineEvents: const {},
        runtime: null,
        turn: null,
      ),
    );

    expect(state.selectedSessionId, 'session-1');
    expect(
      state.agentsBySession['session-background']!.keys,
      {'agent-background'},
    );
    expect(state.runtimesBySession['session-background']!.agentCount, 1);
    expect(state.eventCursorsBySession['session-background'], 9);
    expect(
      state.workspaceSyncBySession['session-background'],
      AgentWorkspaceSyncState.ready,
    );
  });
}
