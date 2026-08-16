part of '../widget_test.dart';

void registerReducerRecoveryTests() {
  test('snapshot replaces the complete workspace but keeps UI state', () {
    final current = _emptyState().copyWith(
      workspaceUiByThread: const {
        'session-1': WorkspaceUiState(
          syncState: AgentWorkspaceSyncState.reconnecting,
          subscriptionGeneration: 7,
          composer: ComposerThreadState.idle(draft: 'local draft'),
        ),
      },
    );
    final incoming = current.selectedWorkspace!.copyWith(
      revision: 9,
      items: [
        _threadItemFixture(
          id: 'canonical',
          threadId: 'session-1',
          turnId: 'turn-1',
          ordinal: 0,
          text: 'canonical',
        ),
      ],
    );

    final next = applyThreadSnapshot(current, incoming);

    expect(next.selectedWorkspace!.items.single.text, 'canonical');
    expect(next.selectedWorkspace!.revision, 9);
    expect(next.selectedWorkspaceUi.composer.draft, 'local draft');
    expect(next.selectedWorkspaceUi.subscriptionGeneration, 7);
    expect(next.selectedWorkspaceUi.syncState, AgentWorkspaceSyncState.ready);
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
    final withItem = applyThreadSnapshot(
      current,
      current.selectedWorkspace!.copyWith(revision: 2, items: [item]),
    );

    final result = applyThreadUpdate(
      withItem,
      threadId: 'session-1',
      revision: 4,
      update: ThreadItemDeltaUpdate(
        const ThreadItemDeltaView(
          itemId: 'item-1',
          revision: 1,
          field: 'text',
          delta: 'gap',
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
    final current = applyThreadSnapshot(
      _emptyState(),
      _emptyState().selectedWorkspace!.copyWith(revision: 5, items: [item]),
    );

    final oldWorkspace = applyThreadUpdate(
      current,
      threadId: 'session-1',
      revision: 5,
      update: ThreadItemUpsert(item.copyWith(text: 'old')),
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
    final current = applyThreadSnapshot(
      _emptyState(),
      _emptyState().selectedWorkspace!.copyWith(revision: 1, items: [item]),
    );
    final changedOrdinal = ThreadItemView(
      id: item.id,
      threadId: item.threadId,
      turnId: item.turnId,
      ordinal: 4,
      revision: 1,
      status: item.status,
      createdAt: item.createdAt,
      updatedAt: item.updatedAt,
      kind: item.kind,
      text: item.text,
      channel: item.channel,
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
    final current = applyThreadSnapshot(
      _emptyState(),
      _emptyState().selectedWorkspace!.copyWith(revision: 1, items: [item]),
    );

    final result = applyThreadUpdate(
      current,
      threadId: 'session-1',
      revision: 2,
      update: const ThreadItemDeltaUpdate(
        ThreadItemDeltaView(
          itemId: 'item-1',
          revision: 1,
          field: 'text',
          delta: 'late',
        ),
      ),
    );

    expect(result.resyncThreadId, 'session-1');
  });
}
