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
import '../../shared/typed_json.dart';
import 'markdown_repair.dart';

part 'timeline_blocks.dart';
part 'timeline_image_blocks.dart';
part 'timeline_markdown_blocks.dart';
part 'timeline_plan_blocks.dart';
part 'timeline_agent_blocks.dart';
part 'timeline_remote_image_blocks.dart';
part 'timeline_tool_blocks.dart';
part 'timeline_wait_indicator.dart';
part 'timeline_paging.dart';

typedef TimelineRemoteImageProviderFactory = ImageProvider Function(String url);

final timelineRemoteImageProviderFactoryProvider =
    Provider<TimelineRemoteImageProviderFactory>((ref) => NetworkImage.new);

class TimelineView extends StatefulWidget {
  const TimelineView({
    required this.threadId,
    required this.rows,
    required this.turn,
    this.planConfirmation,
    this.planExpanded = false,
    this.onPlanToggle,
    this.onLoadOlder,
    this.isLoadingOlder = false,
    this.isLoadingNewer = false,
    this.onLoadNewer,
    this.onJumpToLatest,
    this.onAnchorChanged,
    this.anchor,
    this.olderError,
    this.newerError,
    this.hasNewer = false,
    this.olderCursor,
    this.newerCursor,
    this.windowEpoch = 0,
    super.key,
  });

  final String? threadId;
  final List<TimelineRow> rows;
  final StudioTurnView? turn;
  final PlanConfirmationView? planConfirmation;
  final bool planExpanded;
  final VoidCallback? onPlanToggle;
  final VoidCallback? onLoadOlder;
  final bool isLoadingOlder;
  final bool isLoadingNewer;
  final VoidCallback? onLoadNewer;
  final VoidCallback? onJumpToLatest;
  final ValueChanged<TimelineAnchor>? onAnchorChanged;
  final TimelineAnchor? anchor;
  final String? olderError;
  final String? newerError;
  final bool hasNewer;
  final String? olderCursor;
  final String? newerCursor;
  final int windowEpoch;

  @override
  State<TimelineView> createState() => _TimelineViewState();
}

class _TimelineViewState extends State<TimelineView> {
  static const _bottomThreshold = 80.0;

