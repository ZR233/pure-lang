part of '../widget_test.dart';

/// 有界实时正文：超大 streaming payload 的内存/渲染预算、两条推理通道的
/// chunk 身份与省略量，以及“预览 -> 预览可重试 -> 按身份回源完整正文”的路径。
///
/// 这些用例只断言客户端预算与回源契约；canonical 全文仍由 Rust 持久化。
void registerBoundedBodyTests() {
  test('a long streamed text body stays within the client budget', () {
    var item = _threadItemFixture(
      id: 'stream-text',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 0,
      status: 'streaming',
    );
    var revision = 1;
    var previousOmitted = 0;
    final chunk = 'x' * 512;

    for (var i = 0; i < 40; i++) {
      final next = item.appendDelta(
        delta: ThreadTextDeltaView(chunk),
        nextRevision: revision,
      );
      expect(next, isNotNull);
      item = next!;
      revision += 1;
      final text = (item.state as ThreadTextItemStateView).text;
      expect(text.length, lessThanOrEqualTo(kTimelineItemBodyBudget));
      // 保留的是最近的尾部：最新 delta 必须完整可见，且省略量只增不减。
      expect(text.endsWith(chunk), isTrue);
      expect(item.bodyOmittedUnits, greaterThanOrEqualTo(previousOmitted));
      previousOmitted = item.bodyOmittedUnits;
    }

    expect(item.bodyOmittedUnits, 40 * 512 - kTimelineItemBodyBudget);
    expect(item.bodyPreviewed, isTrue);
    expect(item.bodyLoaded, isFalse);
  });

  test('streamed trimming never starts on a UTF-16 low surrogate', () {
    var item = _threadItemFixture(
      id: 'stream-surrogate',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 0,
      status: 'streaming',
    );
    var revision = 1;
    final expected = StringBuffer();
    for (var i = 0; i < 240; i++) {
      final delta = i.isEven ? '😀' * 40 : 'z' * 37;
      expected.write(delta);
      final next = item.appendDelta(
        delta: ThreadTextDeltaView(delta),
        nextRevision: revision,
      );
      expect(next, isNotNull);
      item = next!;
      revision += 1;
      final text = (item.state as ThreadTextItemStateView).text;
      expect(text.length, lessThanOrEqualTo(kTimelineItemBodyBudget));
      // 驻留正文必须永远是完整拼接结果的精确后缀：既不撕裂代理对，也不重复/漏段。
      expect(expected.toString().endsWith(text), isTrue);
      if (text.isNotEmpty) {
        final first = text.codeUnitAt(0);
        expect(first >= 0xDC00 && first <= 0xDFFF, isFalse);
      }
      expect(text.endsWith(delta), isTrue);
    }
    expect(item.bodyPreviewed, isTrue);
  });

  test(
    'reasoning chunks keep producer identity across a shared body budget',
    () {
      const budget = kTimelineItemBodyBudget;
      var item = _threadItemFixture(
        id: 'reasoning-stream',
        threadId: 'session-1',
        turnId: 'turn-1',
        ordinal: 0,
        kind: ThreadItemKind.reasoning,
        status: 'streaming',
      );
      var revision = 1;

      // 生产者 chunkIndex 是**该通道内**的稠密位置下标（Rust `chunk_appends` 用
      // `next.iter().enumerate()`），两条通道各自独立从 0 开始。
      String chunkText(int index, int units) {
        final marker = 'c$index-';
        final repeat = units ~/ marker.length;
        return repeat <= 0 ? marker : marker * repeat;
      }

      const summaryChunks = 12;
      const contentChunks = 12;
      var summarySent = 0;
      var contentSent = 0;
      // 交替推进两条通道；共享预算会把最旧的分块挤出内存。
      for (var step = 0; step < summaryChunks + contentChunks; step++) {
        final toContent = step.isOdd;
        final index = step ~/ 2;
        if (toContent) {
          contentSent += 1;
        } else {
          summarySent += 1;
        }
        final delta = chunkText(index, 1024);
        final next = item.appendDelta(
          delta: toContent
              ? ThreadThinkingContentDeltaView(index, delta)
              : ThreadThinkingSummaryDeltaView(index, delta),
          nextRevision: revision,
        );
        expect(next, isNotNull);
        item = next!;
        revision += 1;

        final state = item.state as ThreadThinkingItemStateView;
        expect(_chunkUnits(state), lessThanOrEqualTo(budget));
        // chunkBase + 本地分块数 == 生产者已经发出的分块数：丢弃只影响下标基准，
        // 不会把后续 delta 写进错误的分块。
        expect(state.summaryChunkBase + state.summary.length, summarySent);
        expect(state.contentChunkBase + state.content.length, contentSent);
        // 保留的每个分块只能来自单一生产者分块，不得出现跨分块错位拼贴。
        for (final chunk in [...state.summary, ...state.content]) {
          final markers = RegExp(r'c(\d+)-')
              .allMatches(chunk)
              .map((match) => match.group(1))
              .toSet();
          expect(markers.length, lessThanOrEqualTo(1));
        }
      }
      expect(item.bodyPreviewed, isTrue);
      expect(item.bodyOmittedUnits, greaterThan(0));

      // 生产者继续发下一个稠密分块：必须落在正确的本地位置（而不是错位或被误判成缺口）。
      final beforeLate = item.state as ThreadThinkingItemStateView;
      expect(
        beforeLate.summaryChunkBase + beforeLate.summary.length,
        summaryChunks,
      );
      expect(
        beforeLate.contentChunkBase + beforeLate.content.length,
        contentChunks,
      );
      final lateDelta = chunkText(contentChunks, 1024);
      final late = item.appendDelta(
        delta: ThreadThinkingContentDeltaView(contentChunks, lateDelta),
        nextRevision: revision,
      );
      expect(late, isNotNull);
      final lateItem = late!;
      final lateState = lateItem.state as ThreadThinkingItemStateView;
      expect(lateState.content.last.endsWith(lateDelta), isTrue);
      expect(_chunkUnits(lateState), lessThanOrEqualTo(budget));

      // 属于已被丢弃分块的迟到 delta 只累计省略量，不抛错也不伪造缺口。
      expect(
        lateState.contentChunkBase > 0 || lateState.summaryChunkBase > 0,
        isTrue,
      );
      final droppedView = lateState.contentChunkBase > 0
          ? ThreadThinkingContentDeltaView(
              lateState.contentChunkBase - 1,
              'late-tail',
            )
          : ThreadThinkingSummaryDeltaView(
              lateState.summaryChunkBase - 1,
              'late-tail',
            );
      final omittedBefore = lateItem.bodyOmittedUnits;
      final droppedChunk = lateItem.appendDelta(
        delta: droppedView,
        nextRevision: revision + 1,
      );
      expect(droppedChunk, isNotNull);
      final droppedItem = droppedChunk!;
      expect(droppedItem.bodyOmittedUnits, omittedBefore + 'late-tail'.length);
      final droppedState = droppedItem.state as ThreadThinkingItemStateView;
      expect(_chunkUnits(droppedState), lessThanOrEqualTo(budget));
    },
  );

  test('a streamed window item stays bounded and preview-flagged', () {
    final streaming = _threadItemFixture(
      id: 'live-text',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 0,
      status: 'streaming',
    );
    var state = _seedTimelineWindow(_emptyState(), 'session-1', [streaming]);
    final chunk = 'y' * 700;
    var revision = 1;
    for (var i = 0; i < 30; i++) {
      final result = applyThreadUpdate(
        state,
        threadId: 'session-1',
        revision: revision,
        update: ThreadItemDeltaUpdate(
          ThreadItemDeltaView(
            itemId: 'live-text',
            revision: revision,
            state: ThreadTextDeltaView(chunk),
          ),
        ),
      );
      expect(result.resyncThreadId, isNull);
      state = result.state;
      revision += 1;
    }

    var windowed = state.selectedWorkspace!.items.single;
    final text = (windowed.state as ThreadTextItemStateView).text;
    expect(text.length, lessThanOrEqualTo(kTimelineItemBodyBudget));
    expect(windowed.bodyPreviewed, isTrue);
    // 窗口状态由条目自身推导：超大实时正文给出可见的回源入口。
    expect(
      state.selectedWorkspaceUi.history.previewedItemIds,
      contains('live-text'),
    );

    // 终态 upsert 携带完整正文：进入内存的仍只是有界尾部，省略量按 canonical 全量
    // 重新计算且不少于实时累计，不会把“只驻留尾部”静默表示成完整正文。
    final terminalText = chunk * 30;
    final upsert = applyThreadUpdate(
      state,
      threadId: 'session-1',
      revision: revision,
      update: ThreadItemUpsert(
        _threadItemFixture(
          id: 'live-text',
          threadId: 'session-1',
          turnId: 'turn-1',
          ordinal: 0,
          revision: revision,
          text: terminalText,
        ),
      ),
    );
    expect(upsert.resyncThreadId, isNull);
    state = upsert.state;
    windowed = state.selectedWorkspace!.items.single;
    final terminalInMemory = (windowed.state as ThreadTextItemStateView).text;
    expect(terminalInMemory.length, lessThanOrEqualTo(kTimelineItemBodyBudget));
    expect(terminalInMemory.endsWith(chunk), isTrue);
    expect(windowed.bodyPreviewed, isTrue);
    expect(
      windowed.bodyOmittedUnits,
      terminalText.length - terminalInMemory.length,
    );
    expect(
      state.selectedWorkspaceUi.history.previewedItemIds,
      contains('live-text'),
    );
  });

  test('an explicit by-id read restores the exact body after durability', () {
    final full = 'HEAD-${'a' * 40000}-NATIVE_ACCEPT_SENTINEL';
    // 页面按 Rust 单条预览预算给出的载荷：头部片段 + 截断标记，并声明省略量。
    final previewText = 'HEAD-${'a' * 400}-…[truncated]';
    final omitted = full.length - previewText.length;
    final previewItem = _threadItemFixture(
      id: 'persisted-text',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 3,
      text: previewText,
    );
    final seeded = applyTimelinePage(
      _emptyState(),
      'session-1',
      TimelinePage(
        threadId: 'session-1',
        watermark: 0,
        items: [previewItem],
        previews: [
          TimelineItemPreviewView(
            itemId: 'persisted-text',
            ordinal: 3,
            revision: 0,
            totalBytes: full.length,
            previewBytes: previewText.length,
            omittedBytes: omitted,
          ),
        ],
      ),
      TimelineDirection.newer,
      replaceWindow: true,
      followBottom: true,
    );

    final previewed = seeded.selectedWorkspace!.items.single;
    expect(previewed.bodyPreviewed, isTrue);
    expect(previewed.bodyOmittedUnits, omitted);
    expect(
      seeded.selectedWorkspaceUi.history.previewedItemIds,
      contains('persisted-text'),
    );

    // 历史事务尚未 durable：数据源仍按同身份预览返回，入口保持可见可重试。
    final loading = startItemBodyLoad(seeded, 'session-1', 'persisted-text');
    expect(
      loading.selectedWorkspaceUi.history.loadingItemIds,
      contains('persisted-text'),
    );
    final beforeDurable = applyItemBodyPage(
      loading,
      'session-1',
      'persisted-text',
      TimelinePage(
        threadId: 'session-1',
        watermark: 4,
        items: [previewed],
        previews: [
          TimelineItemPreviewView(
            itemId: 'persisted-text',
            ordinal: 3,
            revision: previewed.revision,
            totalBytes: full.length,
            previewBytes: previewText.length,
            omittedBytes: omitted,
          ),
        ],
      ),
    );
    final history = beforeDurable.selectedWorkspaceUi.history;
    expect(history.pendingItemBodyIds, contains('persisted-text'));
    expect(history.unavailableItemIds, isNot(contains('persisted-text')));
    expect(history.loadingItemIds, isNot(contains('persisted-text')));
    expect(history.previewedItemIds, contains('persisted-text'));
    // 仍在途：再次发起回源不会被状态机拒绝。
    expect(
      startItemBodyLoad(
        beforeDurable,
        'session-1',
        'persisted-text',
      ).selectedWorkspaceUi.history.loadingItemIds,
      contains('persisted-text'),
    );

    // durable 之后按 canonical identity 读到完整正文：正文回到内存并保留精确内容。
    final durableItem = _threadItemFixture(
      id: 'persisted-text',
      threadId: 'session-1',
      turnId: 'turn-1',
      ordinal: 3,
      text: full,
    );
    final loaded = applyItemBodyPage(
      startItemBodyLoad(beforeDurable, 'session-1', 'persisted-text'),
      'session-1',
      'persisted-text',
      TimelinePage(threadId: 'session-1', watermark: 8, items: [durableItem]),
    );
    final loadedItem = loaded.selectedWorkspace!.items.single;
    final loadedText = (loadedItem.state as ThreadTextItemStateView).text;
    expect(loadedItem.bodyLoaded, isTrue);
    expect(loadedItem.bodyOmittedUnits, 0);
    expect(loadedText, full);
    expect(loadedText.endsWith('NATIVE_ACCEPT_SENTINEL'), isTrue);
    final loadedHistory = loaded.selectedWorkspaceUi.history;
    expect(loadedHistory.previewedItemIds, isNot(contains('persisted-text')));
    expect(loadedHistory.pendingItemBodyIds, isNot(contains('persisted-text')));
  });

  testWidgets('a pending body read stays visible and retryable', (
    tester,
  ) async {
    final item = _threadItemFixture(
      id: 'pending-1',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      text: 'preview…',
    );
    var retried = false;

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'thread-1',
        items: [item],
        previewedItemIds: const {'pending-1'},
        pendingItemBodyIds: const {'pending-1'},
        onLoadItemBody: (_) => retried = true,
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(StudioDriverKeys.timelineItemBodyNotice('pending-1')),
      findsOneWidget,
    );
    expect(
      find.text('Full content is not durable yet; retry shortly.'),
      findsOneWidget,
    );
    await tester.tap(
      find.byKey(StudioDriverKeys.timelineItemBodyRetry('pending-1')),
    );
    await tester.pump();
    expect(retried, isTrue);
  });
}

/// 推理条目两条通道合计驻留的 code unit 数（共享预算的观测口径）。
int _chunkUnits(ThreadThinkingItemStateView state) {
  var total = 0;
  for (final chunk in state.summary) {
    total += chunk.length;
  }
  for (final chunk in state.content) {
    total += chunk.length;
  }
  return total;
}
