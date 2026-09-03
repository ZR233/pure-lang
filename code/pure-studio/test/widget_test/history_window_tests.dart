part of '../widget_test.dart';

void registerHistoryWindowTests() {
  test(
    'switching back rebuilds the history window from the snapshot',
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
          historyCursor: 'a-turn-0',
        ),
      );
      await pumpEventQueue();
      api.historyPagesByThread['thread-a'] = {
        'a-turn-0': ThreadHistoryPage(
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
      // 窗口必须整体重建（epoch 递增、hasOlder 由新快照锚点决定）。
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
          historyCursor: 'a-turn-0',
        ),
      );
      await pumpEventQueue();

      state = container.read(studioControllerProvider).requireValue;
      final window = state.workspaceUiByThread['thread-a']!.history;
      expect(window.hasOlder, isTrue);
      expect(window.epoch, greaterThan(0));
      // 重建后的窗口就是快照内容；再次回源锚点从新窗口首条派生。
      expect(
        state.workspacesByThread['thread-a']!.items.map((item) => item.id),
        ['a-item-0', 'a-item-1', 'a-item-2'],
      );
      api.historyPagesByThread['thread-a'] = {
        'a-turn-0': ThreadHistoryPage(
          items: _windowItems('thread-a', 'a', -3, 3),
          nextCursor: null,
        ),
      };
      await controller.loadOlderHistory('thread-a');
      expect(api.historyRequests.last.cursor, 'a-turn-0');
      state = container.read(studioControllerProvider).requireValue;
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
    },
  );

  test('late history responses from before a rebuild are dropped', () async {
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
        historyCursor: 'a-turn-0',
      ),
    );
    await pumpEventQueue();

    // 旧 epoch 的历史请求挂起，尚未落地。
    final staleGate = Completer<void>();
    api.historyGates.add(staleGate);
    unawaited(controller.loadOlderHistory('thread-a'));

    // 新快照落地：窗口重建（epoch 递增），随后发起一次新 epoch 的回源。
    final rebuildGate = Completer<void>();
    api.historyGates.add(rebuildGate);
    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: _workspaceWithItems(
          'thread-a',
          revision: 5,
          items: _windowItems('thread-a', 'a', 0, 3),
        ),
        historyCursor: 'a-turn-0',
      ),
    );
    await pumpEventQueue();
    unawaited(controller.loadOlderHistory('thread-a'));
    await pumpEventQueue();

    // 旧响应在重建之后返回：必须被丢弃，不得污染重建后的窗口。
    staleGate.complete();
    await pumpEventQueue();
    var state = container.read(studioControllerProvider).requireValue;
    expect(state.workspacesByThread['thread-a']!.items.map((item) => item.id), [
      'a-item-0',
      'a-item-1',
      'a-item-2',
    ], reason: '跨重建的历史响应属于旧窗口，必须整体丢弃');
    expect(state.selectedWorkspaceUi.history.isLoading, isTrue);

    rebuildGate.complete();
    await pumpEventQueue();
    state = container.read(studioControllerProvider).requireValue;
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
        historyCursor: 'a-turn-0',
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
        historyCursor: 'a-turn-0',
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
