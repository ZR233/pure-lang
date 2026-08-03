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
      expect(state.composer.draft, isEmpty);
      expect(state.turn, isNull);
      expect(state.selectedMessages, isEmpty);
      expect(api.sessionSubscriptions, [
        (sessionId: 'session-1', afterSequence: null),
        (sessionId: 'session-1', afterSequence: null),
      ]);

      api.emitSession(
        _messageUpdatedEvent(
          sessionId: 'session-1',
          message: _timelineMessageFixture(
            id: 'turn-1:assistant',
            sessionId: 'session-1',
            turnId: 'turn-1',
            status: 'streaming',
          ),
        ),
      );
      api.emitSession(
        _partUpdatedEvent(
          sessionId: 'session-1',
          part: _timelinePartFixture(
            id: 'part-1',
            messageId: 'turn-1:assistant',
            sessionId: 'session-1',
            turnId: 'turn-1',
            type: TimelinePartType.text,
            status: 'streaming',
            text: 'hel',
          ),
        ),
      );
      api.emitSession(
        _partDeltaEvent(
          sessionId: 'session-1',
          delta: _timelineDeltaFixture(
            partId: 'part-1',
            revision: 1,
            field: 'text',
            delta: 'lo',
          ),
        ),
      );
      await _pumpFrameBatch();

      state = container.read(studioControllerProvider).requireValue;
      expect(state.selectedMessages.single.role, 'assistant');
      expect(state.selectedTimelineRows.single.part!.text, 'hello');
    },
  );

  test(
    'composer keeps one submit in flight until canonical turn start',
    () async {
      final api = _FakeStudioApi(_emptyState());
      final blocked = Completer<SubmitPromptReceipt>();
      api.blockedPromptSubmit = blocked;
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);
      controller.updateComposer('session-1', 'hello');

      final first = controller.submitComposer('session-1');
      await pumpEventQueue();
      var state = container.read(studioControllerProvider).requireValue;
      expect(state.composer.draft, 'hello');
      expect(state.composer.phase, ComposerSubmissionPhase.submitting);

      await controller.submitComposer('session-1');
      expect(api.submitPromptCount, 1);

      blocked.complete(
        const SubmitPromptReceipt(
          sessionId: 'session-1',
          turnId: 'turn-1',
          cursor: 1,
        ),
      );
      await first;
      state = container.read(studioControllerProvider).requireValue;
      expect(state.composer.draft, isEmpty);
      expect(state.composer.phase, ComposerSubmissionPhase.pendingStart);

      api.emitSession(
        _turnChangedEvent(
          sessionId: 'session-1',
          state: const StudioTurnState.inProgress(StudioTurnActivity.preparing),
        ),
      );
      await pumpEventQueue();
      state = container.read(studioControllerProvider).requireValue;
      expect(state.composer.phase, ComposerSubmissionPhase.idle);
    },
  );

  test('composer ignores a completed submit after provider disposal', () async {
    final api = _FakeStudioApi(_emptyState());
    final blocked = Completer<SubmitPromptReceipt>();
    api.blockedPromptSubmit = blocked;
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );

    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);
    controller.updateComposer('session-1', 'hello');
    final submission = controller.submitComposer('session-1');
    await pumpEventQueue();

    container.dispose();
    blocked.complete(
      const SubmitPromptReceipt(
        sessionId: 'session-1',
        turnId: 'turn-1',
        cursor: 1,
      ),
    );

    await expectLater(submission, completes);
  });

  test('composer ignores a failed submit after provider disposal', () async {
    final api = _FakeStudioApi(_emptyState());
    final blocked = Completer<SubmitPromptReceipt>();
    api.blockedPromptSubmit = blocked;
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );

    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);
    controller.updateComposer('session-1', 'hello');
    final submission = controller.submitComposer('session-1');
    await pumpEventQueue();

    container.dispose();
    blocked.completeError(Exception('bridge submit failed'));

    await expectLater(submission, completes);
  });

  test(
    'composer restores the exact draft and exposes submit failure',
    () async {
      final api = _FakeStudioApi(_emptyState())
        ..submitPromptError = Exception('bridge submit failed');
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);
      controller.updateComposer('session-1', '  exact prompt  ');
      await controller.submitComposer('session-1');

      var state = container.read(studioControllerProvider).requireValue;
      expect(state.composer.draft, '  exact prompt  ');
      expect(state.composer.phase, ComposerSubmissionPhase.idle);
      expect(state.composer.error, contains('bridge submit failed'));

      controller.updateComposer('session-1', 'retry prompt');
      state = container.read(studioControllerProvider).requireValue;
      expect(state.composer.draft, 'retry prompt');
      expect(state.composer.error, isNull);
    },
  );

  test('composer ignores stale submission revisions', () {
    final first = const ComposerSessionState.idle(
      draft: 'first',
    ).beginSubmission();
    final firstRevision = first.submissionRevision;
    final accepted = first.accept(
      const SubmitPromptReceipt(
        sessionId: 'session-1',
        turnId: 'turn-1',
        cursor: 1,
      ),
      submissionRevision: firstRevision,
    );
    final settled = accepted.observeTurn(
      _testTurn(
        sessionId: 'session-1',
        state: const StudioTurnState.inProgress(StudioTurnActivity.preparing),
      ),
    );
    final second = settled.updateDraft('second').beginSubmission();

    expect(second.submissionRevision, firstRevision + 1);
    expect(
      second.fail(
        Exception('stale failure'),
        submissionRevision: firstRevision,
      ),
      same(second),
    );
    expect(
      second.accept(
        const SubmitPromptReceipt(
          sessionId: 'session-1',
          turnId: 'stale-turn',
          cursor: 2,
        ),
        submissionRevision: firstRevision,
      ),
      same(second),
    );
  });

  test('composer rejects a receipt for another session', () async {
    final api = _FakeStudioApi(_emptyState())
      ..submitReceiptSessionId = 'session-other';
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);
    controller.updateComposer('session-1', 'exact prompt');
    await controller.submitComposer('session-1');

    final composer = container
        .read(studioControllerProvider)
        .requireValue
        .composer;
    expect(composer.phase, ComposerSubmissionPhase.idle);
    expect(composer.draft, 'exact prompt');
    expect(composer.error, contains('does not match session-1'));
  });

  test('paused Task resume is explicit and single-flight', () async {
    final api = _FakeStudioApi(_pausedTaskState());
    final blocked = Completer<SubmitPromptReceipt>();
    api.blockedTaskResume = blocked;
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);
    var state = container.read(studioControllerProvider).requireValue;
    expect(
      state.selectedAgentWorkspace!.rootSession.agentStatus,
      'interrupted',
    );
    expect(state.selectedAgentWorkspace!.runtime.task!.phase, 'implementing');
    expect(state.selectedAgentWorkspace!.isTaskPaused, isTrue);

    final first = controller.resumeTask('session-1');
    await pumpEventQueue();

    state = container.read(studioControllerProvider).requireValue;
    expect(state.composer.phase, ComposerSubmissionPhase.submitting);
    expect(api.resumeTaskCount, 1);
    expect(api.resumedTaskSessionIds, ['session-1']);
    expect(api.submittedPrompts, isEmpty);

    await controller.resumeTask('session-1');
    expect(api.resumeTaskCount, 1);

    blocked.complete(
      const SubmitPromptReceipt(
        sessionId: 'session-1',
        turnId: 'turn-1',
        cursor: 1,
      ),
    );
    await first;
    state = container.read(studioControllerProvider).requireValue;
    expect(state.composer.phase, ComposerSubmissionPhase.pendingStart);

    api.emitSession(
      _turnChangedEvent(
        sessionId: 'session-1',
        state: const StudioTurnState.inProgress(StudioTurnActivity.preparing),
      ),
    );
    await pumpEventQueue();
    state = container.read(studioControllerProvider).requireValue;
    expect(state.composer.phase, ComposerSubmissionPhase.idle);
  });

  test('canonical events reconcile composers for every session', () async {
    final childSubmitting = const ComposerSessionState.idle(
      draft: 'child prompt',
    ).beginSubmission();
    final childPending = childSubmitting.accept(
      const SubmitPromptReceipt(
        sessionId: 'session-2',
        turnId: 'turn-child',
        cursor: 1,
      ),
      submissionRevision: childSubmitting.submissionRevision,
    );
    final api = _FakeStudioApi(
      _emptyState().copyWith(
        composersBySession: {
          'session-1': const ComposerSessionState.idle(draft: 'planner draft'),
          'session-2': childPending,
        },
        turnsBySession: {
          'session-2': _testTurn(
            sessionId: 'session-2',
            turnId: 'turn-child',
            state: const StudioTurnState.inProgress(
              StudioTurnActivity.thinking,
            ),
          ),
        },
      ),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      _turnChangedEvent(
        sessionId: 'session-1',
        state: const StudioTurnState.inProgress(StudioTurnActivity.preparing),
      ),
    );
    await pumpEventQueue();

    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .composersBySession['session-2']!
          .phase,
      ComposerSubmissionPhase.idle,
    );
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .composersBySession['session-1']!
          .draft,
      'planner draft',
    );
  });

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
        message: _timelineMessageFixture(
          id: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          status: 'streaming',
        ),
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: _timelinePartFixture(
          id: 'part-1',
          messageId: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.text,
          order: 7,
          status: 'streaming',
          textChannel: TimelineTextChannel.commentary,
        ),
      ),
    );
    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: _timelineDeltaFixture(
          partId: 'part-1',
          revision: 0,
          field: 'text',
          delta: 'stale',
        ),
      ),
    );
    for (final revision in [1, 1, 2]) {
      api.emitSession(
        _partDeltaEvent(
          sessionId: 'session-1',
          delta: _timelineDeltaFixture(
            partId: 'part-1',
            revision: revision,
            field: 'text',
            delta: revision == 1 ? 'a' : 'b',
          ),
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
        part: _timelinePartFixture(
          id: 'part-1',
          messageId: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.text,
          order: 7,
          revision: 2,
          updatedAt: 2,
          textChannel: TimelineTextChannel.commentary,
          text: 'snapshot',
        ),
      ),
    );
    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: _timelineDeltaFixture(
          partId: 'part-1',
          revision: 3,
          field: 'text',
          delta: 'late',
        ),
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
        message: _timelineMessageFixture(
          id: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          status: 'streaming',
        ),
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: _timelinePartFixture(
          id: 'part-1',
          messageId: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.text,
          status: 'streaming',
          textChannel: TimelineTextChannel.finalAnswer,
        ),
      ),
    );
    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: _timelineDeltaFixture(
          partId: 'part-1',
          revision: 1,
          field: 'text',
          delta: 'live',
        ),
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
        message: _timelineMessageFixture(
          id: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          status: 'streaming',
        ),
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: _timelinePartFixture(
          id: 'part-1',
          messageId: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.text,
          status: 'streaming',
          textChannel: TimelineTextChannel.commentary,
        ),
      ),
    );

    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: _timelineDeltaFixture(
          partId: 'part-1',
          revision: 1,
          field: 'text',
          delta: 'v2',
        ),
      ),
    );
    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: _timelineDeltaFixture(
          partId: 'part-1',
          revision: 2,
          field: 'text',
          delta: '-safe',
        ),
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
        message: _timelineMessageFixture(
          id: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          status: 'streaming',
          createdAt: 10,
        ),
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
        part: _timelinePartFixture(
          id: 'part-1',
          messageId: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.text,
          order: 1,
          status: 'streaming',
          createdAt: 20,
          textChannel: TimelineTextChannel.finalAnswer,
        ),
      ),
    );
    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: _timelineDeltaFixture(
          partId: 'part-1',
          revision: 1,
          field: 'text',
          delta: 'projected only',
        ),
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
