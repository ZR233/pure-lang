part of '../widget_test.dart';

void registerThreadStreamTests() {
  test('authoritative Thread snapshot replaces accumulated delta', () async {
    final base = _emptyState();
    final api = _FakeStudioApi(base);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    final thread = base.threads.single;
    final started = _threadItemFixture(
      id: 'item-1',
      threadId: thread.id,
      turnId: 'turn-1',
      ordinal: 0,
      text: '',
      revision: 0,
      status: 'streaming',
    );
    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: base.selectedWorkspace!.copyWith(
          revision: 1,
          items: [started],
        ),
      ),
    );
    api.emitThreadFrame(
      _threadDeltaFrame(
        threadId: thread.id,
        workspaceRevision: 2,
        itemId: started.id,
        itemRevision: 1,
        field: 'text',
        delta: 'partial',
      ),
    );
    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: base.selectedWorkspace!.copyWith(
          revision: 3,
          items: [started.copyWith(revision: 2, text: 'authoritative')],
        ),
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedWorkspace!.revision, 3);
    expect(state.selectedWorkspace!.items.single.text, 'authoritative');
  });

  test('authoritative empty Thread directory clears prior entries', () async {
    final now = DateTime.fromMillisecondsSinceEpoch(1000);
    final initial = _emptyState().copyWith(
      projectDirectory: const ProjectDirectoryState(
        values: [
          StudioProject(id: 'project-1', name: 'one', path: 'one'),
          StudioProject(id: 'project-2', name: 'two', path: 'two'),
        ],
      ),
      threadDirectory: ThreadDirectoryWindow(
        threads: [
          StudioThread(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Thread 1',
            mode: StudioMode.simple,
            updatedAt: now,
          ),
          StudioThread(
            id: 'session-2',
            projectId: 'project-2',
            title: 'Thread 2',
            mode: StudioMode.task,
            updatedAt: now,
          ),
        ],
      ),
    );
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitGlobal(
      _threadDirectoryChangedEvent(
        projectId: 'project-1',
        threads: const [],
        removed: const ['session-1', 'session-2'],
      ),
    );
    await pumpEventQueue();

    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .threads
          .map((thread) => thread.id),
      isEmpty,
    );
  });

  test('ThreadRuntimeUpdated replaces selected workspace runtime', () async {
    final base = _emptyState();
    final api = _FakeStudioApi(base);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    api.emitThreadFrame(_threadSnapshotFrame(base, 'session-1'));
    api.emitThreadFrame(
      _threadRuntimeFrame(
        threadId: 'session-1',
        workspaceRevision: 1,
        runtime: const ThreadRuntimeView(
          model: 'planner/new',
          contextTokens: 42,
          contextWindow: 128000,
          totalTokens: 84,
          costLabel: '￥0.16',
          activeSkills: ['new-skill'],
          activeMcpServers: ['new-mcp'],
          activeLspServers: ['new-lsp'],
          agentCount: 0,
        ),
      ),
    );
    await pumpEventQueue();

    final runtime = container
        .read(studioControllerProvider)
        .requireValue
        .runtime;
    expect(runtime.model, 'planner/new');
    expect(runtime.contextTokens, 42);
    expect(runtime.activeSkills, ['new-skill']);
  });

  test('TurnStarted and TurnCompleted update one active Turn', () async {
    final base = _emptyState();
    final api = _FakeStudioApi(base);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    api.emitThreadFrame(_threadSnapshotFrame(base, 'session-1'));
    api.emitThreadFrame(
      _threadTurnFrame(
        threadId: 'session-1',
        workspaceRevision: 1,
        state: const StudioTurnState.inProgress(StudioTurnActivity.responding),
      ),
    );
    await pumpEventQueue();
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace!
          .activeTurn
          ?.state,
      const StudioTurnState.inProgress(StudioTurnActivity.responding),
    );

    api.emitThreadFrame(
      _threadTurnFrame(
        threadId: 'session-1',
        workspaceRevision: 2,
        state: const StudioTurnState.completed(),
      ),
    );
    await pumpEventQueue();
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace!
          .activeTurn,
      isNull,
    );
  });

  test('ItemCompleted replaces the streaming Item by identity', () async {
    final base = _emptyState();
    final api = _FakeStudioApi(base);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    api.emitThreadFrame(_threadSnapshotFrame(base, 'session-1'));
    final started = _threadItemFixture(
      id: 'item-1',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 1,
      text: 'partial',
      status: 'streaming',
    );
    api.emitThreadFrame(
      _threadItemFrame(
        threadId: 'session-1',
        workspaceRevision: 1,
        item: started,
      ),
    );
    api.emitThreadFrame(
      _threadItemFrame(
        threadId: 'session-1',
        workspaceRevision: 2,
        item: started.copyWith(
          revision: 1,
          status: 'completed',
          text: 'authoritative final',
        ),
      ),
    );
    await pumpEventQueue();

    final items = container
        .read(studioControllerProvider)
        .requireValue
        .selectedWorkspace!
        .items;
    expect(items, hasLength(1));
    expect(items.single.status, 'completed');
    expect(items.single.text, 'authoritative final');
  });
}
