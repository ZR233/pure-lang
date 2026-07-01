part of '../widget_test.dart';

void registerControllerStreamTests() {
  test(
    'composer submit waits for FRB events before timeline changes',
    () async {
      final api = _FakeStudioApi(_emptyState());
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      container.read(studioControllerProvider.notifier).updateComposer('hello');

      await container.read(studioControllerProvider.notifier).submitComposer();

      var state = container.read(studioControllerProvider).requireValue;
      expect(state.composerText, isEmpty);
      expect(state.turnPhase, TurnPhase.waitingForModel);
      expect(state.selectedMessages, isEmpty);

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
            'partType': 'text',
            'order': 0,
            'status': 'streaming',
            'createdAt': 1,
            'updatedAt': 1,
            'text': 'hel',
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
            'revision': 1,
            'field': 'text',
            'delta': 'lo',
          },
        ),
      );
      await _pumpFrameBatch();

      state = container.read(studioControllerProvider).requireValue;
      expect(state.selectedMessages.single.role, 'assistant');
      expect(state.selectedTimelineRows.single.part!.text, 'hello');
    },
  );

  test('timeline deltas use overlay revision guards', () async {
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
          'partType': 'text',
          'order': 7,
          'revision': 0,
          'status': 'streaming',
          'createdAt': 1,
          'updatedAt': 1,
          'textChannel': 'commentary',
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
          'revision': 0,
          'field': 'text',
          'delta': 'legacy',
        },
      ),
    );
    for (final revision in [1, 1, 2]) {
      api.emitSession(
        _partDeltaEvent(
          sessionId: 'session-1',
          delta: {
            'sessionId': 'session-1',
            'messageId': 'turn-1:assistant',
            'partId': 'part-1',
            'revision': revision,
            'field': 'text',
            'delta': revision == 1 ? 'a' : 'b',
          },
        ),
      );
    }
    await _pumpFrameBatch();

    var state = container.read(studioControllerProvider).requireValue;
    var part = state.selectedTimelineRows.single.part!;
    expect(part.text, 'ab');
    expect(part.order, 7);
    expect(part.textChannel, TimelineTextChannel.commentary);
    expect(state.partSnapshotsBySession['session-1']!['part-1']!.text, '');

    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'partType': 'text',
          'order': 7,
          'revision': 2,
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 2,
          'textChannel': 'commentary',
          'text': 'snapshot',
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
          'revision': 3,
          'field': 'text',
          'delta': 'late',
        },
      ),
    );
    await _pumpFrameBatch();

    state = container.read(studioControllerProvider).requireValue;
    part = state.selectedTimelineRows.single.part!;
    expect(part.text, 'snapshot');
    expect(state.partOverlaysBySession['session-1'], isEmpty);
  });

  test('durable events preserve flushed live part overlays', () async {
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
          'partType': 'text',
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
          'revision': 1,
          'field': 'text',
          'delta': 'live',
        },
      ),
    );
    api.emitSession(
      _turnChangedEvent(sessionId: 'session-1', status: 'streaming'),
    );
    await _pumpFrameBatch();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.turnPhase, TurnPhase.streaming);
    expect(state.selectedTimelineRows.single.part!.text, 'live');
    expect(
      state.partOverlaysBySession['session-1']!['part-1']!.values['text'],
      'live',
    );
  });

  test('timeline deltas route by envelope session and part snapshot', () async {
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
          'partType': 'text',
          'order': 0,
          'revision': 0,
          'status': 'streaming',
          'createdAt': 1,
          'updatedAt': 1,
          'textChannel': 'commentary',
          'text': '',
        },
      ),
    );

    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: {
          'partId': 'part-1',
          'revision': 1,
          'field': 'text',
          'delta': 'v2',
        },
      ),
    );
    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: {
          'sessionId': 'other-session',
          'messageId': 'other-message',
          'partId': 'part-1',
          'revision': 2,
          'field': 'text',
          'delta': '-safe',
        },
      ),
    );
    await _pumpFrameBatch();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedTimelineRows.single.part!.text, 'v2-safe');
    expect(state.partOverlaysBySession['other-session'], isNull);
  });

  test('part reducers leave message snapshots untouched', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      _messageUpdatedEvent(
        sessionId: 'session-1',
        sequence: BigInt.from(3),
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
    await _pumpFrameBatch();

    final before = container
        .read(studioControllerProvider)
        .requireValue
        .messagesBySession['session-1']!
        .single;

    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'partType': 'text',
          'order': 1,
          'revision': 0,
          'status': 'streaming',
          'createdAt': 20,
          'updatedAt': 20,
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
          'revision': 1,
          'field': 'text',
          'delta': 'projected only',
        },
      ),
    );
    await _pumpFrameBatch();

    final state = container.read(studioControllerProvider).requireValue;
    final after = state.messagesBySession['session-1']!.single;
    expect(identical(before, after), isTrue);
    expect(after.sequence, 3);
    expect(after.createdAt, DateTime.fromMillisecondsSinceEpoch(10000));
    expect(state.selectedTimelineRows.single.part!.text, 'projected only');
  });
}
