part of '../widget_test.dart';

/// 实时驱动 >500 条窗口裁剪时的历史窗口生命周期。
///
/// 契约：只有**真实淘汰**（窗口最旧条目离开窗口与缓存）才把 `hasOlder`/`olderCursor`
/// 前移到淘汰后的第一条常驻条目；仅"有新帧流入"绝不宣称存在更旧历史；数据库身份与已
/// 采纳写水位不被实时流量改写；已由 SQL 页携带的旧游标在边界未移动时保持原样。
void registerLiveWindowTests() {
  test('live eviction raises hasOlder at the post-eviction first item', () {
    var state = _seedLiveWindow(
      databaseId: 'db-1',
      watermark: 7,
      items: const [],
    );

    // 500 条常驻：窗口刚好占满，尚未发生任何淘汰，因此不得宣称更旧历史。
    state = _feedLiveItems(state, startIndex: 0, count: 500);
    var workspace = state.selectedWorkspace!;
    expect(workspace.items.length, 500);
    expect(workspace.items.first.id, 'live-0');
    expect(workspace.items.last.id, 'live-499');
    expect(state.selectedWorkspaceUi.history.hasOlder, isFalse);
    expect(state.selectedWorkspaceUi.history.olderCursor, isNull);
    expect(state.selectedWorkspaceUi.history.appliedWriteSequence, 7);
    expect(state.selectedWorkspaceUi.history.databaseId, 'db-1');

    // 第 501 条触发真实淘汰；旧边界就是淘汰后的第一条常驻条目。
    state = _feedLiveItems(state, startIndex: 500, count: 1);
    workspace = state.selectedWorkspace!;
    final history = state.selectedWorkspaceUi.history;
    expect(workspace.items.length, 500);
    expect(workspace.items.first.id, 'live-1');
    expect(workspace.items.last.id, 'live-500');
    expect(history.hasOlder, isTrue);
    expect(history.olderCursor, 'live-1');
    expect(history.hasNewer, isFalse);
    expect(history.newerCursor, isNull);
    expect(workspace.cachedItems.containsKey('live-0'), isFalse);
    expect(workspace.latestItemIds, isEmpty);
    // 实时淘汰只前移阅读窗口的旧边界：不换数据库身份、不伪造写水位。
    expect(history.databaseId, 'db-1');
    expect(history.appliedWriteSequence, 7);
    expect(history.detached, isFalse);
  });

  test('live arrival without eviction never claims older history', () {
    var state = _seedLiveWindow(
      databaseId: 'db-1',
      watermark: 2,
      items: const [],
    );
    for (final count in [1, 100, 400, 500]) {
      state = _feedLiveItems(state, startIndex: 0, count: count);
      final history = state.selectedWorkspaceUi.history;
      expect(history.hasOlder, isFalse, reason: 'resident $count items');
      expect(history.olderCursor, isNull, reason: 'resident $count items');
    }
    expect(state.selectedWorkspace!.items.length, 500);
    // 旧水位与身份在整段实时流里保持不变。
    expect(state.selectedWorkspaceUi.history.appliedWriteSequence, 2);
    expect(state.selectedWorkspaceUi.history.databaseId, 'db-1');
  });

  test('a carried SQL boundary survives live flow until a real eviction', () {
    var state = _seedLiveWindow(
      databaseId: 'db-1',
      watermark: 5,
      items: _liveItems(0, 5),
    );
    // 跳回最新时被保留的 SQL 页游标 token：边界未移动前不得被实时流量改写。
    state = _withOlderCursor(state, 'carried-page-cursor');
    expect(state.selectedWorkspaceUi.history.hasOlder, isTrue);
    expect(
      state.selectedWorkspaceUi.history.olderCursor,
      'carried-page-cursor',
    );

    state = _feedLiveItems(state, startIndex: 5, count: 495);
    var history = state.selectedWorkspaceUi.history;
    expect(state.selectedWorkspace!.items.length, 500);
    expect(history.hasOlder, isTrue);
    expect(history.olderCursor, 'carried-page-cursor');
    expect(history.appliedWriteSequence, 5);
    expect(history.databaseId, 'db-1');

    // 真实淘汰：旧边界前移到淘汰后的第一条常驻条目（canonical identity）。
    state = _feedLiveItems(state, startIndex: 500, count: 1);
    history = state.selectedWorkspaceUi.history;
    expect(state.selectedWorkspace!.items.first.id, 'live-1');
    expect(history.hasOlder, isTrue);
    expect(history.olderCursor, 'live-1');
    expect(history.appliedWriteSequence, 5);
    expect(history.databaseId, 'db-1');
  });

  test(
    'a detached reader keeps its own 400-item tail without older claims',
    () {
      var state = _seedLiveWindow(
        databaseId: 'db-1',
        watermark: 3,
        items: _liveItems(0, 1),
      );
      // 首窗没有声明更旧游标：窗口里只有 1 条常驻并不代表存在更旧历史，旧边界必须为空。
      expect(state.selectedWorkspaceUi.history.hasOlder, isFalse);
      expect(state.selectedWorkspaceUi.history.olderCursor, isNull);
      state = _detach(state);
      expect(state.selectedWorkspaceUi.history.detached, isTrue);

      // 离开底部后的实时流只淘汰独立 tail（window 与 cached 都不动），因此既不能新增
      // 更旧历史，也不能前移旧边界。
      state = _feedLiveItems(state, startIndex: 1, count: 450);
      final workspace = state.selectedWorkspace!;
      final history = state.selectedWorkspaceUi.history;
      // 阅读窗口不动，实时尾部独立有界到 400。
      expect(workspace.items.map((item) => item.id), ['live-0']);
      expect(workspace.latestItemIds.length, maxLiveTailItems);
      expect(workspace.latestItemIds.first, 'live-51');
      expect(workspace.latestItemIds.last, 'live-450');
      expect(history.hasOlder, isFalse);
      expect(history.olderCursor, isNull);
      expect(history.hasNewer, isFalse);
      expect(workspace.cachedItems.length, lessThanOrEqualTo(900));
    },
  );

  test(
    'a follow-bottom window takes its older boundary from the page only',
    () {
      // 页显式声明更旧游标：跟随底部的窗口采纳该 token，而不是把它退化成窗口首条身份。
      var state = _seedLiveWindow(
        databaseId: 'db-1',
        watermark: 4,
        items: _liveItems(10, 3),
        pageOlderCursor: 'page-older-token',
      );
      expect(state.selectedWorkspaceUi.history.hasOlder, isTrue);
      expect(state.selectedWorkspaceUi.history.olderCursor, 'page-older-token');

      // 实时流入但没有淘汰：旧边界未移动，页携带的 token 保持原样。
      state = _feedLiveItems(state, startIndex: 13, count: 497);
      expect(state.selectedWorkspace!.items.length, 500);
      expect(state.selectedWorkspace!.items.first.id, 'live-10');
      expect(state.selectedWorkspaceUi.history.olderCursor, 'page-older-token');

      // 真实淘汰：旧边界前移为淘汰后的第一条常驻身份。
      state = _feedLiveItems(state, startIndex: 510, count: 1);
      expect(state.selectedWorkspace!.items.first.id, 'live-11');
      expect(state.selectedWorkspaceUi.history.hasOlder, isTrue);
      expect(state.selectedWorkspaceUi.history.olderCursor, 'live-11');
    },
  );

  test(
    'a live-trimmed window merges older then newer pages in one process',
    () {
      var state = _seedLiveWindow(
        databaseId: 'db-1',
        watermark: 7,
        items: const [],
      );
      state = _feedLiveItems(state, startIndex: 0, count: 501);
      expect(state.selectedWorkspaceUi.history.olderCursor, 'live-1');

      // 更旧一页：回到会话第一条，并带回页面自己的旧游标。
      state = applyTimelinePage(
        state,
        'session-1',
        TimelinePage(
          threadId: 'session-1',
          databaseId: 'db-1',
          watermark: 9,
          items: _liveItems(0, 1),
          olderCursor: 'older-page-cursor',
        ),
        TimelineDirection.older,
      );
      var workspace = state.selectedWorkspace!;
      var history = state.selectedWorkspaceUi.history;
      expect(workspace.items.first.id, 'live-0');
      expect(workspace.items.last.id, 'live-499');
      expect(workspace.items.length, 500);
      expect(history.hasOlder, isTrue);
      expect(history.olderCursor, 'older-page-cursor');
      expect(history.hasNewer, isTrue);
      expect(history.newerCursor, 'live-499');
      expect(history.appliedWriteSequence, 9);

      // 更新一页：回到最新，旧边界变成被裁剪后的第一条常驻条目。
      state = applyTimelinePage(
        state,
        'session-1',
        TimelinePage(
          threadId: 'session-1',
          databaseId: 'db-1',
          watermark: 11,
          items: _liveItems(501, 1),
        ),
        TimelineDirection.newer,
      );
      workspace = state.selectedWorkspace!;
      history = state.selectedWorkspaceUi.history;
      expect(workspace.items.last.id, 'live-501');
      expect(workspace.items.first.id, 'live-1');
      expect(workspace.items.length, 500);
      expect(history.hasNewer, isFalse);
      expect(history.newerCursor, isNull);
      expect(history.hasOlder, isTrue);
      expect(history.olderCursor, 'live-1');
      expect(history.appliedWriteSequence, 11);
      expect(history.databaseId, 'db-1');
    },
  );

  testWidgets('a >500-item live window renders lazily and pages older', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(980, 520));
    const threadId = 'session-live-window';
    var loadCount = 0;
    await tester.pumpWidget(
      _timelineHarness(
        threadId: threadId,
        items: _scrollItems(threadId, 520),
        onLoadOlder: () => loadCount += 1,
      ),
    );
    await tester.pumpAndSettle();

    // 首帧钉在底部：最旧条目根本没有被构建，常驻 520 条不等于渲染 520 行。
    expect(find.textContaining(RegExp(r'message 0 for ')), findsNothing);
    expect(_renderedRowCount(threadId, tester), lessThanOrEqualTo(20));

    // 同一进程内向更旧方向仍然可达：向旧边持续滚动会触发 onLoadOlder，
    // 而远端行始终懒构建（任何一帧渲染的行数都远小于常驻条数）。
    var widest = _renderedRowCount(threadId, tester);
    for (var attempt = 0; attempt < 120 && loadCount == 0; attempt++) {
      await tester.drag(
        find.byKey(StudioDriverKeys.timeline),
        const Offset(0, 3000),
      );
      await tester.pumpAndSettle();
      final rendered = _renderedRowCount(threadId, tester);
      if (rendered > widest) widest = rendered;
    }
    expect(loadCount, greaterThanOrEqualTo(1));
    expect(widest, lessThanOrEqualTo(40));
  });
}

