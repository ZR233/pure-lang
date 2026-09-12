part of '../widget_test.dart';

void registerHistoryWindowTests() {
  test(
    'switching back preserves loaded history while adopting new output',
    () async {
      final initial = _twoThreadHistoryState();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      final controller = container.read(studioControllerProvider.notifier);

      await container.read(studioControllerProvider.future);
      await pumpEventQueue();
      // thread-a 首次快照：窗口内容 + 更旧锚点；上滚回源补齐历史。
      api.emitThreadFrame(
        ThreadSnapshotFrame(
          workspace: _workspaceWithItems(
            'thread-a',
            revision: 1,
            items: _windowItems('thread-a', 'a', 0, 2),
          ),
          historyCursor: 'a-item-0',
        ),
      );
      await pumpEventQueue();
      api.historyPagesByThread['thread-a'] = {
        'a-item-0': ThreadHistoryPage(
          items: _windowItems('thread-a', 'a', -3, 3),
          nextCursor: null,
        ),
      };
      await controller.loadOlderHistory('thread-a');

      var state = container.read(studioControllerProvider).requireValue;
      expect(
        state.workspacesByThread['thread-a']!.items.map((item) => item.id),
        ['a-item--3', 'a-item--2', 'a-item--1', 'a-item-0', 'a-item-1'],
      );
      expect(state.selectedWorkspaceUi.history.hasOlder, isFalse);

      // 切到 thread-b 再切回；期间 thread-a 有新事件，重订快照 revision 更大：
      // 新快照更新尾部，已加载的更旧内容与分页边界保持有效。
      await controller.selectThread('thread-b');
      api.emitThreadFrame(
        ThreadSnapshotFrame(
          workspace: _workspaceWithItems(
            'thread-b',
            revision: 1,
            items: [
              _threadItemFixture(
                id: 'b-live-1',
                threadId: 'thread-b',
                turnId: 'b-turn-1',
                ordinal: 5,
                text: 'b live 1',
              ),
            ],
          ),
        ),
      );
      await pumpEventQueue();

      await controller.selectThread('thread-a');
      api.emitThreadFrame(
        ThreadSnapshotFrame(
          workspace: _workspaceWithItems(
            'thread-a',
            revision: 2,
            items: _windowItems('thread-a', 'a', 0, 3),
          ),
          historyCursor: 'a-item-0',
        ),
      );
      await pumpEventQueue();

      state = container.read(studioControllerProvider).requireValue;
      final window = state.workspaceUiByThread['thread-a']!.history;
      expect(window.hasOlder, isFalse);
      expect(
        state.workspacesByThread['thread-a']!.items.map((item) => item.id),
        [
          'a-item--3',
          'a-item--2',
          'a-item--1',
          'a-item-0',
          'a-item-1',
          'a-item-2',
        ],
      );
      expect(state.workspacesByThread['thread-b']!.items.single.id, 'b-live-1');
      expect(state.workspacesByThread['thread-a']!.revision, 2);
    },
  );

  test('jump to latest invalidates an in-flight history response', () async {
    final initial = _twoThreadHistoryState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    final controller = container.read(studioControllerProvider.notifier);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: _workspaceWithItems(
          'thread-a',
          revision: 1,
          items: _windowItems('thread-a', 'a', 0, 2),
        ),
        historyCursor: 'a-item-0',
      ),
    );
    await pumpEventQueue();

    // 旧 epoch 的历史请求挂起，尚未落地。
    final staleGate = Completer<void>();
    api.historyGates.add(staleGate);
    unawaited(controller.loadOlderHistory('thread-a'));

    await controller.jumpToLatest('thread-a');
    staleGate.complete();
    await pumpEventQueue();
    final state = container.read(studioControllerProvider).requireValue;
    expect(state.workspacesByThread['thread-a']!.items.map((item) => item.id), [
      'a-item-0',
      'a-item-1',
    ]);
    expect(state.selectedWorkspaceUi.history.isLoading, isFalse);
  });

  test('an equal-revision snapshot keeps the established window', () async {
    final initial = _twoThreadHistoryState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: _workspaceWithItems(
          'thread-a',
          revision: 3,
          items: _windowItems('thread-a', 'a', 0, 2),
        ),
        historyCursor: 'a-item-0',
      ),
    );
    await pumpEventQueue();
    final established = container.read(studioControllerProvider).requireValue;

    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: _workspaceWithItems(
          'thread-a',
          revision: 3,
          items: _windowItems('thread-a', 'a', 0, 2),
        ),
        historyCursor: 'a-item-0',
      ),
    );
    await pumpEventQueue();
    final state = container.read(studioControllerProvider).requireValue;

    // 内容未变（revision 相同）：窗口与已加载 items 原样保留。
    expect(
      state.workspacesByThread['thread-a'],
      same(established.workspacesByThread['thread-a']),
    );
    expect(
      state.workspaceUiByThread['thread-a']!.history.epoch,
      established.workspaceUiByThread['thread-a']!.history.epoch,
    );
  });
  test('same commit revision streams model and tool text without resetting loaded history', () async {
    final initial = _twoThreadHistoryState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    final history = _windowItems('thread-a', 'a', 0, 2);
    ThreadItemView preview(String text) =>
        _threadItemFixture(
          id: 'live-answer',
          threadId: 'thread-a',
          turnId: 'a-turn-live',
          ordinal: 2,
          text: text,
        ).copyWith(
          state: ThreadTextItemStateView(
            channel: ThreadTextChannel.commentary,
            text: text,
            attachments: const [],
            lifecycle: const StreamingThreadContentView(),
          ),
        );
    ThreadItemView toolPreview(String text) =>
        _threadItemFixture(
          id: 'live-tool',
          threadId: 'thread-a',
          turnId: 'a-turn-live',
          ordinal: 3,
          text: '',
        ).copyWith(
          state: ThreadToolItemStateView(
            invocation: const ThreadToolInvocationView(
              toolCallId: 'call',
              name: 'exec_command',
              arguments: '{}',
            ),
            lifecycle: RunningThreadToolView(text),
          ),
        );
    void emit(String text) => api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: _workspaceWithItems(
          'thread-a',
          revision: 3,
          items: [...history, preview(text), toolPreview(text)],
        ),
        historyCursor: 'a-item-0',
      ),
    );
    emit('first');
    await pumpEventQueue();
    final first = container.read(studioControllerProvider).requireValue;
    final epoch = first.workspaceUiByThread['thread-a']!.history.epoch;
    final controller = container.read(studioControllerProvider.notifier);
    await controller.selectThread('thread-b');
    emit('first second');
    await pumpEventQueue();
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .workspacesByThread['thread-a']!
          .items[2]
          .text,
      'first',
    );
    await controller.selectThread('thread-a');
    emit('first second');
    await pumpEventQueue();
    final next = container.read(studioControllerProvider).requireValue;
    final workspace = next.workspacesByThread['thread-a']!;
    expect(workspace.revision, 3);
    expect(workspace.items.map((item) => item.id).toList(), [
      ...history.map((item) => item.id),
      'live-answer',
      'live-tool',
    ]);
    expect(
      (workspace.items[workspace.items.length - 2].state
              as ThreadTextItemStateView)
          .text,
      'first second',
    );
    expect(
      ((workspace.items.last.state as ThreadToolItemStateView).lifecycle
              as RunningThreadToolView)
          .streamedOutput,
      'first second',
    );
    expect(next.workspaceUiByThread['thread-a']!.history.epoch, epoch);
    expect(next.workspaceUiByThread['thread-a']!.history.hasOlder, isTrue);
  });
  test('history request survives newer snapshots, deduplicates and exposes a retryable edge failure', () async {
    final api = _FakeStudioApi(_twoThreadHistoryState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    final controller = container.read(studioControllerProvider.notifier);
    void emit(int revision) => api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: _workspaceWithItems(
          'thread-a',
          revision: revision,
          items: _windowItems('thread-a', 'a', 0, 2),
        ),
        historyCursor: 'a-item-0',
      ),
    );
    emit(1);
    await pumpEventQueue();
    final gate = Completer<void>();
    api.historyGates.add(gate);
    final first = controller.loadOlderHistory('thread-a');
    await controller.loadOlderHistory('thread-a');
    emit(2);
    await pumpEventQueue();
    await controller.loadOlderHistory('thread-a');
    expect(api.historyRequests.length, 1);
    gate.completeError(StateError('storage unavailable'));
    await first;
    var state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedWorkspace!.items.map((item) => item.id), [
      'a-item-0',
      'a-item-1',
    ]);
    expect(
      state.selectedWorkspaceUi.history.errorMessage,
      contains('storage unavailable'),
    );
    api.historyPagesByThread['thread-a'] = {
      'a-item-0': ThreadHistoryPage(
        items: _windowItems('thread-a', 'a', -2, 2),
        nextCursor: null,
      ),
    };
    await controller.loadOlderHistory('thread-a');
    state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedWorkspace!.items.map((item) => item.id), [
      'a-item--2',
      'a-item--1',
      'a-item-0',
      'a-item-1',
    ]);
    expect(state.selectedWorkspaceUi.history.errorMessage, isNull);
    expect(api.historyRequests.length, 2);
    final subscriptions = api.threadSubscriptions.length;
    api.emitThreadFrame(
      const ThreadResyncRequiredFrame(threadId: 'thread-a', dropped: 1),
    );
    await pumpEventQueue();
    expect(api.threadSubscriptions.length, subscriptions + 1);
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace!
          .items
          .map((item) => item.id),
      ['a-item--2', 'a-item--1', 'a-item-0', 'a-item-1'],
    );
  });
}

