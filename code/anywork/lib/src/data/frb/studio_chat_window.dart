part of 'studio_api.dart';

/// 条目的执行终态事实（后端 `BridgeChatLifecycle`），与保存水位 [StudioChatItem.saved]
/// 严格独立。
enum StudioChatLifecycle { streaming, terminal }

class StudioChatItem {
  const StudioChatItem(
    this.item, {
    required this.saved,
    required this.lifecycle,
  });

  final ThreadItemView item;
  final bool saved;
  final StudioChatLifecycle lifecycle;

  bool get terminal => lifecycle == StudioChatLifecycle.terminal;
}

class StudioChatSnapshot {
  const StudioChatSnapshot({
    required this.focusedItemId,
    required this.version,
    required this.items,
    required this.hasOlder,
    required this.hasNewer,
  });

  final String? focusedItemId;
  final int version;
  final List<StudioChatItem> items;
  final bool hasOlder;
  final bool hasNewer;
}

abstract interface class StudioChatWindow {
  Future<StudioChatSnapshot> initial();
  Future<StudioChatSnapshot?> next();
  Future<StudioChatSnapshot> load(TimelineDirection direction);
  Future<StudioChatSnapshot> focus(String? itemId);

  /// 按 identity 请求一条窗口条目的完整正文，并交回**同一权威窗口**的新快照。
  ///
  /// 展开是一次同 view 的窗口操作：原生窗口把该身份完整正文 retain 进窗口，并重新订阅
  /// 交出权威快照。返回快照的版本**就是**后续差量的 `from`，调用方把它当成一次 Reset
  /// 应用即可；窗口仍是唯一正文 owner，绝不另造第二条窗口基线或覆盖层。
  ///
  /// 返回 null 表示该身份已不在窗口、数据源此刻给不出完整正文，或响应到达时窗口已经
  /// 改变；调用方保留预览并等待下一次窗口帧或下一次可见触发，不伪造正文。迟到/被取代的
  /// 展开结果绝不写回新窗口。
  Future<StudioChatSnapshot?> expandItem(String itemId);
  Future<void> close();
}

/// The UI requests another batch only after it has processed the preceding one.
class FrbChatWindow implements StudioChatWindow {
  FrbChatWindow(this._view);

  final frb_chat.BridgeChatView _view;
  StudioChatSnapshot? _baseline;
  bool _closed = false;

  StudioChatItem _item(frb_chat_types.BridgeChatItem item) => StudioChatItem(
    boundThreadItemBody(
      _threadItemFromFrb(item.item).copyWith(
        saved: item.saved,
        executionTerminal:
            item.lifecycle == frb_chat_types.BridgeChatLifecycle.terminal,
      ),
      previewOmittedUnits: _frbInt(item.omittedBytes),
    ),
    saved: item.saved,
    lifecycle: item.lifecycle == frb_chat_types.BridgeChatLifecycle.terminal
        ? StudioChatLifecycle.terminal
        : StudioChatLifecycle.streaming,
  );

  StudioChatSnapshot _adopt(frb_chat_types.BridgeChatSnapshot snapshot) {
    final next = StudioChatSnapshot(
      focusedItemId: snapshot.focus.when(
        latest: () => null,
        around: (itemId) => itemId,
      ),
      version: _frbInt(snapshot.version),
      items: List.unmodifiable(snapshot.items.map(_item)),
      hasOlder: snapshot.hasOlder,
      hasNewer: snapshot.hasNewer,
    );
    _baseline = next;
    // 只读内容交付计数（仅 Driver 启用时统计；不是 FRB 物理 wire 字节）。
    StudioDriverState.recordChatWindowReset(next.items.length);
    for (final entry in next.items) {
      _recordItemBody(entry.item);
    }
    return next;
  }

  Future<StudioChatSnapshot> _reset() async => _adopt(await _view.snapshot());

