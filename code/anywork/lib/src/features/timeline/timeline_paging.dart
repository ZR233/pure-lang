part of 'timeline_view.dart';

extension on _TimelineViewState {
  double _estimateRowExtent(int? index, double width) {
    // A positive null-index result tells SuperSliverList that EVERY unmeasured
    // row has that height, bypassing the per-row estimates below entirely.
    if (index == null) return 0;
    if (index >= widget.rows.length) {
      return widget.planConfirmation == null ? 14 : 80;
    }
    final row = widget.rows[index];
    final text = row.part?.text;
    if (text != null && text.isNotEmpty) {
      final scale = _textScale;
      final prompt =
          row.type == TimelineRowType.userMessage ||
          row.type == TimelineRowType.parentAgentMessage;
      final bodyWidth =
          math.min(width, prompt ? 560.0 : 700.0) -
          (row.type == TimelineRowType.userMessage ? 28 : 0);
      final columns = math.max(1.0, bodyWidth / (8 * scale));
      // Explicit line breaks matter even when each line is only a few letters.
      // Do not construct off-screen Markdown merely to measure the scrollbar.
      var lines = 0;
      for (final line in LineSplitter.split(text)) {
        lines += math.max(1, (line.length / columns).ceil());
      }
      return 56 + lines * 22.0 * scale;
    }
    return switch (row.type) {
      TimelineRowType.toolGroup => 110,
      TimelineRowType.reasoningSummary => 80,
      _ => 100,
    };
  }

  Widget _buildRow(TimelineRow row) {
    final reasoningExpanded = _expandedReasoningGroups.contains(
      row.reasoningGroup?.id,
    );
    final toolExpanded = _expandedToolGroups.contains(row.toolGroup?.id);
    final version = row.renderVersion;
    final bodyStates = _itemBodyStates(row);
    final bodyState = _itemBodyCacheKey(bodyStates);
    final cached = _rowWidgets[row.id];
    final defer =
        _deferStreamingUpdates &&
        cached != null &&
        row.part != null &&
        !isTerminalTimelineStatus(row.part!.status);
    final needsUpdate =
        cached == null ||
        cached.version != version ||
        cached.expanded != reasoningExpanded ||
        cached.toolExpanded != toolExpanded ||
        cached.body != bodyState;
    if (defer && needsUpdate) {
      _deferredRowIds.add(row.id);
    } else {
      _deferredRowIds.remove(row.id);
    }
    if (!defer && needsUpdate) {
      _rowWidgets[row.id] = (
        version: version,
        expanded: reasoningExpanded,
        toolExpanded: toolExpanded,
        body: bodyState,
        child: _TimelineRowBlock(
          key: ValueKey(row.id),
          row: row,
          isReasoningExpanded: reasoningExpanded,
          onToggleReasoning: _toggleReasoning,
          isToolGroupExpanded: toolExpanded,
          onToggleToolGroup: _toggleToolGroup,
          onToolDetailsChanged: _handleToolDetailsChanged,
          body: bodyStates,
        ),
      );
    }
    return KeyedSubtree(
      key: ValueKey(row.id),
      child: KeyedSubtree(
        key: StudioDriverKeys.timelineBlock(row.id),
        child: RepaintBoundary(
          child: SizedBox(
            key: _rowKeys.putIfAbsent(row.id, GlobalKey.new),
            child: _rowWidgets[row.id]!.child,
          ),
        ),
      ),
    );
  }

  String _anchorItemId(String rowId, List<TimelineRow> rows) {
    final row = rows.where((row) => row.id == rowId).firstOrNull;
    return row?.part?.id ??
        row?.toolGroup?.items.firstOrNull?.id ??
        row?.reasoningGroup?.parts.firstOrNull?.id ??
        rowId;
  }

  /// 行缓存键里的完整正文状态：载荷/加载/错误变化时重建该行，阅读位置不变。
  ///
  /// 键包含 canonical item id，因此分组行内某一底层条目回源完成/失败时，只有该行重建，
  /// 折叠状态与阅读锚点都不受影响。
  String? _itemBodyCacheKey(List<_ItemBodyState> states) {
    if (states.isEmpty) return null;
    return states
        .map(
          (state) =>
              '${state.itemId}|${state.isPreviewed}|${state.isLoading}|'
              '${state.isPending}|${state.isUnavailable}|${state.error ?? ''}',
        )
        .join(',');
  }

