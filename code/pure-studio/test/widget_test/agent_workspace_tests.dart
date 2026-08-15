part of '../widget_test.dart';

void registerAgentWorkspaceTests() {
  test('selecting a child atomically switches the whole workspace', () async {
    final initial = _rootAndChildState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await container
        .read(studioControllerProvider.notifier)
        .selectAgentThread('child-1');

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedThreadId, 'child-1');
    expect(state.selectedTimelineRows.single.part!.text, 'child timeline');
    expect(state.runtime.model, 'reviewer/model');
    expect(state.selectedTodoList!.items.single.step, 'review child');
    expect(state.activeInteraction!.id, 'child-interaction');
    expect(state.composer.draft, 'child draft');
  });

  test('root and child Composer state remain independent UI state', () {
    final state = _rootAndChildState();

    expect(
      state.workspaceUiByThread['session-1']!.composer.draft,
      'root draft',
    );
    expect(state.workspaceUiByThread['child-1']!.composer.draft, 'child draft');
    expect(
      state.workspacesByThread['session-1']!.items.single.text,
      'root timeline',
    );
    expect(
      state.workspacesByThread['child-1']!.items.single.text,
      'child timeline',
    );
  });

  test('child snapshot never overwrites the root canonical workspace', () {
    final current = _rootAndChildState();
    final incoming = current.workspacesByThread['child-1']!.copyWith(
      revision: 9,
      items: [
        _threadItemFixture(
          id: 'child-new',
          threadId: 'child-1',
          turnId: 'child-turn',
          ordinal: 0,
          text: 'new child snapshot',
        ),
      ],
    );

    final next = applyThreadSnapshot(current, incoming);

    expect(
      next.workspacesByThread['session-1']!.items.single.text,
      'root timeline',
    );
    expect(
      next.workspacesByThread['child-1']!.items.single.text,
      'new child snapshot',
    );
    expect(next.workspaceUiByThread['child-1']!.composer.draft, 'child draft');
  });

  test('thread snapshot cannot overwrite product-owned directory metadata', () {
    final base = _emptyState();
    final canonical = base.selectedThread!.copyWith(
      mode: StudioMode.task,
      role: 'planner',
      updatedAt: DateTime.fromMillisecondsSinceEpoch(2000),
    );
    final current = base.copyWith(
      threadDirectory: ThreadDirectoryWindow(threads: [canonical]),
    );

    final next = applyThreadSnapshot(
      current,
      base.selectedWorkspace!.copyWith(revision: 1),
    );

    expect(next.selectedThread!.mode, StudioMode.task);
    expect(next.selectedThread!.role, 'planner');
    expect(next.selectedWorkspace!.thread.mode, StudioMode.task);
    expect(next.selectedWorkspace!.thread.role, 'planner');
  });

  test(
    'root is derived from rootThreadId without a second selected root id',
    () {
      final state = _rootAndChildState().copyWith(selectedThreadId: 'child-1');

      expect(state.selectedThread!.id, 'child-1');
      expect(state.selectedRootThread!.id, 'session-1');
      expect(state.threadsForSelectedRoot.map((thread) => thread.id), [
        'session-1',
        'child-1',
      ]);
    },
  );

  testWidgets('AgentWorkspacePane renders selected Thread content', (
    tester,
  ) async {
    final initial = _rootAndChildState().copyWith(selectedThreadId: 'child-1');
    final api = _FakeStudioApi(initial);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const Scaffold(body: AgentWorkspacePane())),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('child timeline'), findsOneWidget);
    expect(find.text('reviewer/model'), findsWidgets);
  });
}

StudioState _rootAndChildState() {
  final base = _emptyState();
  final root = base.threads.single.copyWith(
    title: 'Root',
    rootThreadId: 'session-1',
    agentPath: 'root',
  );
  final child = StudioThread(
    id: 'child-1',
    projectId: root.projectId,
    title: 'Reviewer',
    mode: StudioMode.task,
    createdAt: _fixtureDate(2),
    updatedAt: _fixtureDate(2),
    parentThreadId: root.id,
    rootThreadId: root.id,
    agentPath: 'root/reviewer',
    role: 'reviewer',
    status: 'running',
  );
  final rootWorkspace = ThreadWorkspace(
    thread: root,
    revision: 1,
    items: [
      _threadItemFixture(
        id: 'root-item',
        threadId: root.id,
        turnId: 'root-turn',
        ordinal: 0,
        text: 'root timeline',
      ),
    ],
    interactions: const [],
    runtime: _testRuntime().copyWith(model: 'planner/model'),
    todo: const TimelineTodoListUpdate(
      callId: 'root-todo',
      items: [TimelineTodoItem(step: 'plan root', status: 'inProgress')],
    ),
  );
  final childWorkspace = ThreadWorkspace(
    thread: child,
    revision: 1,
    items: [
      _threadItemFixture(
        id: 'child-item',
        threadId: child.id,
        turnId: 'child-turn',
        ordinal: 0,
        text: 'child timeline',
      ),
    ],
    interactions: const [
      PendingInteraction(
        id: 'child-interaction',
        threadId: 'child-1',
        turnId: 'child-turn',
        kind: InteractionKind.userInput,
        title: 'Review input',
        body: 'Continue?',
      ),
    ],
    runtime: _testRuntime().copyWith(model: 'reviewer/model'),
    todo: const TimelineTodoListUpdate(
      callId: 'child-todo',
      items: [TimelineTodoItem(step: 'review child', status: 'inProgress')],
    ),
  );
  return base.copyWith(
    threadDirectory: ThreadDirectoryWindow(threads: [root, child]),
    workspacesByThread: {root.id: rootWorkspace, child.id: childWorkspace},
    workspaceUiByThread: {
      root.id: const WorkspaceUiState(
        syncState: AgentWorkspaceSyncState.ready,
        composer: ComposerThreadState.idle(draft: 'root draft'),
      ),
      child.id: const WorkspaceUiState(
        syncState: AgentWorkspaceSyncState.ready,
        composer: ComposerThreadState.idle(draft: 'child draft'),
      ),
    },
    selectedThreadId: root.id,
  );
}
