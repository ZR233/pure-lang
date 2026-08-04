part of '../widget_test.dart';

void registerControllerStreamTests() {
  test('controller subscribes only the selected Thread', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();

    expect(api.threadSubscriptions, ['session-1']);
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi
          .subscriptionGeneration,
      greaterThan(0),
    );
  });

  test(
    'authoritative product snapshot removes a stale selected Task',
    () async {
      const staleTask = TaskRuntimeView(
        runId: 'task-stale',
        phase: 'implementing',
        branch: 'pure-task-stale',
        expectedHead: 'abc123',
        statusMessage: null,
        stopRequestedOrigin: null,
        stopRequestedReason: null,
        taskGeneration: 1,
        workUnits: [],
        completions: [],
        merges: [],
        reviews: [],
      );
      final initial = _twoProjectState(
        selectedProjectId: 'project-a',
      ).copyWith(tasksByRootThread: const {'session-b': staleTask});
      final api = _FakeStudioApi(initial);
      api.selectProjectStates['project-b'] = _twoProjectState(
        selectedProjectId: 'project-b',
      ).copyWith(tasksByRootThread: const {});
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      await container
          .read(studioControllerProvider.notifier)
          .selectProject('project-b');

      final next = container.read(studioControllerProvider).requireValue;
      expect(next.selectedThreadId, 'session-b');
      expect(next.tasksByRootThread, isNot(contains('session-b')));
    },
  );

  test('idle composer starts a Turn and busy composer steers it', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    final controller = container.read(studioControllerProvider.notifier);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    controller.updateComposer('session-1', 'first');
    await controller.submitComposer('session-1');
    expect(api.submittedPrompts.single.prompt, 'first');

    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: initial.selectedWorkspace!.copyWith(
          revision: 1,
          activeTurn: _testTurn(
            threadId: 'session-1',
            state: const StudioTurnState.inProgress(
              StudioTurnActivity.thinking,
            ),
            turnId: api.submitTurnId,
          ),
        ),
      ),
    );
    await pumpEventQueue();
    controller.updateComposer('session-1', 'steer');
    await controller.submitComposer('session-1');
    expect(api.submittedPrompts.last.prompt, 'steer');
    expect(api.submitPromptCount, 2);
  });

  test('TurnStarted clears the matching pending composer submission', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    final controller = container.read(studioControllerProvider.notifier);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    controller.updateComposer('session-1', 'hello');
    await controller.submitComposer('session-1');
    expect(
      container.read(studioControllerProvider).requireValue.composer.phase,
      ComposerSubmissionPhase.pendingStart,
    );

    api.emitThreadFrame(
      _threadTurnFrame(
        threadId: 'session-1',
        workspaceRevision: 1,
        state: const StudioTurnState.inProgress(StudioTurnActivity.preparing),
        turnId: api.submitTurnId,
      ),
    );
    await pumpEventQueue();

    expect(
      container.read(studioControllerProvider).requireValue.composer.phase,
      ComposerSubmissionPhase.idle,
    );
  });

  test('interrupt uses the exact active Turn identity', () async {
    final initial = _emptyState();
    final workspace = initial.selectedWorkspace!.copyWith(
      activeTurn: _testTurn(
        threadId: 'session-1',
        state: const StudioTurnState.inProgress(StudioTurnActivity.thinking),
        turnId: 'turn-active',
      ),
    );
    final api = _FakeStudioApi(
      initial.copyWith(workspacesByThread: {'session-1': workspace}),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await container.read(studioControllerProvider.notifier).stop('session-1');
    expect(api.interruptedTurn, (threadId: 'session-1', turnId: 'turn-active'));
  });

  test(
    'Lagged marks reconnecting and establishes a fresh generation',
    () async {
      final initial = _emptyState();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      await pumpEventQueue();
      final before = container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi
          .subscriptionGeneration;
      api.emitThreadFrame(
        const ThreadResyncRequiredFrame(threadId: 'session-1', dropped: 3),
      );
      await Future<void>.delayed(const Duration(milliseconds: 220));
      await pumpEventQueue();

      final after = container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi;
      expect(after.subscriptionGeneration, greaterThan(before));
      expect(api.threadSubscriptions.length, 2);
    },
  );

  test('Thread switch does not wait for old transport teardown', () async {
    final cancellation = Completer<void>();
    final api = _FakeStudioApi(_emptyState())
      ..blockedThreadCancellation = cancellation;
    final coordinator = ThreadStreamCoordinator(api, (_, _, _) {}, (_, _) {});
    addTearDown(() async {
      if (!cancellation.isCompleted) cancellation.complete();
      await coordinator.dispose();
    });

    coordinator.switchThread('session-1');
    await pumpEventQueue();
    coordinator.switchThread('session-2');
    await Future<void>.delayed(const Duration(milliseconds: 200));

    expect(api.threadSubscriptions.last, 'session-2');
    cancellation.complete();
  });

  test('history uses opaque cursor and merges by Item identity', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    api.historyPagesByThread['session-1'] = {
      null: ThreadHistoryPage(
        items: [
          _threadItemFixture(
            id: 'history-item',
            threadId: 'session-1',
            turnId: 'turn-old',
            ordinal: -1,
            text: 'older',
          ),
        ],
        nextCursor: 'opaque-next',
      ),
    };
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await container
        .read(studioControllerProvider.notifier)
        .loadOlderHistory('session-1');

    final state = container.read(studioControllerProvider).requireValue;
    expect(api.historyRequests.single.cursor, isNull);
    expect(state.selectedWorkspace!.items.single.text, 'older');
    expect(state.selectedWorkspaceUi.history.nextCursor, 'opaque-next');
  });

  test(
    'interaction response removes only the selected Thread request',
    () async {
      const interaction = PendingInteraction(
        id: 'interaction-1',
        threadId: 'session-1',
        kind: InteractionKind.userInput,
        title: 'Question',
        body: 'Continue?',
      );
      final initial = _emptyState();
      final workspace = initial.selectedWorkspace!.copyWith(
        interactions: const [interaction],
      );
      final api = _FakeStudioApi(
        initial.copyWith(workspacesByThread: {'session-1': workspace}),
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
            const UserInputResolutionCommand(answers: []),
          );

      expect(api.resolvedInteractionId, interaction.id);
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .selectedWorkspace!
            .interactions,
        isEmpty,
      );
    },
  );
}