StudioState _twoThreadHistoryState() {
  const project = StudioProject(id: 'project-1', name: 'project', path: '.');
  StudioThread thread(String id) => StudioThread(
    id: id,
    projectId: project.id,
    title: id,
    mode: ThreadModeId.simple,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );
  ThreadWorkspace emptyWorkspace(StudioThread owner) => ThreadWorkspace(
    thread: owner,
    revision: 0,
    items: const [],
    interactions: const [],
    runtime: _testRuntime(),
  );
  final a = thread('thread-a');
  final b = thread('thread-b');
  return _studioStateFixture(
    projects: const [project],
    threads: [a, b],
    workspacesByThread: {a.id: emptyWorkspace(a), b.id: emptyWorkspace(b)},
    selectedProjectId: project.id,
    selectedThreadId: a.id,
  );
}

ThreadWorkspace _workspaceWithItems(
  String threadId, {
  required int revision,
  required List<ThreadItemView> items,
}) {
  final thread = StudioThread(
    id: threadId,
    projectId: 'project-1',
    title: threadId,
    mode: ThreadModeId.simple,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );
  return ThreadWorkspace(
    thread: thread,
    revision: revision,
    items: items,
    interactions: const [],
    runtime: _testRuntime(),
  );
}

List<ThreadItemView> _windowItems(
  String threadId,
  String label,
  int base,
  int count,
) {
  return List.generate(count, (index) {
    final ordinal = base + index;
    return _threadItemFixture(
      id: '$label-item-$ordinal',
      threadId: threadId,
      turnId: '$label-turn-$ordinal',
      ordinal: ordinal,
      text: '$label message $ordinal',
    );
  });
}
