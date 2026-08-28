import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:gpt_markdown/custom_widgets/markdown_config.dart';
import 'package:gpt_markdown/gpt_markdown.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../data/repositories/studio_repository.dart';
import '../../l10n/studio_l10n.dart';
import '../../platform/external_url_launcher.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import 'markdown_repair.dart';

part 'timeline_blocks.dart';
part 'timeline_markdown_blocks.dart';
part 'timeline_plan_agent_blocks.dart';
part 'timeline_tool_blocks.dart';
part 'timeline_wait_indicator.dart';

class TimelineView extends StatefulWidget {
  const TimelineView({
    required this.threadId,
    required this.rows,
    required this.turn,
    this.onLoadOlder,
    this.isLoadingOlder = false,
    super.key,
  });

  final String? threadId;
  final List<TimelineRow> rows;
  final StudioTurnView? turn;
  final VoidCallback? onLoadOlder;
  final bool isLoadingOlder;

  @override
  State<TimelineView> createState() => _TimelineViewState();
}

class _TimelineViewState extends State<TimelineView> {
  static const _bottomThreshold = 80.0;
  static const _scrollDuration = Duration(milliseconds: 180);

  final ScrollController _controller = ScrollController();
  final Map<String, _TimelineScrollSnapshot> _threadScroll = {};
  final Set<String> _expandedReasoningGroups = {};
  bool _followingBottom = true;
  bool _detachedByUser = false;
  bool _programmaticScroll = false;
  bool _bottomScrollScheduled = false;
  bool _scrollBoundsCorrectionScheduled = false;
  bool _olderLoadRequested = false;
  int _pendingNewEvents = 0;
  int _contentVersion = 0;
  _BottomScrollIntent _scheduledBottomIntent = _BottomScrollIntent.jump;
  _TimelineRestore _pendingRestore = const _TimelineRestore.bottom();

  /// 最近一次非空选区的文本。
  ///
  /// Flutter 3.47 桌面端右键会把选区折叠后再构建菜单,菜单的 Copy 只能
  /// 依赖此缓存执行;线程切换时失效。
  String? _lastSelectedText;

  void _handleTimelineSelectionChanged(SelectedContent? content) {
    final text = content?.plainText;
    if (text != null && text.isNotEmpty) {
      _lastSelectedText = text;
    }
  }

  Widget _buildTimelineContextMenu(
    BuildContext context,
    SelectableRegionState selectableRegion,
  ) {
    final items = selectableRegion.contextMenuButtonItems;
    final hasCopy = items.any(
      (item) => item.type == ContextMenuButtonType.copy,
    );
    final cached = _lastSelectedText;
    if (!hasCopy && cached != null && cached.isNotEmpty) {
      items.insert(
        0,
        ContextMenuButtonItem(
          type: ContextMenuButtonType.copy,
          onPressed: () {
            Clipboard.setData(ClipboardData(text: cached));
            selectableRegion.hideToolbar();
          },
        ),
      );
    }
    return AdaptiveTextSelectionToolbar.buttonItems(
      buttonItems: items,
      anchors: selectableRegion.contextMenuAnchors,
    );
  }

