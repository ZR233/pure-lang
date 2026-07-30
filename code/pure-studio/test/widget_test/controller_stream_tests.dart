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
      container
          .read(studioControllerProvider.notifier)
          .updateComposer('session-1', 'hello');

      await container
          .read(studioControllerProvider.notifier)
          .submitComposer('session-1');

      var state = container.read(studioControllerProvider).requireValue;
      expect(state.composerText, isEmpty);
      expect(state.turn, isNull);
      expect(state.selectedMessages, isEmpty);
      expect(api.sessionSubscriptions, [
        (sessionId: 'session-1', afterSequence: null),
        (sessionId: 'session-1', afterSequence: null),
      ]);

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
          'type': 'text',
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
          'type': 'text',
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
          'revision': 1,
          'field': 'text',
          'delta': 'live',
        },
      ),
    );
    api.emitSession(
      _turnChangedEvent(
        sessionId: 'session-1',
        state: const StudioTurnState.inProgress(StudioTurnActivity.responding),
      ),
    );
    await _pumpFrameBatch();

    final state = container.read(studioControllerProvider).requireValue;
    expect(
      state.turn?.state,
      const StudioTurnState.inProgress(StudioTurnActivity.responding),
    );
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
          'type': 'text',
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
          'type': 'text',
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

  test(
    'resolved interaction rebuilds the canonical session load barrier',
    () async {
      final api = _FakeStudioApi(
        _emptyState().copyWith(
          pendingInteractions: const [
            PendingInteraction(
              id: 'interaction-plan',
              sessionId: 'session-1',
              kind: InteractionKind.planConfirmation,
              title: 'Confirm plan',
              body: '## Plan',
            ),
          ],
        ),
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      await container
          .read(studioControllerProvider.notifier)
          .resolveActiveInteraction(
            'session-1',
            const PlanConfirmationResolutionCommand(
              decision: PlanConfirmationDecision.implementFreshContext,
            ),
          );

      expect(api.sessionSubscriptions, [
        (sessionId: 'session-1', afterSequence: null),
        (sessionId: 'session-1', afterSequence: null),
      ]);
    },
  );

  test('explicit session selection ignores stale cached cursor', () async {
    final session2 = StudioSession(
      id: 'session-2',
      projectId: 'project-1',
      title: 'Second session',
      mode: StudioMode.simple,
      updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
    );
    final initial = _emptyState();
    final api = _FakeStudioApi(
      initial.copyWith(
        sessions: [...initial.sessions, session2],
        eventCursorsBySession: const {'session-2': 99},
      ),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await container
        .read(studioControllerProvider.notifier)
        .selectSession('session-2');

    expect(api.sessionSubscriptions.last, (
      sessionId: 'session-2',
      afterSequence: null,
    ));
  });

  test('session switch does not wait for old transport teardown', () async {
    final cancellation = Completer<void>();
    final api = _FakeStudioApi(_emptyState())
      ..blockedSessionCancellation = cancellation;
    final coordinator = SessionStreamCoordinator(api, (_, _, _) {}, (_, _) {});
    addTearDown(() async {
      if (!cancellation.isCompleted) {
        cancellation.complete();
      }
      await coordinator.dispose();
    });

    await coordinator.switchSession('session-1');
    await coordinator
        .switchSession('session-2')
        .timeout(const Duration(milliseconds: 200));

    expect(api.sessionSubscriptions.last.sessionId, 'session-2');
    cancellation.complete();
  });
}