  /// 该行需要“回源完整正文”提示的底层条目；不需要时返回空列表。
  ///
  /// 单条行、raw 行与分组行都按 canonical item id 解析：分组行的 `row.id` 是合成身份
  /// （`tool-group:`/`reasoning-group:`），不能直接拿去查预览/回源状态，否则超大工具输出、
  /// 推理正文或 raw 载荷永远拿不到回源入口。
  List<_ItemBodyState> _itemBodyStates(TimelineRow row) {
    final states = <_ItemBodyState>[];
    for (final candidate in _rowBodyCandidates(row)) {
      final state = _itemBodyState(
        candidate.id,
        candidate.label,
        autoComplete: candidate.isText,
      );
      if (state != null) states.add(state);
    }
    return states;
  }

  /// 一行底层承载的条目身份：单条/raw 行是自身条目，分组行是其全部成员条目。
  ///
  /// [isText] 标记该身份的正文由窗口按身份自动补齐：它不再需要“加载完整内容”入口。
  List<({String id, String? label, bool isText})> _rowBodyCandidates(
    TimelineRow row,
  ) {
    final isTextRow = switch (row.type) {
      TimelineRowType.userMessage ||
      TimelineRowType.parentAgentMessage ||
      TimelineRowType.commentary ||
      TimelineRowType.finalAnswer => true,
      _ => false,
    };
    if (row.part case final part?) {
      return <({String id, String? label, bool isText})>[
        (id: part.id, label: null, isText: isTextRow),
      ];
    }
    if (row.toolGroup case final group?) {
      return <({String id, String? label, bool isText})>[
        for (final item in group.items)
          (id: item.id, label: item.name, isText: false),
      ];
    }
    if (row.reasoningGroup case final group?) {
      return <({String id, String? label, bool isText})>[
        for (final part in group.parts)
          (id: part.id, label: null, isText: false),
      ];
    }
    return const [];
  }

  /// 单个底层条目的“回源完整正文”提示；不需要时返回 null。
  ///
  /// [autoComplete] 的正文由窗口按身份自动补齐，因此“预览态”与它自己的加载态都不产生提示：
  /// 读者既不需要点击加载，也不会看到分页/折叠入口。只有补齐失败与“尚未 durable”如实提示。
  _ItemBodyState? _itemBodyState(
    String itemId,
    String? label, {
    bool autoComplete = false,
  }) {
    final previewed = widget.previewedItemIds.contains(itemId);
    final loading = widget.loadingItemIds.contains(itemId);
    final error = widget.itemBodyErrors[itemId];
    final pending = widget.pendingItemBodyIds.contains(itemId);
    final unavailable = widget.unavailableItemIds.contains(itemId);
    if (autoComplete && !pending && !unavailable && error == null) {
      return null;
    }
    if (!previewed && !loading && !pending && !unavailable && error == null) {
      return null;
    }
    return _ItemBodyState(
      itemId: itemId,
      label: label,
      isPreviewed: previewed,
      isLoading: loading,
      isPending: pending,
      isUnavailable: unavailable,
      error: error,
      onLoad: widget.onLoadItemBody == null
          ? null
          : () => widget.onLoadItemBody!(itemId),
    );
  }

  String? _anchorRowId(String itemId, {List<TimelineRow>? rows}) {
    for (final row in rows ?? widget.rows) {
      if (row.id == itemId ||
          row.part?.id == itemId ||
          row.toolGroup?.items.any((item) => item.id == itemId) == true ||
          row.reasoningGroup?.parts.any((item) => item.id == itemId) == true) {
        return row.id;
      }
    }
    return null;
  }

  bool _needsAnchorRebase(TimelineAnchor anchor, List<TimelineRow> oldRows) {
    final oldId = _anchorRowId(anchor.itemId, rows: oldRows);
    final newId = _anchorRowId(anchor.itemId);
    final oldIndex = oldRows.indexWhere((row) => row.id == oldId);
    final newIndex = widget.rows.indexWhere((row) => row.id == newId);
    if (oldIndex != newIndex || oldId != newId) return true;
    for (var index = 0; index <= oldIndex; index++) {
      if (oldRows[index].id != widget.rows[index].id ||
          oldRows[index].renderVersion != widget.rows[index].renderVersion) {
        return true;
      }
    }
    return false;
  }