int _renderedRowCount(String threadId, WidgetTester tester) {
  return tester
      .widgetList(
        find.textContaining(
          RegExp('message \\d+ for ${RegExp.escape(threadId)}'),
        ),
      )
      .length;
}

StudioState _seedLiveWindow({
  required String databaseId,
  required int watermark,
  required List<ThreadItemView> items,
  String? pageOlderCursor,
}) {
  return applyTimelinePage(
    _emptyState(),
    'session-1',
    TimelinePage(
      threadId: 'session-1',
      databaseId: databaseId,
      watermark: watermark,
      items: items,
      olderCursor: pageOlderCursor,
    ),
    TimelineDirection.newer,
    replaceWindow: true,
    followBottom: true,
  );
}

/// 模拟"上一页由 SQL 携带、经跳回最新保留下来"的旧边界游标 token。
StudioState _withOlderCursor(StudioState state, String cursor) {
  final ui = state.workspaceUiByThread['session-1']!;
  return state.copyWith(
    workspaceUiByThread: {
      ...state.workspaceUiByThread,
      'session-1': ui.copyWith(
        history: ui.history.copyWith(hasOlder: true, olderCursor: cursor),
      ),
    },
  );
}

List<ThreadItemView> _liveItems(int startIndex, int count) {
  return [
    for (var index = startIndex; index < startIndex + count; index++)
      _threadItemFixture(
        id: 'live-$index',
        threadId: 'session-1',
        turnId: 'turn-$index',
        ordinal: index,
        revision: index,
        text: 'live payload $index',
      ),
  ];
}

/// 用真实实时路径（item upsert）推进会话状态；每个 upsert 占一个 workspace revision。
StudioState _feedLiveItems(
  StudioState state, {
  required int startIndex,
  required int count,
}) {
  var next = state;
  for (var index = startIndex; index < startIndex + count; index++) {
    final revision = next.workspacesByThread['session-1']!.revision + 1;
    final result = applyThreadUpdate(
      next,
      threadId: 'session-1',
      revision: revision,
      update: ThreadItemUpsert(
        _threadItemFixture(
          id: 'live-$index',
          threadId: 'session-1',
          turnId: 'turn-$index',
          ordinal: index,
          revision: revision,
          text: 'live payload $index',
        ),
      ),
    );
    expect(result.resyncThreadId, isNull);
    next = result.state;
  }
  return next;
}

StudioState _detach(StudioState state) {
  final ui = state.workspaceUiByThread['session-1']!;
  return state.copyWith(
    workspaceUiByThread: {
      ...state.workspaceUiByThread,
      'session-1': ui.copyWith(history: ui.history.copyWith(detached: true)),
    },
  );
}