  @override
  void initState() {
    super.initState();
    _contentVersion = _timelineContentVersion(widget.rows, widget.turn);
    _restoreThreadState();
    _controller.addListener(_handleScrollPositionChanged);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        _restorePendingPosition();
      }
    });
  }

  @override
  void didUpdateWidget(covariant TimelineView oldWidget) {
    super.didUpdateWidget(oldWidget);
    final threadChanged = widget.threadId != oldWidget.threadId;
    if (threadChanged) {
      _saveThreadState(oldWidget.threadId);
      _expandedReasoningGroups.clear();
      _lastSelectedText = null;
      _restoreThreadState();
      _contentVersion = _timelineContentVersion(widget.rows, widget.turn);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) {
          _restorePendingPosition();
        }
      });
      return;
    }
    if (oldWidget.isLoadingOlder && !widget.isLoadingOlder) {
      _olderLoadRequested = false;
    }

    final nextContentVersion = _timelineContentVersion(
      widget.rows,
      widget.turn,
    );
    if (nextContentVersion == _contentVersion) {
      return;
    }
    final wasNearBottom = _isNearBottom();
    final prepended = _hasPrependedTimelineRows(oldWidget.rows, widget.rows);
    final evictedLeading = _hasEvictedLeadingTimelineRows(
      oldWidget.rows,
      widget.rows,
    );
    final previousExtent = _controller.hasClients
        ? _controller.position.maxScrollExtent
        : 0.0;
    final previousPixels = _controller.hasClients
        ? _controller.position.pixels
        : 0.0;
    final hasNewEvent = _hasNewTimelineEvent(oldWidget, widget);
    _contentVersion = nextContentVersion;

    if (prepended && _controller.hasClients && !wasNearBottom) {
      _followingBottom = false;
      _detachedByUser = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || !_controller.hasClients) {
          return;
        }
        final insertedExtent =
            _controller.position.maxScrollExtent - previousExtent;
        final target = (previousPixels + insertedExtent)
            .clamp(
              _controller.position.minScrollExtent,
              _controller.position.maxScrollExtent,
            )
            .toDouble();
        _programmaticScroll = true;
        try {
          _controller.jumpTo(target);
        } finally {
          _programmaticScroll = false;
        }
        _saveThreadState(widget.threadId);
      });
      return;
    }

    if (evictedLeading && _controller.hasClients && !wasNearBottom) {
      // 历史窗口驱逐了最旧一页：按移除内容高度回补偏移，保持视口内容稳定。
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || !_controller.hasClients) {
          return;
        }
        final removedExtent =
            previousExtent - _controller.position.maxScrollExtent;
        if (removedExtent <= 0) return;
        final target = (previousPixels - removedExtent)
            .clamp(
              _controller.position.minScrollExtent,
              _controller.position.maxScrollExtent,
            )
            .toDouble();
        _programmaticScroll = true;
        try {
          _controller.jumpTo(target);
        } finally {
          _programmaticScroll = false;
        }
        _saveThreadState(widget.threadId);
      });
      return;
    }

    if (!_detachedByUser && (_followingBottom || wasNearBottom)) {
      _followingBottom = true;
      _detachedByUser = false;
      _pendingNewEvents = 0;
      _scheduleBottomScroll(
        hasNewEvent ? _BottomScrollIntent.animate : _BottomScrollIntent.jump,
      );
    } else {
      _followingBottom = false;
      _detachedByUser = true;
      if (hasNewEvent) {
        _pendingNewEvents += 1;
      }
    }
  }

  @override
  void dispose() {
    _saveThreadState(widget.threadId);
    _controller.removeListener(_handleScrollPositionChanged);
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final activeTurn = widget.turn?.state.isBusy == true ? widget.turn : null;
    final currentActivityRow = _currentActivityRow(widget.rows, activeTurn);
    if (widget.rows.isEmpty && activeTurn == null) {
      return const _EmptyTimeline();
    }
    final blocks = _timelineDisplayBlocks(
      widget.rows,
      currentActivityRowId: currentActivityRow?.id,
    );
    final activityCount = activeTurn == null ? 0 : 1;
    return SelectionArea(
      onSelectionChanged: _handleTimelineSelectionChanged,
      contextMenuBuilder: _buildTimelineContextMenu,
      child: Stack(
        children: [
          Align(
            alignment: Alignment.topCenter,
            child: ConstrainedBox(
              constraints: const BoxConstraints(
                maxWidth: StudioLayout.conversationWidth,
              ),
              child: NotificationListener<ScrollMetricsNotification>(
                onNotification: _handleScrollMetricsChanged,
                child: ListView.builder(
                  key: StudioDriverKeys.timeline,
                  controller: _controller,
                  padding: const EdgeInsets.fromLTRB(24, 28, 24, 38),
                  itemCount: blocks.length + activityCount + 1,
                  findChildIndexCallback: (key) {
                    if (key is! ValueKey<String>) {
                      return null;
                    }
                    if (activeTurn != null &&
                        key ==
                            StudioDriverKeys.turnActivity(
                              _turnActivityId(activeTurn),
                            )) {
                      return blocks.length;
                    }
                    final index = blocks.indexWhere(
                      (block) =>
                          StudioDriverKeys.timelineBlock(block.id) == key,
                    );
                    return index == -1 ? null : index;
                  },
                  itemBuilder: (context, index) {
                    if (activeTurn != null && index == blocks.length) {
                      return _TurnActivityBlock(
                        key: StudioDriverKeys.turnActivity(
                          _turnActivityId(activeTurn),
                        ),
                        turn: activeTurn,
                        reasoningGroup: currentActivityRow?.reasoningGroup,
                        toolGroup: currentActivityRow?.toolGroup,
                        reasoningExpanded: _expandedReasoningGroups.contains(
                          currentActivityRow?.reasoningGroup?.id,
                        ),
                        onToggleReasoning: () {
                          final group = currentActivityRow?.reasoningGroup;
                          if (group != null) {
                            _toggleReasoning(group.id);
                          }
                        },
                      );
                    }
                    if (index == blocks.length + activityCount) {
                      return const SizedBox(height: 24);
                    }
                    final block = blocks[index];
                    return _TimelineRowBlock(
                      key: StudioDriverKeys.timelineBlock(block.id),
                      row: block.rows.single,
                      isCurrentActivity: block.isCurrentActivity,
                      isReasoningExpanded: _expandedReasoningGroups.contains(
                        block.rows.single.reasoningGroup?.id,
                      ),
                      onToggleReasoning: _toggleReasoning,
                    );
                  },
                ),
              ),
            ),
          ),
          if (_showJumpToLatest)
            Positioned.fill(
              child: Align(
                alignment: Alignment.bottomCenter,
                child: ConstrainedBox(
                  constraints: const BoxConstraints(
                    maxWidth: StudioLayout.conversationWidth,
                  ),
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(24, 0, 24, 16),
                    child: Align(
                      alignment: Alignment.bottomRight,
                      child: _JumpToLatestButton(
                        pendingCount: _pendingNewEvents,
                        onPressed: _jumpToLatest,
                      ),
                    ),
                  ),
                ),
              ),
            ),
          if (widget.isLoadingOlder)
            const Positioned(
              top: 8,
              left: 0,
              right: 0,
              child: Center(
                child: SizedBox.square(
                  key: ValueKey('timeline-history-loading'),
                  dimension: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
              ),
            ),
        ],
      ),
    );
  }

  bool get _showJumpToLatest {
    return widget.rows.isNotEmpty &&
        (_detachedByUser || _pendingNewEvents > 0 || !_isNearBottom());
  }

  bool _isNearBottom() {
    if (!_controller.hasClients) {
      return true;
    }
    return _controller.position.extentAfter <= _bottomThreshold;
  }

  void _handleScrollPositionChanged() {
    if (!_controller.hasClients || _programmaticScroll) {
      return;
    }
    final nearBottom = _isNearBottom();
    if (nearBottom) {
      if (!_followingBottom || _detachedByUser || _pendingNewEvents != 0) {
        setState(() {
          _followingBottom = true;
          _detachedByUser = false;
          _pendingNewEvents = 0;
        });
      }
    } else if (_followingBottom || !_detachedByUser) {
      setState(() {
        _followingBottom = false;
        _detachedByUser = true;
      });
    }
    _saveThreadState(widget.threadId);
    if (_controller.position.extentBefore <= _bottomThreshold &&
        widget.onLoadOlder != null &&
        !widget.isLoadingOlder &&
        !_olderLoadRequested) {
      _olderLoadRequested = true;
      widget.onLoadOlder!();
    }
  }

  bool _handleScrollMetricsChanged(ScrollMetricsNotification notification) {
    final metrics = notification.metrics;
    if (metrics.axis != Axis.vertical ||
        (metrics.pixels >= metrics.minScrollExtent &&
            metrics.pixels <= metrics.maxScrollExtent) ||
        _scrollBoundsCorrectionScheduled) {
      return false;
    }
    _scrollBoundsCorrectionScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _scrollBoundsCorrectionScheduled = false;
      if (!mounted || !_controller.hasClients) {
        return;
      }
      final position = _controller.position;
      final target =
          (_followingBottom && !_detachedByUser
                  ? position.maxScrollExtent
                  : position.pixels.clamp(
                      position.minScrollExtent,
                      position.maxScrollExtent,
                    ))
              .toDouble();
      if ((position.pixels - target).abs() <= 0.5) {
        return;
      }
      _programmaticScroll = true;
      try {
        _controller.jumpTo(target);
      } finally {
        _programmaticScroll = false;
      }
      _saveThreadState(widget.threadId);
    });
    return false;
  }

  void _scheduleBottomScroll(_BottomScrollIntent intent) {
    if (intent == _BottomScrollIntent.animate) {
      _scheduledBottomIntent = _BottomScrollIntent.animate;
    }
    if (_bottomScrollScheduled) {
      return;
    }
    _bottomScrollScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) {
        return;
      }
      final scheduledIntent = _scheduledBottomIntent;
      _scheduledBottomIntent = _BottomScrollIntent.jump;
      _bottomScrollScheduled = false;
      _scrollToBottom(animated: scheduledIntent == _BottomScrollIntent.animate);
    });
  }

  Future<void> _scrollToBottom({required bool animated}) async {
    if (!_controller.hasClients) {
      return;
    }
    final target = _controller.position.maxScrollExtent;
    _programmaticScroll = true;
    try {
      if (animated && (_controller.position.pixels - target).abs() > 1) {
        await _controller.animateTo(
          target,
          duration: _scrollDuration,
          curve: Curves.easeOutCubic,
        );
      } else {
        _controller.jumpTo(target);
      }
    } finally {
      _programmaticScroll = false;
    }
    if (!mounted) {
      return;
    }
    setState(() {
      _followingBottom = true;
      _detachedByUser = false;
      _pendingNewEvents = 0;
    });
    _saveThreadState(widget.threadId);
  }

  void _jumpToLatest() {
    unawaited(_scrollToBottom(animated: true));
  }

  void _toggleReasoning(String groupId) {
    setState(() {
      if (!_expandedReasoningGroups.remove(groupId)) {
        _expandedReasoningGroups.add(groupId);
      }
    });
  }

  void _restoreThreadState() {
    final threadId = widget.threadId;
    final snapshot = threadId == null ? null : _threadScroll[threadId];
    if (snapshot == null) {
      _followingBottom = true;
      _detachedByUser = false;
      _pendingNewEvents = 0;
      _pendingRestore = const _TimelineRestore.bottom();
      return;
    }
    _followingBottom = snapshot.followingBottom;
    _detachedByUser = snapshot.detachedByUser;
    _pendingNewEvents = snapshot.pendingNewEvents;
    _pendingRestore = snapshot.followingBottom
        ? const _TimelineRestore.bottom()
        : _TimelineRestore.offset(snapshot.pixels);
  }

  void _restorePendingPosition() {
    if (!_controller.hasClients) {
      return;
    }
    switch (_pendingRestore.kind) {
      case _TimelineRestoreKind.bottom:
        _scrollToBottom(animated: false);
      case _TimelineRestoreKind.offset:
        final target = _pendingRestore.pixels
            .clamp(
              _controller.position.minScrollExtent,
              _controller.position.maxScrollExtent,
            )
            .toDouble();
        _programmaticScroll = true;
        try {
          _controller.jumpTo(target);
        } finally {
          _programmaticScroll = false;
        }
        _saveThreadState(widget.threadId);
    }
  }

  void _saveThreadState(String? threadId) {
    if (threadId == null || !_controller.hasClients) {
      return;
    }
    _threadScroll[threadId] = _TimelineScrollSnapshot(
      pixels: _controller.position.pixels,
      followingBottom: _followingBottom && _isNearBottom(),
      detachedByUser: _detachedByUser || !_isNearBottom(),
      pendingNewEvents: _pendingNewEvents,
    );
  }
}