  /// 记录“最靠上的可见行”的锚点。
  ///
  /// 记录的偏移量是**内容自身**的偏移：短内容整体贴底时整段内容被
  /// [_BottomAlignedSliver] 下移了 `_bottomSlack`，这里减掉它，锚点就与
  /// 视口是否贴底无关。恢复位置仍然只用 `-offset`，不需要知道当前留白。
  TimelineAnchor? _captureAnchor({List<TimelineRow>? rows}) {
    final viewport = _viewportKey.currentContext?.findRenderObject();
    if (viewport is! RenderBox || !viewport.hasSize) return null;
    String? id;
    double? offset;
    for (final entry in _rowKeys.entries) {
      final box = entry.value.currentContext?.findRenderObject();
      if (box is! RenderBox || !box.hasSize || !box.attached) continue;
      final top = box.localToGlobal(Offset.zero, ancestor: viewport).dy;
      if (top + box.size.height <= 0 || top >= viewport.size.height) continue;
      if (offset == null || top < offset) {
        id = entry.key;
        offset = top;
      }
    }
    return id == null
        ? null
        : TimelineAnchor(
            _anchorItemId(id, rows ?? widget.rows),
            offset! - _bottomSlack,
            followingBottom:
                _followingBottom && !_detachedByUser && !widget.hasNewer,
          );
  }

  void _schedulePrefetch() {
    if (_prefetchScheduled) return;
    _prefetchScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _prefetchScheduled = false;
      if (!mounted ||
          _pointerHeld ||
          widget.isLoadingOlder ||
          widget.isLoadingNewer) {
        return;
      }
      final position = _controller.hasClients ? _controller.position : null;
      // Use actual layout: raw item counts do not predict the height after grouping.
      final underfull = position == null
          ? widget.rows.isEmpty
          : position.maxScrollExtent - position.minScrollExtent < 1;
      if (_followingBottom && !_detachedByUser) {
        // Filling a latest viewport must not change focus or evict the live tail.
        // The backend owns capacity; a full bounded window never falls through to browsing.
        if (underfull &&
            !widget.hasNewer &&
            widget.onExtendLatest != null &&
            widget.olderError == null &&
            !_olderLoadRequested) {
          _olderLoadRequested = true;
          widget.onExtendLatest!();
        }
        return;
      }
      // History reading advances only in its chosen direction, even when underfull.
      // Reversing automatically at an edge would oscillate a bounded window.
      final threshold = 1.5 * (position?.viewportDimension ?? 0);
      if (_scrollingOlder &&
          (_detachedByUser || underfull) &&
          (underfull ||
              (position?.extentBefore ?? double.infinity) < threshold) &&
          widget.onLoadOlder != null &&
          widget.olderError == null &&
          !_olderLoadRequested) {
        _olderLoadRequested = true;
        widget.onLoadOlder!();
      } else if (!_scrollingOlder &&
          (underfull ||
              (position?.extentAfter ?? double.infinity) < threshold) &&
          widget.onLoadNewer != null &&
          widget.newerError == null &&
          !_newerLoadRequested) {
        _newerLoadRequested = true;
        widget.onLoadNewer!();
      }
    });
  }

  void _updateLoadingIndicator() {
    if (!widget.isLoadingOlder && !widget.isLoadingNewer) {
      _loadingTimer?.cancel();
      _loadingTimer = null;
      _showLoading = false;
    } else {
      _loadingTimer ??= Timer(const Duration(milliseconds: 150), () {
        _showLoadingIndicator();
      });
    }
  }

  Widget _edgeIndicator({required bool older}) {
    final error = older ? widget.olderError : widget.newerError;
    return Positioned(
      top: older ? 8 : null,
      bottom: older ? null : 8,
      left: 24,
      right: 24,
      child: Center(
        child: error == null
            ? SizedBox.square(
                key: ValueKey(
                  older ? 'timeline-history-loading' : 'timeline-newer-loading',
                ),
                dimension: 18,
                child: const CircularProgressIndicator(strokeWidth: 2),
              )
            : Tooltip(
                message: error,
                child: TextButton.icon(
                  key: ValueKey(
                    older ? 'timeline-history-retry' : 'timeline-newer-retry',
                  ),
                  onPressed: older
                      ? (_followingBottom && !_detachedByUser
                            ? widget.onExtendLatest
                            : widget.onLoadOlder)
                      : widget.onLoadNewer,
                  icon: const Icon(Icons.refresh),
                  label: Text(context.l10n.timelineImageRetry),
                ),
              ),
      ),
    );
  }
}
