part of 'studio_api.dart';

class StudioChatItem {
  const StudioChatItem(this.item, {required this.saved});

  final ThreadItemView item;
  final bool saved;
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
  Future<ThreadItemView?> readItem(String itemId);
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
      _threadItemFromFrb(item.item).copyWith(saved: item.saved),
      previewOmittedUnits: _frbInt(item.omittedBytes),
    ),
    saved: item.saved,
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
    return next;
  }

  Future<StudioChatSnapshot> _reset() async => _adopt(await _view.snapshot());

  @override
  Future<StudioChatSnapshot> initial() async {
    if (_closed) throw StateError('Chat window is closed');
    return _baseline ?? _adopt(await _view.initial());
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
          visible.replaceRange(start, start + count, items.map(_item));
        case frb_chat_types.BridgeViewChange_AppendText():
          // The product currently encodes full typed items. A future plain-text part
          // must be decoded with its own type instead of appending into a JSON body.
          return _reset();
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
    return next;
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
  Future<ThreadItemView?> readItem(String itemId) async {
    if (_closed) throw StateError('Chat window is closed');
    final item = await _view.readItem(itemId: itemId);
    return item == null
        ? null
        : _threadItemFromFrb(item.item)
              .copyWith(bodyLoaded: true, saved: item.saved);
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
