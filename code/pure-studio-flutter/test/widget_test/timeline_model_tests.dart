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

  test('timeline reasoning part preserves text and render version', () {
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
    expect(first.part!.text, 'alpha');
    expect(first.renderVersion, isNot(second.renderVersion));
  });

  test('timeline groups tool parts by activityGroupId', () {
    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final firstMessage = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      turnId: 'turn-1',
      role: 'assistant',
      createdAt: now,
      sequence: 1,
    );
    final secondMessage = TimelineMessage(
      id: 'turn-2:assistant',
      sessionId: 'session-1',
      turnId: 'turn-2',
      role: 'assistant',
      createdAt: now,
      sequence: 10,
    );
    final rows = timelineRowsFromMessages(
      [firstMessage, secondMessage],
      parts: [
        _toolTimelinePart(
          id: 'tool-a',
          messageId: firstMessage.id,
          turnId: 'turn-1',
          order: 2,
          sequence: 3,
          name: 'read_file',
          activityGroupId: 'tool-group:turn-1:2',
          result: 'ok',
        ),
        TimelinePart(
          id: 'final-a',
          messageId: firstMessage.id,
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.text,
          text: 'answer',
          textChannel: TimelineTextChannel.finalAnswer,
          order: 4,
          sequence: 5,
        ),
        _toolTimelinePart(
          id: 'tool-b',
          messageId: firstMessage.id,
          turnId: 'turn-1',
          order: 6,
          sequence: 7,
          name: 'search_files',
          activityGroupId: 'tool-group:turn-1:2',
          result: 'matches',
        ),
        _toolTimelinePart(
          id: 'tool-d',
          messageId: firstMessage.id,
          turnId: 'turn-1b',
          order: 8,
          sequence: 9,
          name: 'write_file',
          activityGroupId: 'tool-group:turn-1:8',
          status: 'denied',
        ),
        _toolTimelinePart(
          id: 'tool-c',
          messageId: secondMessage.id,
          turnId: 'turn-2',
          order: 0,
          sequence: 11,
          name: 'bash',
          activityGroupId: 'tool-group:turn-2:0',
          status: 'running',
        ),
      ],
    );

    final toolGroups = rows
        .where((row) => row.type == TimelineRowType.toolGroup)
        .toList();
    expect(toolGroups, hasLength(3));
    expect(toolGroups.first.id, 'tool-group:turn-1:2');
    expect(toolGroups.first.order, 2);
    expect(toolGroups.first.sequence, 7);
    expect(toolGroups.first.toolGroup!.items.map((item) => item.id), [
      'tool-a',
      'tool-b',
    ]);
    expect(toolGroups.first.toolGroup!.status, 'completed');
    expect(toolGroups[1].id, 'tool-group:turn-1:8');
    expect(toolGroups[1].toolGroup!.status, 'denied');
    expect(toolGroups.last.toolGroup!.status, 'running');
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
            name: 'bash',
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
      expect(rows[1].id, 'tool-group:turn-1:1');
      expect(rows[1].toolGroup!.items.map((item) => item.id), [
        'tool-a',
        'tool-b',
      ]);
      expect(rows[3].id, 'tool-group:turn-1:4');
      expect(rows[3].toolGroup!.items.single.id, 'tool-c');
    },
  );

  test('timeline keeps tools without activityGroupId as singleton groups', () {
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

    expect(rows, hasLength(2));
    expect(rows.map((row) => row.toolGroup!.items.single.id), [
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
            name: 'bash',
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
            'type': 'spawnBegin',
            'callId': 'call-1',
            'senderPath': 'root',
            'taskName': 'Audit',
            'prompt': 'check',
            'role': 'reviewer',
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
            'type': 'spawnEnd',
            'callId': 'call-1',
            'senderPath': 'root',
            'path': 'root/reviewer',
            'status': 'completed',
            'prompt': 'check',
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
    expect(row.agentEvent!.payload, isA<TimelineAgentSpawnEnd>());
  });

  test('agent snapshots update status state without timeline rows', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      StudioBridgeEvent.fromFrb(
        frb.BridgeEventEnvelope(
          eventId: 'agent-snapshot-1',
          sessionId: 'session-1',
          sequence: BigInt.from(1),
          createdAt: 1,
          payload: frb.BridgeEventPayload.agentChanged(
            agent: frb.BridgeAgentSnapshotDto(
              id: 'agent-1',
              sessionId: 'session-1',
              path: 'root/reviewer',
              role: 'reviewer',
              task: 'Audit timeline',
              status: 'running',
              depth: 1,
              updatedAt: 1,
            ),
          ),
        ),
      ),
    );
    api.emitSession(
      StudioBridgeEvent.fromFrb(
        frb.BridgeEventEnvelope(
          eventId: 'agent-snapshot-2',
          sessionId: 'session-1',
          sequence: BigInt.from(2),
          createdAt: 2,
          payload: frb.BridgeEventPayload.agentChanged(
            agent: frb.BridgeAgentSnapshotDto(
              id: 'agent-1',
              sessionId: 'session-1',
              path: 'root/reviewer',
              role: 'reviewer',
              task: 'Audit timeline',
              status: 'completed',
              summary: 'done',
              depth: 1,
              updatedAt: 2,
            ),
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

  test(
    'typed session snapshot restores agent timeline events for projection',
    () {
      const session = frb.SessionDto(
        id: 'session-1',
        projectId: 'project-1',
        title: 'Session',
        mode: 'auto',
        updatedAt: 1,
        visibility: 'visible',
      );
      final state = studioStateFromFrbSession(
        frb.BridgeSessionStateResponse(
          sessionId: 'session-1',
          session: session,
          sessions: const [session],
          messages: const [],
          parts: const [],
          events: const [],
          eventNextSequence: BigInt.zero,
          agents: const [],
          agentEvents: [
            frb.BridgeAgentTimelineEventDto(
              eventId: 'agent-event-1',
              sessionId: 'session-1',
              sequence: BigInt.from(7),
              createdAt: 3,
              payload: const frb.BridgeAgentTimelinePayloadDto.interactionBegin(
                callId: 'call-2',
                senderPath: 'root',
                receiverPath: 'root/worker',
                prompt: 'status',
              ),
            ),
          ],
          interactions: const [],
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
      expect(row.agentEvent!.payload, isA<TimelineAgentInteractionBegin>());
    },
  );

  test('typed session snapshot restores agent status state', () {
    const session = frb.SessionDto(
      id: 'session-1',
      projectId: 'project-1',
      title: 'Session',
      mode: 'auto',
      updatedAt: 1,
      visibility: 'visible',
    );
    final state = studioStateFromFrbSession(
      frb.BridgeSessionStateResponse(
        sessionId: 'session-1',
        session: session,
        sessions: const [session],
        messages: const [],
        parts: const [],
        events: const [],
        eventNextSequence: BigInt.zero,
        agents: const [
          frb.BridgeAgentSnapshotDto(
            id: 'agent-1',
            sessionId: 'session-1',
            path: 'root/worker',
            role: 'worker',
            task: 'Implement',
            status: 'running',
            summary: 'halfway',
            depth: 1,
            updatedAt: 4,
          ),
        ],
        agentEvents: const [],
        interactions: const [],
      ),
    );

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
