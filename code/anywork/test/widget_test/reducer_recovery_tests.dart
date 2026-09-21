part of '../widget_test.dart';

void registerReducerRecoveryTests() {
  test('snapshot replaces current state and keeps UI state and window', () {
    final current = _emptyState().copyWith(
      workspaceUiByThread: const {
        'session-1': WorkspaceUiState(
          syncState: AgentWorkspaceSyncState.reconnecting,
          subscriptionGeneration: 7,
          composer: ComposerThreadState.idle(draft: 'local draft'),
        ),
      },
    );
    final seeded = _seedTimelineWindow(current, 'session-1', [
      _threadItemFixture(
        id: 'canonical',
        threadId: 'session-1',
        turnId: 'turn-1',
        ordinal: 0,
        text: 'canonical',
      ),
    ]);
    final incoming = seeded.selectedWorkspace!.copyWith(revision: 9);

    final next = applyThreadSnapshot(seeded, incoming);

    expect(next.selectedWorkspace!.items.single.text, 'canonical');
    expect(next.selectedWorkspace!.revision, 9);
    expect(next.selectedWorkspaceUi.composer.draft, 'local draft');
    expect(next.selectedWorkspaceUi.subscriptionGeneration, 7);
    expect(next.selectedWorkspaceUi.syncState, AgentWorkspaceSyncState.ready);
  });

  test('snapshot without window content never injects timeline items', () {
    final current = _emptyState();
    final seeded = _seedTimelineWindow(current, 'session-1', [
      _threadItemFixture(
        id: 'window-item',
        threadId: 'session-1',
        turnId: 'turn-1',
        ordinal: 0,
        text: 'window',
      ),
    ]);
    final incoming = seeded.selectedWorkspace!.copyWith(
      revision: 4,
      items: [
        _threadItemFixture(
          id: 'snapshot-item',
          threadId: 'session-1',
          turnId: 'turn-2',
          ordinal: 1,
          text: 'snapshot',
        ),
      ],
    );

    final next = applyThreadSnapshot(seeded, incoming);

    expect(next.selectedWorkspace!.revision, 4);
    expect(next.selectedWorkspace!.items.map((item) => item.text), ['window']);
  });

  test('workspace revision gap requests resubscription', () {
    final current = _emptyState();
    final item = _threadItemFixture(
      id: 'item-1',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 0,
      status: 'streaming',
    );
    final seeded = _seedTimelineWindow(current, 'session-1', [item]);
    final withItem = applyThreadSnapshot(
      seeded,
      seeded.selectedWorkspace!.copyWith(revision: 2),
    );

    final result = applyThreadUpdate(
      withItem,
      threadId: 'session-1',
      revision: 4,
      update: ThreadItemDeltaUpdate(
        const ThreadItemDeltaView(
          itemId: 'item-1',
          revision: 1,
          state: ThreadTextDeltaView('gap'),
        ),
      ),
    );

    expect(result.resyncThreadId, 'session-1');
    expect(result.state.selectedWorkspace!.revision, 2);
  });

  test('old workspace and Item revisions are ignored', () {
    final item = _threadItemFixture(
      id: 'item-1',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 0,
      revision: 2,
      status: 'streaming',
      text: 'new',
    );
    final seeded = _seedTimelineWindow(_emptyState(), 'session-1', [item]);
    final current = applyThreadSnapshot(
      seeded,
      seeded.selectedWorkspace!.copyWith(revision: 5),
    );

    final oldWorkspace = applyThreadUpdate(
      current,
      threadId: 'session-1',
      revision: 5,
      update: ThreadItemUpsert(
        _threadItemFixture(
          id: item.id,
          threadId: item.threadId,
          turnId: item.turnId,
          ordinal: item.ordinal,
          revision: item.revision,
          status: 'streaming',
          text: 'old',
        ),
      ),
    );

    expect(oldWorkspace.resyncThreadId, isNull);
    expect(oldWorkspace.state.selectedWorkspace!.items.single.text, 'new');
  });

  test('Item ordinal and identity cannot change after first insertion', () {
    final item = _threadItemFixture(
      id: 'item-1',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 3,
      revision: 0,
    );
    final seeded = _seedTimelineWindow(_emptyState(), 'session-1', [item]);
    final current = applyThreadSnapshot(
      seeded,
      seeded.selectedWorkspace!.copyWith(revision: 1),
    );
    final changedOrdinal = ThreadItemView(
      id: item.id,
      threadId: item.threadId,
      turnId: item.turnId,
      ordinal: 4,
      revision: 1,
      createdAt: item.createdAt,
      updatedAt: item.updatedAt,
      state: item.state,
    );

    final result = applyThreadUpdate(
      current,
      threadId: 'session-1',
      revision: 2,
      update: ThreadItemUpsert(changedOrdinal),
    );

    // ordinal 是总线一次性分配的不可变顺序事实：正常情况下同 id 不会携带
    // 不同 ordinal；防御性地忽略迟到载荷中的 ordinal 漂移（以已加载值为准），
    // 不再触发 resync。
    expect(result.resyncThreadId, isNull);
    expect(result.state.selectedWorkspace!.items.single.revision, 1);
    expect(result.state.selectedWorkspace!.items.single.ordinal, 3);
  });

  test('terminal Item rejects late delta', () {
    final item = _threadItemFixture(
      id: 'item-1',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 0,
      status: 'completed',
    );
    final seeded = _seedTimelineWindow(_emptyState(), 'session-1', [item]);
    final current = applyThreadSnapshot(
      seeded,
      seeded.selectedWorkspace!.copyWith(revision: 1),
    );

    final result = applyThreadUpdate(
      current,
      threadId: 'session-1',
      revision: 2,
      update: const ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: 'item-1',
          revision: 1,
          state: ThreadTextDeltaView('late'),
        ),
      ),
    );

    expect(result.resyncThreadId, 'session-1');
  });

  test('detached live tail is bounded at 400, not the window cap', () {
    // 阅读窗口 500、实时尾部 400 是两条独立契约：detached 读者不应因为共享一个上限而
    // 多保留 100 条永远不会进入窗口的尾部载荷（design/19-studio-ui.md§19.10）。
    final anchor = _threadItemFixture(
      id: 'item-0',
      threadId: 'session-1',
      turnId: 'turn-0',
      ordinal: 0,
      text: 'anchor',
    );
    var state = _seedTimelineWindow(_emptyState(), 'session-1', [anchor]);
    final seededUi = state.workspaceUiByThread['session-1']!;
    state = state.copyWith(
      workspaceUiByThread: {
        ...state.workspaceUiByThread,
        'session-1': seededUi.copyWith(
          history: seededUi.history.copyWith(
            detached: true,
            anchor: const TimelineAnchor('item-0', 0),
          ),
        ),
      },
    );

    for (var n = 1; n <= 420; n++) {
      final workspace = state.selectedWorkspace!;
      final result = applyThreadUpdate(
        state,
        threadId: 'session-1',
        revision: workspace.revision + 1,
        baseRevision: workspace.revision,
        update: ThreadItemUpsert(
          _threadItemFixture(
            id: 'live-$n',
            threadId: 'session-1',
            turnId: 'turn-live',
            ordinal: n,
            text: 'live $n',
          ),
        ),
      );
      expect(result.resyncThreadId, isNull);
      state = result.state;
    }

    final tail = state.selectedWorkspace!;
    expect(tail.latestItemIds.length, 400);
    // 最新的尾部条目保留、最旧的被淘汰，窗口本身不受影响。
    expect(tail.latestItemIds.last, 'live-420');
    expect(tail.latestItemIds.first, 'live-21');
    expect(tail.items.map((item) => item.id), ['item-0']);
  });
}
