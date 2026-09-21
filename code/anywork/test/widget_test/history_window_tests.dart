part of '../widget_test.dart';

void registerHistoryWindowTests() {
  test(
    'every subscription reads the authoritative window from the history API',
    () async {
      final initial = _twoThreadHistoryState();
      final api = _FakeStudioApi(initial);
      // thread-a 首窗来自历史 API；Thread 快照只表达当前状态。
      api.historyPagesByThread['thread-a'] = {
        null: ThreadHistoryPage(
          items: _windowItems('thread-a', 'a', 0, 2),
          nextCursor: 'a-item-0',
        ),
      };
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      final controller = container.read(studioControllerProvider.notifier);

      await container.read(studioControllerProvider.future);
      await pumpEventQueue();
      // 首屏只恢复选择，不打开会话（§6.1）：显式选择/打开才建立订阅并读权威窗口。
      await controller.selectThread('thread-a');
      await pumpEventQueue();
      var state = container.read(studioControllerProvider).requireValue;
      expect(
        state.workspacesByThread['thread-a']!.items.map((item) => item.id),
        ['a-item-0', 'a-item-1'],
      );
      expect(state.selectedWorkspaceUi.history.hasOlder, isTrue);

      // 切到 thread-b 再切回：每次订阅都重新读取窗口。
      await controller.selectThread('thread-b');
      await pumpEventQueue();
      expect(
        api.historyRequests.where((request) => request.threadId == 'thread-b'),
        hasLength(1),
      );

      await controller.selectThread('thread-a');
      await pumpEventQueue();
      state = container.read(studioControllerProvider).requireValue;
      expect(
        state.workspacesByThread['thread-a']!.items.map((item) => item.id),
        ['a-item-0', 'a-item-1'],
      );
      expect(
        api.historyRequests.where((request) => request.threadId == 'thread-a'),
        hasLength(2),
      );

      // 实时条目按身份合并进当前窗口。
      api.emitThreadFrame(
        _threadItemFrame(
          threadId: 'thread-a',
          workspaceRevision: 1,
          item: _threadItemFixture(
            id: 'a-item-2',
            threadId: 'thread-a',
            turnId: 'a-turn-2',
            ordinal: 2,
            text: 'a live 2',
          ),
        ),
      );
      await pumpEventQueue();
      state = container.read(studioControllerProvider).requireValue;
      expect(
        state.workspacesByThread['thread-a']!.items.map((item) => item.id),
        ['a-item-0', 'a-item-1', 'a-item-2'],
      );
    },
  );

  test('jump to latest invalidates an in-flight history response', () async {
    final initial = _twoThreadHistoryState();
    final api = _FakeStudioApi(initial);
    api.historyPagesByThread['thread-a'] = {
      null: ThreadHistoryPage(
        items: _windowItems('thread-a', 'a', 0, 2),
        nextCursor: 'a-item-0',
      ),
    };
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    final controller = container.read(studioControllerProvider.notifier);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    await controller.selectThread('thread-a');
    await pumpEventQueue();
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace!
          .items
          .map((item) => item.id),
      ['a-item-0', 'a-item-1'],
    );

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

  test('an equal-revision snapshot keeps the loaded window content', () async {
    final initial = _twoThreadHistoryState();
    final api = _FakeStudioApi(initial);
    api.historyPagesByThread['thread-a'] = {
      null: ThreadHistoryPage(
        items: _windowItems('thread-a', 'a', 0, 2),
        nextCursor: 'a-item-0',
      ),
    };
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    await container
        .read(studioControllerProvider.notifier)
        .selectThread('thread-a');
    await pumpEventQueue();
    final established = container.read(studioControllerProvider).requireValue;

    api.emitThreadFrame(_threadSnapshotFrame(initial, 'thread-a'));
    await pumpEventQueue();
    final state = container.read(studioControllerProvider).requireValue;

    // 相同 revision 的快照不重置阅读面：窗口内容与 epoch 原样保留。
    expect(state.workspacesByThread['thread-a']!.items.map((item) => item.id), [
      'a-item-0',
      'a-item-1',
    ]);
    expect(
      state.workspaceUiByThread['thread-a']!.history.epoch,
      established.workspaceUiByThread['thread-a']!.history.epoch,
    );
  });

  test('authoritative window reload keeps newer live previews and adopts newer rows', () async {
    final initial = _twoThreadHistoryState();
    final api = _FakeStudioApi(initial);
    api.historyPagesByThread['thread-a'] = {
      null: ThreadHistoryPage(
        items: [
          _threadItemFixture(
            id: 'a-item-0',
            threadId: 'thread-a',
            turnId: 'a-turn-0',
            ordinal: 0,
            revision: 1,
            status: 'completed',
            text: 'from database',
          ),
        ],
        nextCursor: null,
      ),
    };
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    final controller = container.read(studioControllerProvider.notifier);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    await controller.selectThread('thread-a');
    await pumpEventQueue();
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace!
          .items
          .single
          .text,
      'from database',
    );

    // 内存预览比数据库行新：重新读取窗口不得回退到旧 revision。
    api.emitThreadFrame(
      _threadItemFrame(
        threadId: 'thread-a',
        workspaceRevision: 1,
        item: _threadItemFixture(
          id: 'a-item-0',
          threadId: 'thread-a',
          turnId: 'a-turn-0',
          ordinal: 0,
          revision: 2,
          status: 'streaming',
          text: 'live preview',
        ),
      ),
    );
    await pumpEventQueue();
    await controller.jumpToLatest('thread-a');
    await pumpEventQueue();
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace!
          .items
          .single
          .text,
      'live preview',
    );

    // 数据库行比内存预览新（终态已落库）：以数据库为准。
    api.historyPagesByThread['thread-a'] = {
      null: ThreadHistoryPage(
        items: [
          _threadItemFixture(
            id: 'a-item-0',
            threadId: 'thread-a',
            turnId: 'a-turn-0',
            ordinal: 0,
            revision: 3,
            status: 'completed',
            text: 'persisted final',
          ),
        ],
        nextCursor: null,
      ),
    };
    await controller.jumpToLatest('thread-a');
    await pumpEventQueue();
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace!
          .items
          .single
          .text,
      'persisted final',
    );
  });

  test(
    'lagged resubscription re-reads the window from the history API',
    () async {
      final initial = _twoThreadHistoryState();
      final api = _FakeStudioApi(initial);
      api.historyPagesByThread['thread-a'] = {
        null: ThreadHistoryPage(
          items: _windowItems('thread-a', 'a', 0, 2),
          nextCursor: null,
        ),
      };
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      await pumpEventQueue();
      await container
          .read(studioControllerProvider.notifier)
          .selectThread('thread-a');
      await pumpEventQueue();
      final before = api.historyRequests.length;
      final subscriptions = api.threadSubscriptions.length;
      api.emitThreadFrame(
        const ThreadResyncRequiredFrame(threadId: 'thread-a', dropped: 1),
      );
      await pumpEventQueue();

      // 缺口不能由增量恢复：重新订阅后必须重新读取数据库窗口。
      expect(api.threadSubscriptions.length, subscriptions + 1);
      expect(api.historyRequests.length, greaterThan(before));
      expect(api.historyRequests.last.threadId, 'thread-a');
      expect(api.historyRequests.last.cursor, isNull);
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .selectedWorkspace!
            .items
            .map((item) => item.id),
        ['a-item-0', 'a-item-1'],
      );
    },
  );
}

StudioState _twoThreadHistoryState() {
  const project = StudioProject(id: 'project-1', name: 'project', path: '.');
  StudioThread thread(String id) => StudioThread(
    id: id,
    projectId: project.id,
    title: id,
    mode: ThreadModeId.simple,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
    workspacePath: project.path,
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
