part of '../widget_test.dart';

void registerDemoProjectTests() {
  test('Demo read exposes independent Thread workspaces', () async {
    final state = await DemoStudioApi().readStudioState();

    // 目录窗口按 updatedAt 倒序：main(now) → alt(-3m) → reviewer(-7m)。
    expect(state.threads.map((thread) => thread.id), [
      'thread-main',
      'thread-alt',
      'thread-reviewer',
    ]);
    expect(
      state.workspacesByThread.keys,
      containsAll(state.threads.map((e) => e.id)),
    );
    expect(state.workspacesByThread['thread-main']!.items, isNotEmpty);
    expect(
      state.workspacesByThread['thread-reviewer']!.runtime.model,
      'reviewer/model',
    );
  });

  test('Demo mode update changes only the addressed root Thread', () async {
    final api = DemoStudioApi();

    await api.setThreadMode(threadId: 'thread-main', mode: StudioMode.task);
    final state = await api.readStudioState();

    expect(state.threads[0].mode, StudioMode.task);
    expect(state.threads[0].role, 'planner');
    expect(state.threads[1].mode, StudioMode.simple);
    expect(state.threads[1].role, 'executor');
    expect(state.threads[2].mode, StudioMode.simple);
    expect(state.threads[2].role, 'reviewer');
    expect(state.workspacesByThread['thread-main']!.thread, state.threads[0]);
  });

  test('Demo mode update rejects a child Thread', () async {
    final api = DemoStudioApi();

    await expectLater(
      api.setThreadMode(threadId: 'thread-reviewer', mode: StudioMode.task),
      throwsA(
        isA<StateError>().having(
          (error) => error.message,
          'message',
          'only a root Thread can change mode',
        ),
      ),
    );
  });

  test('Fake mode update rejects an active Task', () async {
    final state = await DemoStudioApi().readStudioState();
    final api = _FakeStudioApi(
      state.copyWith(
        taskDirectory: TaskDirectoryState(
          values: [
            TaskDirectoryEntryView(
              rootThreadId: 'thread-main',
              task: TaskRuntimeView(
                runId: 'task-run-active',
                state: const WorkingTaskStateView(
                  documentEditSummary: 'test documents updated',
                ),
                revision: 0,
                generation: 0,
                workUnits: [],
                completions: [],
                merges: [],
                reviews: [],
              ),
            ),
          ],
        ),
      ),
    );

    await expectLater(
      api.setThreadMode(threadId: 'thread-main', mode: StudioMode.task),
      throwsA(
        isA<StateError>().having(
          (error) => error.message,
          'message',
          'thread mode cannot change while a task is active',
        ),
      ),
    );
    expect(api.modeUpdate, isNull);
  });

  test('Demo startTurn publishes typed Turn and Item notifications', () async {
    final api = DemoStudioApi();
    final frames = <ThreadStreamFrame>[];
    final subscription = api.subscribeThread('thread-main').listen(frames.add);
    addTearDown(subscription.cancel);
    await pumpEventQueue();

    final receipt = await api.startTurn(
      'thread-main',
      const StudioPromptInput(text: 'hello demo', attachmentDraftIds: []),
    );

    expect(receipt.threadId, 'thread-main');
    expect(frames.first, isA<ThreadSnapshotFrame>());
    final deadline = DateTime.now().add(const Duration(seconds: 5));
    while (DateTime.now().isBefore(deadline) &&
        !frames.whereType<ThreadNotificationFrame>().any((frame) {
          final update = frame.update;
          return update is ThreadTurnUpdate &&
              update.turn.state is CompletedStudioTurnState;
        })) {
      await Future<void>.delayed(const Duration(milliseconds: 25));
    }
    expect(
      frames.whereType<ThreadNotificationFrame>().map((frame) => frame.update),
      containsAll([
        isA<ThreadTurnUpdate>(),
        isA<ThreadItemUpsert>(),
        isA<ThreadItemDeltaUpdate>(),
      ]),
    );
    final deltaTypes = frames
        .whereType<ThreadNotificationFrame>()
        .map((frame) => frame.update)
        .whereType<ThreadItemDeltaUpdate>()
        .map((update) => update.delta.state.runtimeType)
        .toSet();
    expect(
      deltaTypes,
      containsAll(<Type>[
        ThreadTextDeltaView,
        ThreadThinkingSummaryDeltaView,
        ThreadThinkingContentDeltaView,
        ThreadToolArgumentsDeltaView,
        ThreadToolResultDeltaView,
        ThreadPlanDeltaView,
      ]),
    );
    final snapshot =
        await api.subscribeThread('thread-main').first as ThreadSnapshotFrame;
    final turnItems = snapshot.workspace.items.where(
      (item) => item.turnId == receipt.turnId,
    );
    expect(turnItems, isNotEmpty);
    expect(turnItems.every(_demoItemIsTerminal), isTrue);
  });

  test('Driver demo interactions live in the Thread snapshot', () async {
    final api = DriverDemoStudioApi();
    final frame = await api.subscribeThread('thread-main').first;

    final snapshot = frame as ThreadSnapshotFrame;
    expect(
      snapshot.workspace.interactions.map(
        (item) => (item.id, item.kind, item.turnId),
      ),
      [
        ('driver-tool', InteractionKind.toolApproval, 'driver-origin-turn'),
        ('driver-input', InteractionKind.userInput, 'driver-origin-turn'),
        (
          'driver-plan-revise',
          InteractionKind.planConfirmation,
          'driver-origin-turn',
        ),
        (
          'driver-plan-confirm',
          InteractionKind.planConfirmation,
          'driver-origin-turn',
        ),
      ],
    );
  });

  test('Demo LSP activity loop publishes indexing then idle', () async {
    final api = _FastLspDemoApi();
    final events = <StudioBridgeEvent>[];
    final subscription = api.subscribeProductEvents().listen((event) {
      if (event is StudioBridgeEvent) events.add(event);
    });
    addTearDown(subscription.cancel);

    for (var i = 0; i < 100 && events.length < 6; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 20));
    }

    final states = [
      for (final event in events.take(6))
        (event.payload as LspStateChangedPayload).state,
    ];
    expect(states, hasLength(6));
    expect(
      states.take(5).map((state) {
        final serverState = state.servers.single.state;
        return switch (serverState) {
          LspAvailableState(activity: LspIndexingActivity(:final percentage)) =>
            percentage,
          LspCheckingState() ||
          LspAvailableState() ||
          LspUnavailableState() ||
          LspDisabledState() => null,
        };
      }),
      [40, 55, 70, 85, 100],
    );
    expect(
      states.take(5).map((state) => state.servers.single.state),
      everyElement(
        isA<LspAvailableState>().having(
          (state) => state.activity,
          'activity',
          isA<LspIndexingActivity>(),
        ),
      ),
    );
    expect(states[5].servers, isEmpty);
    final revisions = states.map((state) => state.revision).toList();
    expect(revisions.toSet().length, revisions.length);
  });
}

bool _demoItemIsTerminal(ThreadItemView item) => switch (item.state) {
  ThreadTextItemStateView(:final lifecycle) => lifecycle.isTerminal,
  ThreadThinkingItemStateView(:final lifecycle) => lifecycle.isTerminal,
  ThreadPlanItemStateView(:final lifecycle) => lifecycle.isTerminal,
  ThreadToolItemStateView(:final lifecycle) => lifecycle.isTerminal,
  _ => true,
};

class _FastLspDemoApi extends DemoStudioApi {
  _FastLspDemoApi() : super(lspActivityLoop: true);

  @override
  Duration get lspActivityStepDelay => const Duration(milliseconds: 40);
}