enum _BottomScrollIntent { jump, animate }

enum _TimelineRestoreKind { bottom, offset }

class _TimelineRestore {
  const _TimelineRestore.bottom()
    : kind = _TimelineRestoreKind.bottom,
      pixels = 0;

  const _TimelineRestore.offset(this.pixels)
    : kind = _TimelineRestoreKind.offset;

  final _TimelineRestoreKind kind;
  final double pixels;
}

class _TimelineScrollSnapshot {
  const _TimelineScrollSnapshot({
    required this.pixels,
    required this.followingBottom,
    required this.detachedByUser,
    required this.pendingNewEvents,
  });

  final double pixels;
  final bool followingBottom;
  final bool detachedByUser;
  final int pendingNewEvents;
}

int _timelineContentVersion(List<TimelineRow> rows, StudioTurnView? turn) {
  return Object.hashAll([
    turn,
    rows.length,
    for (final row in rows) ...[row.id, row.type, row.renderVersion],
  ]);
}

class _TimelineDisplayBlock {
  const _TimelineDisplayBlock._(
    this.rows, {
    required this.id,
    this.isCurrentActivity = false,
  });

  factory _TimelineDisplayBlock.single(TimelineRow row) {
    return _TimelineDisplayBlock._([row], id: row.id);
  }