  @override
  Future<StudioChatSnapshot> initial() async {
    if (_closed) throw StateError('Chat window is closed');
    final baseline = _baseline;
    return baseline ?? _adopt(await _view.initial());
  }

  @override
  Future<StudioChatSnapshot?> next() async {
    if (_closed) return null;
    final update = await _view.next();
    if (_closed || update == null) return null;
    return switch (update) {
      frb_chat_types.BridgeChatUpdate_Reset(:final snapshot) => _adopt(
        snapshot,
      ),
      frb_chat_types.BridgeChatUpdate_Patch(
        :final from,
        :final to,
        :final changes,
        :final hasNewer,
      ) =>
        await _applyPatch(from, to, changes, hasNewer),
    };
  }

  Future<StudioChatSnapshot> _applyPatch(
    BigInt from,
    BigInt to,
    List<frb_chat_types.BridgeViewChange> changes,
    bool hasNewer,
  ) async {
    final baseline = _baseline;
    if (baseline == null || _frbInt(from) != baseline.version) {
      return _reset();
    }
    final visible = [...baseline.items];
    for (final change in changes) {
      switch (change) {
        case frb_chat_types.BridgeViewChange_Splice(
          :final index,
          :final remove,
          :final items,
        ):
          final start = _frbInt(index);
          final count = _frbInt(remove);
          if (start < 0 || count < 0 || start + count > visible.length) {
            return _reset();
          }
          final added = items.map(_item).toList();
          visible.replaceRange(start, start + count, added);
          // 新进窗口的条目正文也是一次真实交付（只读计数，仅 Driver 启用时统计）。
          for (final entry in added) {
            _recordItemBody(entry.item);
          }
        case frb_chat_types.BridgeViewChange_UpdateItem(
          :final itemId,
          :final expectedRevision,
          :final revision,
          :final omittedBytes,
          :final saved,
          :final fields,
        ):
          // 整组字段在**一次**提交里原子应用：本地 revision 必须等于 expectedRevision，
          // 应用完再一次性提交 revision/omitted/saved。终态条目拒绝迟到流式字段，
          // 只接受保存/版本帧；任一条不成立就重建权威窗口，绝不把片段当正文。
          //
          // 字段身份只由 typed discriminator 决定（如 thinkingContent 的 chunkIndex），
          // 不解析 JSON、不按字符串形状推断；真正不匹配的 item 或 revision 一律重建。
          // `Patch.priority`（Immediate/Coalesced）由 core 分类，宿主不自行 sleep/throttle，
          // 因此两档都在此立即应用。
          final index = visible.indexWhere((entry) => entry.item.id == itemId);
          if (index < 0) return _reset();
          final entry = visible[index];
          final updates = <ThreadFieldUpdateView>[
            for (final update in fields)
              ThreadFieldUpdateView(
                field: _contentField(update.field),
                change: _fieldChange(update.change),
              ),
          ];
          for (final update in updates) {
            _recordFieldChange(update.change);
          }
          final nextItem = entry.item.applyFieldUpdates(
            fields: updates,
            expectedRevision: _frbInt(expectedRevision),
            revision: _frbInt(revision),
            omittedUnits: _frbInt(omittedBytes),
            saved: saved,
            terminal: entry.lifecycle == StudioChatLifecycle.terminal,
          );
          if (nextItem == null) return _reset();
          visible[index] = StudioChatItem(
            nextItem,
            saved: saved,
            lifecycle: entry.lifecycle,
          );
      }
    }
    final next = StudioChatSnapshot(
      focusedItemId: baseline.focusedItemId,
      version: _frbInt(to),
      items: List.unmodifiable(visible),
      hasOlder: baseline.hasOlder,
      hasNewer: hasNewer,
    );
    _baseline = next;
    StudioDriverState.recordChatWindowPatch(next.items.length);
    return next;
  }

