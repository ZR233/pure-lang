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

    await api.setThreadMode(threadId: 'thread-main', mode: ThreadModeId.task);
    final state = await api.readStudioState();

    expect(state.threads[0].mode, ThreadModeId.task);
    expect(state.threads[0].role, 'planner');
    expect(state.threads[1].mode, ThreadModeId.simple);
    expect(state.threads[1].role, 'planner');
    expect(state.threads[2].mode, ThreadModeId.simple);
    expect(state.threads[2].role, 'reviewer');
    expect(state.workspacesByThread['thread-main']!.thread, state.threads[0]);
  });

  test('Demo mode update rejects a child Thread', () async {
    final api = DemoStudioApi();

    await expectLater(
      api.setThreadMode(threadId: 'thread-reviewer', mode: ThreadModeId.task),
      throwsA(
        isA<StateError>().having(
          (error) => error.message,
          'message',
          'only a root Thread can change mode',
        ),
      ),
    );
  });

  test(
    'Demo project directory revision advances with project mutations',
    () async {
      final api = DemoStudioApi();
      final initial = await api.readStudioState();
      final unchanged = await api.readStudioState();

      expect(
        unchanged.projectDirectory.revision,
        initial.projectDirectory.revision,
      );

      await api.openRemoteProject('demo-ssh', '/home/projects');
      final withRemote = await api.readStudioState();
      expect(
        withRemote.projectDirectory.revision,
        initial.projectDirectory.revision + 1,
      );
      expect(withRemote.projects.map((project) => project.id), [
        'project-local',
        'project-remote',
      ]);

      await api.openRemoteProject('demo-ssh', '/home/projects');
      final sameRemote = await api.readStudioState();
      expect(
        sameRemote.projectDirectory.revision,
        withRemote.projectDirectory.revision,
      );

      await api.archiveProject('project-local');
      final archived = await api.readStudioState();
      expect(
        archived.projectDirectory.revision,
        sameRemote.projectDirectory.revision + 1,
      );
      expect(archived.projects, isEmpty);

      await api.openProject('/workspace/pure-lang');
      final reopened = await api.readStudioState();
      expect(
        reopened.projectDirectory.revision,
        archived.projectDirectory.revision + 1,
      );
      expect(reopened.projects.map((project) => project.id), [
        'project-local',
        'project-remote',
      ]);
    },
  );

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

  test(
    'Demo new Thread shows provisional then generated title event',
    () async {
      final api = DemoStudioApi();
      final titleEvent = api
          .subscribeProductEvents()
          .where((event) => event is StudioBridgeEvent)
          .cast<StudioBridgeEvent>()
          .where((event) {
            final payload = event.payload;
            return payload is ThreadDirectoryChangedPayload &&
                payload.upserted.any(
                  (thread) => thread.title == 'Demo generated session',
                );
          })
          .first
          .timeout(const Duration(seconds: 5));

      final result = await api.startNewThread(
        'project-local',
        const StudioPromptInput(
          text: 'Review the session title lifecycle',
          attachmentDraftIds: [],
        ),
        ThreadModeId.simple,
      );
      expect(result.thread.title, 'Review the session title lifecycle');

      await titleEvent;
      final state = await api.readStudioState();
      expect(
        state.threads
            .singleWhere((thread) => thread.id == result.thread.id)
            .title,
        'Demo generated session',
      );
    },
  );

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
  ThreadToolItemStateView(:final lifecycle) => lifecycle.isTerminal,
  _ => true,
};

class _FastLspDemoApi extends DemoStudioApi {
  _FastLspDemoApi() : super(lspActivityLoop: true);

  @override
  Duration get lspActivityStepDelay => const Duration(milliseconds: 40);
}
