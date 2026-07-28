part of '../widget_test.dart';

void registerReducerRecoveryTests() {
  test('studio state copyWith can explicitly clear nullable fields', () {
    final state = _stateWithPlannerModels().copyWith(
      defaultProviderId: 'deepseek',
    );

    final cleared = state.copyWith(
      defaultProviderId: null,
      selectedProjectId: null,
      selectedSessionId: null,
    );

    expect(cleared.defaultProviderId, isNull);
    expect(cleared.selectedProjectId, isNull);
    expect(cleared.selectedSessionId, isNull);
    expect(cleared.projects, state.projects);
    expect(cleared.sessions, state.sessions);
  });

  test(
    'timeline delta revision gaps clear overlay and recover session',
    () async {
      final recovered = _emptyState().copyWith(
        messagesBySession: {
          'session-1': [
            TimelineMessage(
              id: 'turn-1:assistant',
              sessionId: 'session-1',
              role: 'assistant',
              createdAt: DateTime.fromMillisecondsSinceEpoch(1),
              sequence: 1,
            ),
          ],
        },
        partSnapshotsBySession: {
          'session-1': {
            'part-1': TimelinePartSnapshot(
              id: 'part-1',
              messageId: 'turn-1:assistant',
              sessionId: 'session-1',
              turnId: 'turn-1',
              type: TimelinePartType.text,
              order: 0,
              revision: 3,
              sequence: 2,
              text: 'restored',
              status: 'streaming',
              createdAt: DateTime.fromMillisecondsSinceEpoch(1),
              updatedAt: DateTime.fromMillisecondsSinceEpoch(2),
              textChannel: TimelineTextChannel.finalAnswer,
            ),
          },
        },
      );
      final api = _FakeStudioApi(_emptyState());
      api.sessionStates['session-1'] = recovered;
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      api.emitSession(
        _messageUpdatedEvent(
          sessionId: 'session-1',
          message: {
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'role': 'assistant',
            'status': 'streaming',
            'createdAt': 1,
            'updatedAt': 1,
          },
        ),
      );
      api.emitSession(
        _partUpdatedEvent(
          sessionId: 'session-1',
          part: {
            'partId': 'part-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'type': 'text',
            'order': 0,
            'revision': 0,
            'status': 'streaming',
            'createdAt': 1,
            'updatedAt': 1,
            'textChannel': 'final',
            'text': '',
          },
        ),
      );
      api.emitSession(
        _partDeltaEvent(
          sessionId: 'session-1',
          delta: {
            'sessionId': 'session-1',
            'messageId': 'turn-1:assistant',
            'partId': 'part-1',
            'revision': 2,
            'field': 'text',
            'delta': 'skipped',
          },
        ),
      );
      await _pumpFrameBatch();
      await pumpEventQueue();

      expect(api.sessionSubscriptions.last, (
        sessionId: 'session-1',
        afterSequence: null,
      ));
      api.emitSessionFrame(_sessionSnapshotFrame(recovered));
      await pumpEventQueue();
      final state = container.read(studioControllerProvider).requireValue;
      expect(api.loadedSessionIds, isEmpty);
      expect(state.partOverlaysBySession['session-1'], isEmpty);
      expect(state.selectedTimelineRows.single.part!.text, 'restored');
    },
  );

  test('part snapshots reject identity and terminal regressions', () async {
    final recovered = _emptyState().copyWith(
      messagesBySession: {
        'session-1': [
          TimelineMessage(
            id: 'turn-1:assistant',
            sessionId: 'session-1',
            role: 'assistant',
            createdAt: DateTime.fromMillisecondsSinceEpoch(1),
            sequence: 1,
          ),
        ],
      },
      partSnapshotsBySession: {
        'session-1': {
          'part-1': TimelinePartSnapshot(
            id: 'part-1',
            messageId: 'turn-1:assistant',
            sessionId: 'session-1',
            turnId: 'turn-1',
            type: TimelinePartType.text,
            order: 0,
            revision: 3,
            sequence: 2,
            text: 'recovered',
            status: 'completed',
            createdAt: DateTime.fromMillisecondsSinceEpoch(1),
            updatedAt: DateTime.fromMillisecondsSinceEpoch(3),
            completedAt: DateTime.fromMillisecondsSinceEpoch(3),
            textChannel: TimelineTextChannel.finalAnswer,
          ),
        },
      },
    );
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      _messageUpdatedEvent(
        sessionId: 'session-1',
        message: {
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'role': 'assistant',
          'status': 'streaming',
          'createdAt': 1,
          'updatedAt': 1,
        },
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'type': 'text',
          'order': 0,
          'revision': 2,
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 2,
          'completedAt': 2,
          'textChannel': 'final',
          'text': 'stable',
        },
      ),
    );
    await pumpEventQueue();

    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'type': 'text',
          'order': 9,
          'revision': 2,
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 2,
          'completedAt': 2,
          'textChannel': 'final',
          'text': 'wrong order',
        },
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'type': 'text',
          'order': 0,
          'revision': 1,
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 2,
          'completedAt': 2,
          'textChannel': 'final',
          'text': 'low revision',
        },
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'type': 'text',
          'order': 0,
          'revision': 2,
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 2,
          'completedAt': 2,
          'textChannel': 'final',
          'text': 'changed terminal',
        },
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedTimelineRows.single.part!.text, 'stable');
    expect(state.partSnapshotsBySession['session-1']!['part-1']!.order, 0);
    expect(api.sessionSubscriptions.last, (
      sessionId: 'session-1',
      afterSequence: null,
    ));
    api.emitSessionFrame(_sessionSnapshotFrame(recovered));
    await pumpEventQueue();
    final recoveredState = container
        .read(studioControllerProvider)
        .requireValue;
    expect(recoveredState.selectedTimelineRows.single.part!.text, 'recovered');
  });

  test('message snapshots keep original createdAt', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      _messageUpdatedEvent(
        sessionId: 'session-1',
        sequence: BigInt.one,
        message: {
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'role': 'assistant',
          'status': 'streaming',
          'createdAt': 10,
          'updatedAt': 10,
        },
      ),
    );
    api.emitSession(
      _messageUpdatedEvent(
        sessionId: 'session-1',
        sequence: BigInt.two,
        message: {
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'role': 'assistant',
          'status': 'completed',
          'createdAt': 20,
          'updatedAt': 20,
          'completedAt': 20,
        },
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'type': 'text',
          'order': 0,
          'revision': 0,
          'status': 'streaming',
          'createdAt': 10,
          'updatedAt': 10,
          'textChannel': 'final',
          'text': 'still streaming',
        },
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    final message = state.selectedMessages.single;
    expect(message.createdAt, DateTime.fromMillisecondsSinceEpoch(10000));
    expect(message.updatedAt, DateTime.fromMillisecondsSinceEpoch(20000));
    expect(message.completedAt, DateTime.fromMillisecondsSinceEpoch(20000));
    expect(message.status, 'completed');
    expect(message.turnId, 'turn-1');
    expect(message.sequence, 2);
    expect(state.selectedTimelineRows.single.part!.status, 'streaming');
  });

  test('timeline drops orphan part snapshots', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-orphan',
          'messageId': 'missing-message',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'type': 'text',
          'order': 0,
          'revision': 0,
          'status': 'streaming',
          'createdAt': 1,
          'updatedAt': 1,
          'text': 'orphan',
        },
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedMessages, isEmpty);
    expect(state.partSnapshotsBySession['session-1'], isNull);
    expect(state.selectedTimelineRows, isEmpty);
  });
}