  /// 记录一次字段变化的正文交付字节（只读计数，仅 Driver 启用时统计）。
  void _recordFieldChange(ThreadFieldChangeView change) {
    switch (change) {
      case AppendThreadFieldChangeView(:final text) ||
          ReplaceThreadFieldChangeView(:final text):
        StudioDriverState.recordContentDelta(text);
      case UnchangedThreadFieldChangeView() || RemoveThreadFieldChangeView():
        break;
    }
  }

  /// 记录整条正文交付的 UTF-8 字节（从已解码的 domain 字段取，不序列化 JSON）。
  void _recordItemBody(ThreadItemView item) {
    switch (item.state) {
      case ThreadTextItemStateView(:final text):
        StudioDriverState.recordContentBody(text);
      case ThreadThinkingItemStateView(:final summary, :final content):
        for (final chunk in summary) {
          StudioDriverState.recordContentBody(chunk);
        }
        for (final chunk in content) {
          StudioDriverState.recordContentBody(chunk);
        }
      case ThreadToolItemStateView(:final invocation, :final lifecycle):
        StudioDriverState.recordContentBody(invocation.arguments);
        switch (lifecycle) {
          case RunningThreadToolView(:final streamedOutput) ||
              CancellingThreadToolView(:final streamedOutput):
            StudioDriverState.recordContentBody(streamedOutput);
          case SucceededThreadToolView(:final output):
            StudioDriverState.recordContentBody(output.result);
          case FailedThreadToolView(:final output):
            StudioDriverState.recordContentBody(output?.result ?? '');
          default:
            break;
        }
      default:
        break;
    }
  }

  @override
  Future<StudioChatSnapshot> load(TimelineDirection direction) async {
    if (_closed) throw StateError('Chat window is closed');
    return _adopt(
      await _view.load(
        direction: switch (direction) {
          TimelineDirection.older => frb_chat_types.BridgeChatDirection.older,
          TimelineDirection.newer => frb_chat_types.BridgeChatDirection.newer,
        },
      ),
    );
  }

  @override
  Future<StudioChatSnapshot> focus(String? itemId) async {
    if (_closed) throw StateError('Chat window is closed');
    return _adopt(
      await _view.focus(
        focus: itemId == null
            ? const frb_chat_types.BridgeChatFocus.latest()
            : frb_chat_types.BridgeChatFocus.around(itemId: itemId),
      ),
    );
  }

  @override
  Future<StudioChatSnapshot?> expandItem(String itemId) async {
    if (_closed) return null;
    final baseline = _baseline;
    if (baseline == null) return null;
    // 原生窗口是唯一正文 owner：请求该身份完整正文后重新订阅，交回权威快照。
    // 迟到/被取代的结果不写回：只接纳仍与当前窗口基线一致（身份仍在窗口）的返回。
    final snapshot = await _view.expand(itemId: itemId);
    if (_closed) return null;
    return _adopt(snapshot);
  }

  ThreadContentFieldView _contentField(
    frb_chat_types.BridgeContentField field,
  ) {
    return field.when(
      text: () => const ThreadTextFieldView(),
      thinkingSummary: (chunkIndex) =>
          ThreadThinkingSummaryFieldView(chunkIndex),
      thinkingContent: (chunkIndex) =>
          ThreadThinkingContentFieldView(chunkIndex),
      toolArguments: () => const ThreadToolArgumentsFieldView(),
      toolResult: () => const ThreadToolResultFieldView(),
    );
  }

  ThreadFieldChangeView _fieldChange(frb_chat_types.BridgeFieldChange change) {
    return change.when(
      unchanged: () => const UnchangedThreadFieldChangeView(),
      append: (text) => AppendThreadFieldChangeView(text),
      replace: (text) => ReplaceThreadFieldChangeView(text),
      remove: () => const RemoveThreadFieldChangeView(),
    );
  }

  @override
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    try {
      await _view.close();
    } finally {
      _view.dispose();
    }
  }
}