  final ScrollController _controller = ScrollController();
  final Map<String, _TimelineScrollSnapshot> _threadScroll = {};
  final Set<String> _expandedReasoningGroups = {};
  final _ThreadImageLoader _imageLoader = _ThreadImageLoader();
  bool _followingBottom = true;
  bool _detachedByUser = false;
  bool _programmaticScroll = false;
  bool _bottomScrollScheduled = false;
  bool _scrollBoundsCorrectionScheduled = false;
  bool _olderLoadRequested = false;
  bool _newerLoadRequested = false;
  Timer? _loadingTimer;
  bool _showLoading = false;
  bool _prefetchScheduled = false;
  bool _anchorPublishScheduled = false;
  final _centerKey = GlobalKey();
  final _viewportKey = GlobalKey();
  final Map<String, GlobalKey> _rowKeys = {};
  final Map<String, ({int version, bool expanded, Widget child})> _rowWidgets =
      {};
  String? _centerId;
  bool _scrollingOlder = true;
  int _pendingNewEvents = 0;
  int _contentVersion = 0;
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
    _contentVersion = _timelineContentVersion(
      widget.rows,
      widget.turn,
      widget.planConfirmation,
    );
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
      _rowWidgets.clear();
      _rowKeys.clear();
      _lastSelectedText = null;
      _imageLoader.clear();
      _restoreThreadState();
      _contentVersion = _timelineContentVersion(
        widget.rows,
        widget.turn,
        widget.planConfirmation,
      );
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) {
          _restorePendingPosition();
        }
      });
      return;
    }
    if ((oldWidget.isLoadingOlder && !widget.isLoadingOlder) ||
        oldWidget.windowEpoch != widget.windowEpoch ||
        (oldWidget.olderCursor ?? oldWidget.rows.firstOrNull?.id) !=
            (widget.olderCursor ?? widget.rows.firstOrNull?.id)) {
      _olderLoadRequested = false;
    }

    if ((oldWidget.isLoadingNewer && !widget.isLoadingNewer) ||
        oldWidget.windowEpoch != widget.windowEpoch ||
        (oldWidget.newerCursor ?? oldWidget.rows.lastOrNull?.id) !=
            (widget.newerCursor ?? widget.rows.lastOrNull?.id)) {
      _newerLoadRequested = false;
    }
    _updateLoadingIndicator();
    _schedulePrefetch();
    final nextContentVersion = _timelineContentVersion(
      widget.rows,
      widget.turn,
      widget.planConfirmation,
    );
    if (nextContentVersion == _contentVersion) return;
    final savedAnchor = widget.anchor;
    final anchor =
        savedAnchor != null &&
            !savedAnchor.followingBottom &&
            _anchorRowId(savedAnchor.itemId) != null &&
            !widget.rows.any((row) => row.id == _centerId)
        ? savedAnchor
        : _captureAnchor();
    final hasNewEvent = _hasNewTimelineEvent(oldWidget, widget);
    _contentVersion = nextContentVersion;
    if (!_detachedByUser && _followingBottom && !widget.hasNewer) {
      _pendingNewEvents = 0;
      _scheduleBottomScroll();
    } else {
      _followingBottom = false;
      _detachedByUser = true;
      if (hasNewEvent || widget.hasNewer) _pendingNewEvents += 1;
      if (anchor != null && _anchorRowId(anchor.itemId) != null) {
        _programmaticScroll = true;
        _controller.position.correctPixels(-anchor.offset);
        _centerId = _anchorRowId(anchor.itemId);
        _pendingRestore = _TimelineRestore.anchor(anchor);
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) _restorePendingPosition();
        });
      }
    }
  }

  @override
  void dispose() {
    _saveThreadState(widget.threadId);
    _controller.removeListener(_handleScrollPositionChanged);
    _controller.dispose();
    _loadingTimer?.cancel();
    _imageLoader.clear();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final activeTurn = widget.turn?.state.isBusy == true ? widget.turn : null;
    final currentActivityRow = _currentActivityRow(widget.rows, activeTurn);
    if (widget.rows.isEmpty &&
        activeTurn == null &&
        widget.planConfirmation == null) {
      return const _EmptyTimeline();
    }
    final blocks = _timelineDisplayBlocks(
      widget.rows,
      currentActivityRowId: currentActivityRow?.id,
    );
    final activity = activeTurn == null
        ? null
        : _TurnActivityBlock(
            key: StudioDriverKeys.turnActivity(_turnActivityId(activeTurn)),
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
    final planSummary = widget.planConfirmation == null
        ? null
        : TimelinePlanSummaryCard(
            plan: widget.planConfirmation!,
            expanded: widget.planExpanded,
            onPressed: widget.onPlanToggle ?? () {},
          );
    final centerIndex = math.max(
      0,
      blocks.indexWhere((block) => block.id == _centerId),
    );
    final activeIds = blocks.map((block) => block.id).toSet();
    _rowKeys.removeWhere((id, _) => !activeIds.contains(id));
    _rowWidgets.removeWhere((id, _) => !activeIds.contains(id));
    return _ThreadImageCacheScope(
      loader: _imageLoader,
      child: SelectionArea(
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
                child: SizedBox(
                  key: _viewportKey,
                  child: NotificationListener<ScrollMetricsNotification>(
                    onNotification: _handleScrollMetricsChanged,
                    child: NotificationListener<ScrollEndNotification>(
                      onNotification: (_) {
                        if (!_programmaticScroll) {
                          WidgetsBinding.instance.addPostFrameCallback((_) {
                            if (mounted && !_programmaticScroll) {
                              _rebaseToVisibleAnchor();
                            }
                          });
                        }
                        return false;
                      },
                      child: CustomScrollView(
                        center: _centerKey,
                        key: StudioDriverKeys.timeline,
                        controller: _controller,
                        slivers: [
                          _itemSliver(
                            blocks.take(centerIndex).toList().reversed.toList(),
                          ),
                          _itemSliver(
                            blocks.skip(centerIndex).toList(),
                            key: _centerKey,
                          ),
                          SliverPadding(
                            padding: const EdgeInsets.symmetric(horizontal: 24),
                            sliver: SliverLayoutBuilder(
                              builder: (context, constraints) {
                                final remaining =
                                    constraints.viewportMainAxisExtent -
                                    constraints.precedingScrollExtent;
                                return SliverToBoxAdapter(
                                  child: ConstrainedBox(
                                    constraints: BoxConstraints(
                                      minHeight: remaining > 0 ? remaining : 0,
                                    ),
                                    child: _TimelineTail(
                                      activity: activity,
                                      planSummary: planSummary,
                                    ),
                                  ),
                                );
                              },
                            ),
                          ),
                        ],
                      ),
                    ),
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
            if (_showLoading && widget.isLoadingOlder ||
                widget.olderError != null)
              _edgeIndicator(older: true),
            if (_showLoading && widget.isLoadingNewer ||
                widget.newerError != null)
              _edgeIndicator(older: false),
          ],
        ),
      ),
    );
  }

  bool get _showJumpToLatest {
    return (widget.rows.isNotEmpty || widget.planConfirmation != null) &&
        (widget.hasNewer ||
            _detachedByUser ||
            _pendingNewEvents > 0 ||
            !_isNearBottom());
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
    switch (_controller.position.userScrollDirection) {
      case ScrollDirection.forward:
        _scrollingOlder = true;
      case ScrollDirection.reverse:
        _scrollingOlder = false;
      case ScrollDirection.idle:
        break;
    }
    final nearBottom = _isNearBottom() && !widget.hasNewer;
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
    _schedulePrefetch();
  }

  bool _handleScrollMetricsChanged(ScrollMetricsNotification notification) {
    _schedulePrefetch();
    if (_programmaticScroll) return false;
    final metrics = notification.metrics;
    if (_followingBottom &&
        !_detachedByUser &&
        !widget.hasNewer &&
        metrics.extentAfter > 0.5) {
      _scheduleBottomScroll();
    }
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

  void _scheduleBottomScroll() {
    if (_bottomScrollScheduled) return;
    _bottomScrollScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _bottomScrollScheduled = false;
      if (mounted && _followingBottom && !_detachedByUser) _scrollToBottom();
    });
  }

  void _scrollToBottom() {
    if (!_controller.hasClients) return;
    _programmaticScroll = true;
    try {
      _controller.jumpTo(_controller.position.maxScrollExtent);
    } finally {
      _programmaticScroll = false;
    }
    setState(() {
      _followingBottom = true;
      _detachedByUser = false;
      _pendingNewEvents = 0;
    });
    _saveThreadState(widget.threadId);
  }

  void _jumpToLatest() {
    _followingBottom = true;
    _detachedByUser = false;
    widget.onJumpToLatest?.call();
    _scheduleBottomScroll();
  }

  void _toggleReasoning(String groupId) {
    setState(() {
      if (!_expandedReasoningGroups.remove(groupId)) {
        _expandedReasoningGroups.add(groupId);
      }
    });
  }

  void _showLoadingIndicator() {
    if (mounted) setState(() => _showLoading = true);
  }

  void _rebaseToVisibleAnchor() {
    if (_followingBottom && !_detachedByUser) return;
    final anchor = _captureAnchor();
    if (anchor == null || _anchorRowId(anchor.itemId) == _centerId) return;
    _programmaticScroll = true;
    _controller.position.correctPixels(-anchor.offset);
    setState(() {
      _centerId = _anchorRowId(anchor.itemId);
      _pendingRestore = _TimelineRestore.anchor(anchor);
    });
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _restorePendingPosition();
    });
  }

  void _restoreThreadState() {
    _programmaticScroll = true;
    final snapshot = _threadScroll[widget.threadId];
    final anchor = widget.anchor ?? snapshot?.anchor;
    _followingBottom = anchor?.followingBottom ?? true;
    _detachedByUser = !_followingBottom;
    _pendingNewEvents = snapshot?.pendingNewEvents ?? 0;
    if (anchor != null && !anchor.followingBottom && _controller.hasClients) {
      _controller.position.correctPixels(-anchor.offset);
    }
    _centerId = anchor == null ? null : _anchorRowId(anchor.itemId);
    _pendingRestore = anchor == null || anchor.followingBottom
        ? const _TimelineRestore.bottom()
        : _TimelineRestore.anchor(anchor);
    _olderLoadRequested = false;
    _newerLoadRequested = false;
    _updateLoadingIndicator();
  }

  void _restorePendingPosition() {
    if (!_controller.hasClients) return;
    final anchor = _pendingRestore.anchor;
    if (anchor == null) {
      _scrollToBottom();
    } else {
      _programmaticScroll = true;
      try {
        _controller.jumpTo(
          (-anchor.offset)
              .clamp(
                _controller.position.minScrollExtent,
                _controller.position.maxScrollExtent,
              )
              .toDouble(),
        );
      } finally {
        _programmaticScroll = false;
      }
    }
    _schedulePrefetch();
  }

  void _saveThreadState(String? threadId) {
    if (threadId == null || !_controller.hasClients) return;
    if (threadId != widget.threadId) return;
    if (_anchorPublishScheduled) return;
    _anchorPublishScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _anchorPublishScheduled = false;
      if (!mounted || widget.threadId != threadId) return;
      final anchor = _captureAnchor();
      if (anchor == null) return;
      _threadScroll[threadId] = _TimelineScrollSnapshot(
        anchor: anchor,
        pendingNewEvents: _pendingNewEvents,
      );
      widget.onAnchorChanged?.call(anchor);
    });
  }
}

class _TimelineRestore {
  const _TimelineRestore.bottom() : anchor = null;
  const _TimelineRestore.anchor(this.anchor);
  final TimelineAnchor? anchor;
}

class _TimelineScrollSnapshot {
  const _TimelineScrollSnapshot({
    required this.anchor,
    required this.pendingNewEvents,
  });
  final TimelineAnchor anchor;
  final int pendingNewEvents;
}

int _timelineContentVersion(
  List<TimelineRow> rows,
  StudioTurnView? turn,
  PlanConfirmationView? plan,
) {
  return Object.hashAll([
    turn,
    plan?.interactionId,
    plan?.markdown,
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
  if (newWidget.planConfirmation?.interactionId != null &&
      newWidget.planConfirmation?.interactionId !=
          oldWidget.planConfirmation?.interactionId) {
    return true;
  }
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
