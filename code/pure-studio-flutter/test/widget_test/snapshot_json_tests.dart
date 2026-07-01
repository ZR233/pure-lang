part of '../widget_test.dart';

void registerSnapshotJsonTests() {
  test('typed session snapshot filters synthetic lifecycle parts', () {
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
        messages: [
          frb.BridgeStudioMessageProjectionDto(
            message: const frb.BridgeStudioMessageDto(
              messageId: 'turn-1:assistant',
              sessionId: 'session-1',
              turnId: 'turn-1',
              role: 'assistant',
              status: 'completed',
              createdAt: 1,
              updatedAt: 1,
            ),
            sequence: BigInt.from(1),
          ),
        ],
        parts: [
          frb.BridgeStudioPartProjectionDto(
            part_: _bridgePartDto(
              partId: 'turn-1',
              messageId: 'turn-1:assistant',
              partType: 'turn',
              text: 'turn lifecycle',
              synthetic: true,
            ),
            sequence: BigInt.from(2),
          ),
          frb.BridgeStudioPartProjectionDto(
            part_: _bridgePartDto(
              partId: 'turn-1-inf-1',
              messageId: 'turn-1:assistant',
              partType: 'inference',
              text: 'inference lifecycle',
              synthetic: true,
            ),
            sequence: BigInt.from(3),
          ),
          frb.BridgeStudioPartProjectionDto(
            part_: _bridgePartDto(
              partId: 'turn-1-final',
              messageId: 'turn-1:assistant',
              partType: 'text',
              text: 'visible answer',
              textChannel: 'final',
            ),
            sequence: BigInt.from(4),
          ),
        ],
        events: const [],
        eventNextSequence: BigInt.zero,
        agents: const [],
        agentEvents: const [],
        interactions: const [],
      ),
    );

    expect(state.partSnapshotsBySession['session-1']!.keys, {'turn-1-final'});
    expect(state.selectedTimelineRows.single.part!.text, 'visible answer');
  });

  test('session JSON keeps message snapshots free of projected parts', () {
    final state = studioStateFromSessionJson({
      'sessionId': 'session-1',
      'session': {
        'id': 'session-1',
        'projectId': 'project-1',
        'title': 'Session',
        'mode': 'auto',
        'updatedAt': 1,
      },
      'sessions': [
        {
          'id': 'session-1',
          'projectId': 'project-1',
          'title': 'Session',
          'mode': 'auto',
          'updatedAt': 1,
        },
      ],
      'messages': [
        {
          'sequence': 1,
          'message': {
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'role': 'assistant',
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 2,
          },
        },
      ],
      'parts': [
        {
          'sequence': 2,
          'part': {
            'partId': 'turn-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'partType': 'turn',
            'order': 0,
            'revision': 0,
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 2,
            'text': 'turn lifecycle',
            'synthetic': true,
          },
        },
        {
          'sequence': 3,
          'part': {
            'partId': 'turn-1-inf-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'partType': 'inference',
            'order': 1,
            'revision': 0,
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 2,
            'text': 'inference lifecycle',
            'synthetic': true,
          },
        },
        {
          'sequence': 4,
          'part': {
            'partId': 'part-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'partType': 'text',
            'order': 2,
            'revision': 0,
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 2,
            'textChannel': 'final',
            'text': 'restored from snapshot',
          },
        },
      ],
    });

    expect(state.messagesBySession['session-1']!.single.id, 'turn-1:assistant');
    expect(state.partSnapshotsBySession['session-1']!.keys, {'part-1'});
    expect(
      state.selectedTimelineRows.single.part!.text,
      'restored from snapshot',
    );
  });

  test('session JSON accepts legacy type and snake case timeline fields', () {
    final state = studioStateFromSessionJson({
      'sessionId': 'session-1',
      'session': {
        'id': 'session-1',
        'projectId': 'project-1',
        'title': 'Session',
        'mode': 'auto',
        'updatedAt': 1,
      },
      'sessions': [
        {
          'id': 'session-1',
          'projectId': 'project-1',
          'title': 'Session',
          'mode': 'auto',
          'updatedAt': 1,
        },
      ],
      'messages': [
        {
          'sequence': 1,
          'message': {
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'role': 'assistant',
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 2,
          },
        },
      ],
      'parts': [
        {
          'sequence': 2,
          'part': {
            'id': 'turn-1',
            'message_id': 'turn-1:assistant',
            'session_id': 'session-1',
            'turn_id': 'turn-1',
            'type': 'turn',
            'order': 0,
            'revision': 0,
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 2,
            'text': 'turn lifecycle',
            'synthetic': true,
          },
        },
        {
          'sequence': 3,
          'part': {
            'id': 'part-1',
            'message_id': 'turn-1:assistant',
            'session_id': 'session-1',
            'turn_id': 'turn-1',
            'type': 'text',
            'order': 1,
            'revision': 2,
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 2,
            'text_channel': 'final_answer',
            'text': 'legacy visible answer',
          },
        },
      ],
    });

    expect(state.partSnapshotsBySession['session-1']!.keys, {'part-1'});
    final part = state.selectedTimelineRows.single.part!;
    expect(part.textChannel, TimelineTextChannel.finalAnswer);
    expect(part.text, 'legacy visible answer');
    expect(part.sessionId, 'session-1');
    expect(part.turnId, 'turn-1');
    expect(part.revision, 2);
  });

  test('timeline parser accepts internal parts and rejects unknown values', () {
    final base = {
      'partId': 'part-1',
      'messageId': 'turn-1:assistant',
      'sessionId': 'session-1',
      'turnId': 'turn-1',
      'order': 0,
      'revision': 0,
      'status': 'completed',
      'createdAt': 1,
      'updatedAt': 2,
      'text': 'hello',
    };

    final message = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(1000),
    );
    for (final internalType in ['file', 'turn', 'inference']) {
      final internalPart = timelinePartSnapshotFromJson({
        ...base,
        'partType': internalType,
      });
      expect(isInternalTimelinePartType(internalPart.type), isTrue);
      expect(
        timelineRowsFromMessages(
          [message],
          parts: [timelinePartFromSnapshot(internalPart)],
        ),
        isEmpty,
      );
    }
    expect(
      () => timelinePartSnapshotFromJson({...base, 'partType': 'widget'}),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          contains('Unknown timeline part type'),
        ),
      ),
    );
    expect(
      () => timelinePartSnapshotFromJson({
        ...base,
        'partType': 'text',
        'textChannel': 'draft',
      }),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          contains('Unknown text channel'),
        ),
      ),
    );
  });

  test('realtime synthetic lifecycle part updates are ignored', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      StudioBridgeEvent.fromFrb(
        frb.BridgeEventEnvelope(
          eventId: 'event-1',
          sessionId: 'session-1',
          sequence: BigInt.from(1),
          createdAt: 1,
          payload: frb.BridgeEventPayload.messagePartUpdated(
            part_: _bridgePartDto(
              partId: 'turn-1',
              messageId: 'turn-1:assistant',
              partType: 'turn',
              text: 'turn lifecycle',
              synthetic: true,
            ),
          ),
        ),
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.partSnapshotsBySession['session-1'], isNull);
    expect(state.selectedTimelineRows, isEmpty);
    expect(state.eventCursorsBySession['session-1'], 1);
  });

  test('live stale events do not advance durable cursor', () async {
    final api = _FakeStudioApi(
      _emptyState().copyWith(eventCursorsBySession: const {'session-1': 1}),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      StudioBridgeEvent(
        sessionId: 'session-1',
        sequence: BigInt.from(9),
        payload: const StalePayload(laggedEvents: 1),
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.eventCursorsBySession['session-1'], 1);
  });

  test('studio bridge event normalizes FRB sealed payload', () {
    final event = StudioBridgeEvent.fromFrb(
      frb.BridgeEventEnvelope(
        eventId: 'event-1',
        sessionId: 'session-1',
        sequence: BigInt.from(9),
        createdAt: 1,
        payload: frb.BridgeEventPayload.messagePartDelta(
          delta: frb.BridgeStudioPartDeltaDto(
            partId: 'part-1',
            revision: BigInt.from(4),
            field: 'text',
            delta: 'typed',
          ),
        ),
      ),
    );

    final payload = event.payload;
    expect(payload, isA<MessagePartDeltaPayload>());
    expect((payload as MessagePartDeltaPayload).delta.delta, 'typed');
    expect(payload.delta.revision, 4);
  });

  test('FRB typed message part events reject unknown part types', () {
    expect(
      () => StudioBridgeEvent.fromFrb(
        frb.BridgeEventEnvelope(
          eventId: 'event-unknown-part',
          sessionId: 'session-1',
          sequence: BigInt.from(1),
          createdAt: 1,
          payload: frb.BridgeEventPayload.messagePartUpdated(
            part_: _bridgePartDto(
              partId: 'part-1',
              messageId: 'turn-1:assistant',
              partType: 'widget',
              text: 'should not enter reducer',
            ),
          ),
        ),
      ),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          contains('Unknown timeline part type'),
        ),
      ),
    );
  });

  test('timeline delta parser accepts only v2 fields', () {
    final base = {
      'sessionId': 'session-1',
      'messageId': 'message-1',
      'partId': 'part-1',
      'revision': 1,
      'delta': 'summary',
    };

    expect(
      timelinePartDeltaFromJson({...base, 'field': 'reasoning.summary'}).field,
      'reasoning.summary',
    );
    expect(
      () => timelinePartDeltaFromJson({...base, 'field': 'reasoningText'}),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          contains('Unknown timeline delta field'),
        ),
      ),
    );
  });
}