  final List<TimelineRow> rows;
  final String id;
  final bool isCurrentActivity;

  _TimelineDisplayBlock asCurrentActivity() {
    return _TimelineDisplayBlock._(rows, id: id, isCurrentActivity: true);
  }
}

List<_TimelineDisplayBlock> _timelineDisplayBlocks(
  List<TimelineRow> rows, {
  String? currentActivityRowId,
}) {
  final blocks = rows.map(_TimelineDisplayBlock.single).toList();

  if (currentActivityRowId != null) {
    final activityIndex = blocks.indexWhere(
      (block) => block.rows.any((row) => row.id == currentActivityRowId),
    );
    if (activityIndex != -1) {
      blocks.removeAt(activityIndex);
    }
  }

  return blocks;
}

TimelineRow? _currentActivityRow(List<TimelineRow> rows, StudioTurnView? turn) {
  final activity = turn?.state.activity;
  if (turn == null || activity == null) {
    return null;
  }
  for (final row in rows.reversed) {
    if (row.turnId != turn.turnId) {
      continue;
    }
    if (activity == StudioTurnActivity.thinking && row.reasoningGroup != null) {
      return row;
    }
    if (activity.drivesToolGroup && row.toolGroup != null) {
      return row;
    }
  }
  return null;
}

bool _hasNewTimelineEvent(TimelineView oldWidget, TimelineView newWidget) {
  final oldIds = oldWidget.rows.map((row) => row.id).toSet();
  if (newWidget.rows.any((row) => !oldIds.contains(row.id))) {
    return true;
  }
  final previousActivity = _timelineActivityIdentity(
    oldWidget.rows,
    oldWidget.turn,
  );
  final nextActivity = _timelineActivityIdentity(
    newWidget.rows,
    newWidget.turn,
  );
  return nextActivity != null && nextActivity != previousActivity;
}

bool _hasPrependedTimelineRows(
  List<TimelineRow> previous,
  List<TimelineRow> next,
) {
  if (previous.isEmpty || next.length <= previous.length) {
    return false;
  }
  return next.indexWhere((row) => row.id == previous.first.id) > 0;
}

/// 历史窗口驱逐最旧一页：首行被移除且总行数变少。
bool _hasEvictedLeadingTimelineRows(
  List<TimelineRow> previous,
  List<TimelineRow> next,
) {
  if (previous.isEmpty || next.isEmpty || next.length >= previous.length) {
    return false;
  }
  return next.indexWhere((row) => row.id == previous.first.id) < 0;
}

String? _timelineActivityIdentity(
  List<TimelineRow> rows,
  StudioTurnView? turn,
) {
  if (turn?.state.isBusy != true) {
    return null;
  }
  final row = _currentActivityRow(rows, turn);
  if (row != null) {
    return '${row.id}:${row.renderVersion}';
  }
  return '${turn!.turnId}:${turn.state.hashCode}';
}

String _turnActivityId(StudioTurnView turn) {
  return 'turn-activity:${turn.threadId}:${turn.turnId}';
}
