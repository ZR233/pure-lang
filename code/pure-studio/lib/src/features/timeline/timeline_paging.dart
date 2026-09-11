part of 'timeline_view.dart';

extension on _TimelineViewState {
  Widget _itemSliver(List<_TimelineDisplayBlock> blocks, {Key? key}) {
    return SliverPadding(
      key: key,
      padding: const EdgeInsets.symmetric(horizontal: 24),
      sliver: SliverList(
        key: ValueKey((key == _centerKey, _centerId)),
        delegate: SliverChildBuilderDelegate(
          (context, index) {
            final block = blocks[index];
            final expanded = _expandedReasoningGroups.contains(
              block.rows.single.reasoningGroup?.id,
            );
            final version = block.rows.single.renderVersion;
            final cached = _rowWidgets[block.id];
            if (cached == null ||
                cached.version != version ||
                cached.expanded != expanded) {
              _rowWidgets[block.id] = (
                version: version,
                expanded: expanded,
                child: _TimelineRowBlock(
                  key: ValueKey(block.id),
                  row: block.rows.single,
                  isCurrentActivity: block.isCurrentActivity,
                  isReasoningExpanded: expanded,
                  onToggleReasoning: _toggleReasoning,
                ),
              );
            }
            return KeyedSubtree(
              key: StudioDriverKeys.timelineBlock(block.id),
              child: SizedBox(
                key: _rowKeys.putIfAbsent(block.id, GlobalKey.new),
                child: _rowWidgets[block.id]!.child,
              ),
            );
          },
          childCount: blocks.length,
          findChildIndexCallback: (key) {
            final index = blocks.indexWhere(
              (block) => StudioDriverKeys.timelineBlock(block.id) == key,
            );
            return index < 0 ? null : index;
          },
        ),
      ),
    );
  }

  String _anchorItemId(String rowId) {
    final row = widget.rows.where((row) => row.id == rowId).firstOrNull;
    return row?.part?.id ??
        row?.toolGroup?.items.firstOrNull?.id ??
        row?.reasoningGroup?.parts.firstOrNull?.id ??
        rowId;
  }

  String? _anchorRowId(String itemId) {
    for (final row in widget.rows) {
      if (row.id == itemId ||
          row.part?.id == itemId ||
          row.toolGroup?.items.any((item) => item.id == itemId) == true ||
          row.reasoningGroup?.parts.any((item) => item.id == itemId) == true) {
        return row.id;
      }
    }
    return null;
  }

  TimelineAnchor? _captureAnchor() {
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
            _anchorItemId(id),
            offset!,
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
          !_controller.hasClients ||
          widget.isLoadingOlder ||
          widget.isLoadingNewer) {
        return;
      }
      final position = _controller.position;
      final underfull = position.maxScrollExtent - position.minScrollExtent < 1;
      final threshold = 1.5 * position.viewportDimension;
      if ((_scrollingOlder || underfull) &&
          position.extentBefore < threshold &&
          widget.onLoadOlder != null &&
          widget.olderError == null &&
          !_olderLoadRequested) {
        _olderLoadRequested = true;
        widget.onLoadOlder!();
      } else if ((!_scrollingOlder || underfull) &&
          position.extentAfter < threshold &&
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
                  onPressed: older ? widget.onLoadOlder : widget.onLoadNewer,
                  icon: const Icon(Icons.refresh),
                  label: Text(context.l10n.timelineImageRetry),
                ),
              ),
      ),
    );
  }
}
