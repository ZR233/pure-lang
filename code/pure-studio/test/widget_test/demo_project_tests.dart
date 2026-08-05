part of '../widget_test.dart';

void registerDemoProjectTests() {
  test('Demo bootstrap exposes independent Thread workspaces', () async {
    final state = await DemoStudioApi().bootstrap();

    expect(state.threads.map((thread) => thread.id), [
      'thread-main',
      'thread-reviewer',
      'thread-alt',
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

    final state = await api.setThreadMode(
      threadId: 'thread-main',
      mode: StudioMode.task,
    );

    expect(state.threads[0].mode, StudioMode.task);
    expect(state.threads[0].role, 'planner');
    expect(state.threads[1].mode, StudioMode.simple);
    expect(state.threads[1].role, 'reviewer');
    expect(state.threads[2].mode, StudioMode.simple);
    expect(state.threads[2].role, 'executor');
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
    final state = await DemoStudioApi().bootstrap();
    final api = _FakeStudioApi(
      state.copyWith(
        tasksByRootThread: const {
          'thread-main': TaskRuntimeView(
            runId: 'task-run-active',
            phase: 'implementing',
            branch: 'codex/task-mode',
            expectedHead: '1234567890abcdef',
            statusMessage: null,
            stopRequestedOrigin: null,
            stopRequestedReason: null,
            taskGeneration: 0,
            workUnits: [],
            completions: [],
            merges: [],
            reviews: [],
          ),
        },
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

    final receipt = await api.startTurn('thread-main', 'hello demo', const []);

    expect(receipt.threadId, 'thread-main');
    expect(frames.first, isA<ThreadSnapshotFrame>());
    expect(
      frames.whereType<ThreadNotificationFrame>().map((frame) => frame.update),
      containsAll([
        isA<ThreadTurnUpdate>(),
        isA<ThreadItemUpsert>(),
        isA<ThreadItemDeltaUpdate>(),
      ]),
    );
  });

  test('Driver demo interactions live in the Thread snapshot', () async {
    final api = DriverDemoStudioApi();
    final frame = await api.subscribeThread('thread-main').first;

    final snapshot = frame as ThreadSnapshotFrame;
    expect(snapshot.workspace.interactions.map((item) => item.kind), [
      InteractionKind.toolApproval,
      InteractionKind.userInput,
      InteractionKind.planConfirmation,
    ]);
  });
}
