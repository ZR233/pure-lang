part of '../widget_test.dart';

void registerDemoProjectTests() {
  test('demo API emits prompt and assistant timeline events', () async {
    final api = DemoStudioApi();
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    container
        .read(studioControllerProvider.notifier)
        .updateComposer('session-main', 'demo hello');

    await container
        .read(studioControllerProvider.notifier)
        .submitComposer('session-main');
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.turnPhase, TurnPhase.completed);
    expect(
      state.selectedTimelineRows
          .where((row) => row.role == 'user')
          .last
          .part!
          .text,
      'demo hello',
    );
    expect(
      state.selectedTimelineRows
          .where((row) => row.role == 'assistant')
          .last
          .part!
          .text,
      contains('Demo response for'),
    );
    final reasoningGroup = state.selectedTimelineRows
        .where(
          (row) =>
              row.reasoningGroup != null &&
              row.messageId?.startsWith('demo-assistant-') == true,
        )
        .single
        .reasoningGroup!;
    expect(reasoningGroup.parts, hasLength(2));
    expect(reasoningGroup.summaries, [
      'Inspecting the request',
      'Preparing the tool call',
    ]);
    expect(
      state.selectedTimelineRows
          .where(
            (row) =>
                row.toolGroup != null &&
                row.messageId?.startsWith('demo-assistant-') == true,
          )
          .single
          .toolGroup!
          .status,
      'completed',
    );
  });

  test('demo API stores sample timeline as snapshots only', () async {
    final state = await DemoStudioApi().bootstrap();
    final sessionId = state.selectedSessionId!;

    expect(state.messagesBySession[sessionId], isNotEmpty);
    expect(state.partSnapshotsBySession[sessionId], isNotEmpty);
    expect(
      state.selectedTimelineRows
          .where((row) => row.role == 'assistant' && row.part != null)
          .map((row) => row.part!.id),
      contains('turn-demo:final-1'),
    );
  });

  test(
    'bootstrap hydrates selected session from subscription snapshot',
    () async {
      final api = _FakeStudioApi(
        _twoProjectState(selectedProjectId: 'project-a'),
      );
      api.sessionStates['session-a'] = _sessionHistoryState(
        projectId: 'project-a',
        sessionId: 'session-a',
        text: 'restored history from session a',
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      api.emitSessionFrame(
        _sessionSnapshotFrame(api.sessionStates['session-a']!),
      );
      await pumpEventQueue();
      final state = container.read(studioControllerProvider).requireValue;

      expect(api.loadedSessionIds, isEmpty);
      expect(api.sessionSubscriptions, [
        (sessionId: 'session-a', afterSequence: null),
      ]);
      expect(state.selectedSessionId, 'session-a');
      expect(
        state.selectedTimelineRows.single.part!.text,
        'restored history from session a',
      );
    },
  );

  test(
    'project selection switches subscription and applies its snapshot',
    () async {
      final api = _FakeStudioApi(
        _twoProjectState(selectedProjectId: 'project-a'),
      );
      api.selectProjectStates['project-b'] = _twoProjectState(
        selectedProjectId: 'project-b',
      );
      api.sessionStates['session-b'] = _sessionHistoryState(
        projectId: 'project-b',
        sessionId: 'session-b',
        text: 'restored history from session b',
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);

      await container
          .read(studioControllerProvider.notifier)
          .selectProject('project-b');
      api.emitSessionFrame(
        _sessionSnapshotFrame(api.sessionStates['session-b']!),
      );
      await pumpEventQueue();
      await _pumpFrameBatch();

      final state = container.read(studioControllerProvider).requireValue;
      expect(api.loadedSessionIds, isEmpty);
      expect(api.sessionSubscriptions.map((item) => item.sessionId), [
        'session-a',
        'session-b',
      ]);
      expect(state.selectedProjectId, 'project-b');
      expect(state.selectedSessionId, 'session-b');
      expect(
        state.selectedTimelineRows.single.part!.text,
        'restored history from session b',
      );
    },
  );

  test(
    'session snapshot cursor rejects older replay and accepts later events',
    () async {
      final api = _FakeStudioApi(
        _twoProjectState(selectedProjectId: 'project-a'),
      );
      api.selectProjectStates['project-b'] = _twoProjectState(
        selectedProjectId: 'project-b',
      );
      api.sessionStates['session-b'] = _sessionHistoryState(
        projectId: 'project-b',
        sessionId: 'session-b',
        text: 'old snapshot',
        eventCursor: 99,
        messageSequence: 1,
        partSequence: 2,
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);

      await container
          .read(studioControllerProvider.notifier)
          .selectProject('project-b');
      api.emitSessionFrame(
        _sessionSnapshotFrame(api.sessionStates['session-b']!),
      );
      await pumpEventQueue();

      api.emitSession(
        _messageUpdatedEvent(
          sessionId: 'session-b',
          sequence: BigInt.from(98),
          message: {
            'messageId': 'turn-stale:assistant',
            'sessionId': 'session-b',
            'turnId': 'turn-stale',
            'role': 'assistant',
            'status': 'streaming',
            'createdAt': 5,
            'updatedAt': 5,
          },
        ),
      );
      api.emitSession(
        _messageUpdatedEvent(
          sessionId: 'session-b',
          sequence: BigInt.from(100),
          message: {
            'messageId': 'turn-live:assistant',
            'sessionId': 'session-b',
            'turnId': 'turn-live',
            'role': 'assistant',
            'status': 'streaming',
            'createdAt': 5,
            'updatedAt': 5,
          },
        ),
      );
      api.emitSession(
        _partUpdatedEvent(
          sessionId: 'session-b',
          sequence: BigInt.from(101),
          part: {
            'partId': 'part-live',
            'sessionId': 'session-b',
            'messageId': 'turn-live:assistant',
            'turnId': 'turn-live',
            'type': 'text',
            'order': 6,
            'revision': 0,
            'status': 'streaming',
            'createdAt': 5,
            'updatedAt': 5,
            'textChannel': 'commentary',
            'text': 'live durable',
          },
        ),
      );
      await pumpEventQueue();

      final state = container.read(studioControllerProvider).requireValue;
      expect(state.selectedSessionId, 'session-b');
      expect(
        state.selectedMessages.where(
          (message) => message.id == 'turn-stale:assistant',
        ),
        isEmpty,
      );
      final liveRow = state.selectedTimelineRows.singleWhere(
        (row) => row.messageId == 'turn-live:assistant',
      );
      expect(liveRow.part!.text, 'live durable');
      expect(state.eventCursorsBySession['session-b'], 101);
    },
  );

  test(
    'session stream keeps live delta arrival order after snapshot',
    () async {
      final api = _FakeStudioApi(
        _twoProjectState(selectedProjectId: 'project-a'),
      );
      api.selectProjectStates['project-b'] = _twoProjectState(
        selectedProjectId: 'project-b',
      );
      api.sessionStates['session-b'] = _sessionHistoryState(
        projectId: 'project-b',
        sessionId: 'session-b',
        text: '',
        eventCursor: 20,
        messageSequence: 1,
        partSequence: 2,
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);

      await container
          .read(studioControllerProvider.notifier)
          .selectProject('project-b');
      api.emitSessionFrame(
        _sessionSnapshotFrame(api.sessionStates['session-b']!),
      );
      await pumpEventQueue();

      api.emitSession(
        _messageUpdatedEvent(
          sessionId: 'session-b',
          sequence: BigInt.from(21),
          message: {
            'messageId': 'turn-live:assistant',
            'sessionId': 'session-b',
            'turnId': 'turn-live',
            'role': 'assistant',
            'status': 'streaming',
            'createdAt': 5,
            'updatedAt': 5,
          },
        ),
      );
      api.emitSession(
        _partUpdatedEvent(
          sessionId: 'session-b',
          sequence: BigInt.from(22),
          part: {
            'partId': 'part-live',
            'sessionId': 'session-b',
            'messageId': 'turn-live:assistant',
            'turnId': 'turn-live',
            'type': 'text',
            'order': 0,
            'revision': 0,
            'status': 'streaming',
            'createdAt': 5,
            'updatedAt': 5,
            'textChannel': 'final',
            'text': '',
          },
        ),
      );
      api.emitSession(
        _partDeltaEvent(
          sessionId: 'session-b',
          sequence: BigInt.from(23),
          eventId: 'z-revision-1',
          createdAt: DateTime.fromMillisecondsSinceEpoch(5000),
          delta: {
            'sessionId': 'session-b',
            'messageId': 'turn-live:assistant',
            'partId': 'part-live',
            'revision': 1,
            'field': 'text',
            'delta': 'a',
          },
        ),
      );
      api.emitSession(
        _partDeltaEvent(
          sessionId: 'session-b',
          sequence: BigInt.from(23),
          eventId: 'a-revision-2',
          createdAt: DateTime.fromMillisecondsSinceEpoch(5000),
          delta: {
            'sessionId': 'session-b',
            'messageId': 'turn-live:assistant',
            'partId': 'part-live',
            'revision': 2,
            'field': 'text',
            'delta': 'b',
          },
        ),
      );
      await pumpEventQueue();
      await _pumpFrameBatch();

      final state = container.read(studioControllerProvider).requireValue;
      final liveRow = state.selectedTimelineRows.singleWhere(
        (row) => row.messageId == 'turn-live:assistant',
      );
      expect(liveRow.part!.text, 'ab');
      expect(state.partOverlaysBySession['session-b']!['part-live']!.values, {
        'text': 'ab',
      });
    },
  );

  test('archive project switches project and session subscription', () async {
    final api = _FakeStudioApi(
      _twoProjectState(selectedProjectId: 'project-a'),
    );
    api.archiveProjectStates['project-a'] = _twoProjectState(
      selectedProjectId: 'project-b',
      projects: const [
        StudioProject(id: 'project-b', name: 'Project B', path: 'b'),
      ],
    );
    api.sessionStates['session-b'] = _sessionHistoryState(
      projectId: 'project-b',
      sessionId: 'session-b',
      text: 'history after project close',
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    await container
        .read(studioControllerProvider.notifier)
        .archiveProject('project-a');
    api.emitSessionFrame(
      _sessionSnapshotFrame(api.sessionStates['session-b']!),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(api.archivedProjectId, 'project-a');
    expect(api.archiveSelectedProjectId, 'project-a');
    expect(api.loadedSessionIds, isEmpty);
    expect(api.sessionSubscriptions.map((item) => item.sessionId), [
      'session-a',
      'session-b',
    ]);
    expect(state.projects.map((project) => project.id), ['project-b']);
    expect(state.selectedProjectId, 'project-b');
    expect(
      state.selectedTimelineRows.single.part!.text,
      'history after project close',
    );
  });

  test('closed session stream resubscribes from canonical snapshot', () async {
    final api = _FakeStudioApi(
      _twoProjectState(selectedProjectId: 'project-a'),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    expect(api.sessionSubscriptions, hasLength(1));
    await api.closeSessionStream();
    await Future<void>.delayed(const Duration(milliseconds: 200));
    await pumpEventQueue();

    expect(api.sessionSubscriptions, hasLength(2));
    expect(api.sessionSubscriptions.last.sessionId, 'session-a');
    expect(api.sessionSubscriptions.last.afterSequence, isNull);
  });

  test('archive last project clears current selection', () async {
    final api = _FakeStudioApi(
      _twoProjectState(selectedProjectId: 'project-a'),
    );
    api.archiveProjectStates['project-a'] = _noProjectState();
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    await container
        .read(studioControllerProvider.notifier)
        .archiveProject('project-a');

    final state = container.read(studioControllerProvider).requireValue;
    expect(api.archivedProjectId, 'project-a');
    expect(state.projects, isEmpty);
    expect(state.sessions, isEmpty);
    expect(state.selectedProjectId, isNull);
    expect(state.selectedSessionId, isNull);
    expect(state.selectedMessages, isEmpty);
  });
}
