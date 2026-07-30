import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:gpt_markdown/custom_widgets/markdown_config.dart';
import 'package:gpt_markdown/gpt_markdown.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import 'markdown_repair.dart';

part 'timeline_blocks.dart';
part 'timeline_markdown_blocks.dart';
part 'timeline_plan_agent_blocks.dart';
part 'timeline_runtime_progress_blocks.dart';
part 'timeline_tool_blocks.dart';

class TimelineView extends StatefulWidget {
  const TimelineView({
    required this.sessionId,
    required this.rows,
    required this.turn,
    super.key,
  });

  final String? sessionId;
  final List<TimelineRow> rows;
  final StudioTurnView? turn;

  @override
  State<TimelineView> createState() => _TimelineViewState();
}

class _TimelineViewState extends State<TimelineView> {
  static const _bottomThreshold = 80.0;
  static const _scrollDuration = Duration(milliseconds: 180);

  final ScrollController _controller = ScrollController();
  final Map<String, _TimelineScrollSnapshot> _sessionScroll = {};
  final Set<String> _expandedReasoningGroups = {};
  bool _followingBottom = true;
  bool _detachedByUser = false;
  bool _programmaticScroll = false;
  bool _bottomScrollScheduled = false;
  int _pendingNewEvents = 0;
  int _contentVersion = 0;
  _BottomScrollIntent _scheduledBottomIntent = _BottomScrollIntent.jump;
  _TimelineRestore _pendingRestore = const _TimelineRestore.bottom();

  @override
  void initState() {
    super.initState();
    _contentVersion = _timelineContentVersion(widget.rows, widget.turn);
    _restoreSessionState();
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
    final sessionChanged = widget.sessionId != oldWidget.sessionId;
    if (sessionChanged) {
      _saveSessionState(oldWidget.sessionId);
      _expandedReasoningGroups.clear();
      _restoreSessionState();
      _contentVersion = _timelineContentVersion(widget.rows, widget.turn);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) {
          _restorePendingPosition();
        }
      });
      return;
    }

    final nextContentVersion = _timelineContentVersion(
      widget.rows,
      widget.turn,
    );
    if (nextContentVersion == _contentVersion) {
      return;
    }
    final wasNearBottom = _isNearBottom();
    final hasNewEvent = _hasNewTimelineEvent(oldWidget, widget);
    _contentVersion = nextContentVersion;

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
    _saveSessionState(widget.sessionId);
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
    return Stack(
      children: [
        Align(
          alignment: Alignment.topCenter,
          child: ConstrainedBox(
            constraints: const BoxConstraints(
              maxWidth: StudioLayout.conversationWidth,
            ),
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
                    key.value == _turnActivityId(activeTurn)) {
                  return blocks.length;
                }
                final index = blocks.indexWhere(
                  (block) => block.id == key.value,
                );
                return index == -1 ? null : index;
              },
              itemBuilder: (context, index) {
                if (activeTurn != null && index == blocks.length) {
                  return _TurnActivityBlock(
                    key: ValueKey(_turnActivityId(activeTurn)),
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
                return block.isRuntimeProgressGroup
                    ? _TimelineProgressGroupBlock(
                        key: ValueKey(block.id),
                        block: block,
                      )
                    : _TimelineRowBlock(
                        key: ValueKey(block.id),
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
      ],
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
    _saveSessionState(widget.sessionId);
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
    _saveSessionState(widget.sessionId);
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

  void _restoreSessionState() {
    final sessionId = widget.sessionId;
    final snapshot = sessionId == null ? null : _sessionScroll[sessionId];
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
        _saveSessionState(widget.sessionId);
    }
  }

  void _saveSessionState(String? sessionId) {
    if (sessionId == null || !_controller.hasClients) {
      return;
    }
    _sessionScroll[sessionId] = _TimelineScrollSnapshot(
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
    for (final row in rows) ...[row.id, row.role, row.type, row.renderVersion],
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

  factory _TimelineDisplayBlock.runtimeProgress(List<TimelineRow> rows) {
    return _TimelineDisplayBlock._(
      List.unmodifiable(rows),
      id: 'runtime-progress:${rows.first.id}',
    );
  }

  final List<TimelineRow> rows;
  final String id;
  final bool isCurrentActivity;

  bool get isRuntimeProgressGroup => rows.length > 1;

  _TimelineDisplayBlock asCurrentActivity() {
    return _TimelineDisplayBlock._(rows, id: id, isCurrentActivity: true);
  }
}

List<_TimelineDisplayBlock> _timelineDisplayBlocks(
  List<TimelineRow> rows, {
  String? currentActivityRowId,
}) {
  final blocks = <_TimelineDisplayBlock>[];
  final pendingProgress = <TimelineRow>[];

  void flushProgress() {
    if (pendingProgress.isEmpty) {
      return;
    }
    if (pendingProgress.length == 1) {
      blocks.add(_TimelineDisplayBlock.single(pendingProgress.single));
    } else {
      blocks.add(_TimelineDisplayBlock.runtimeProgress(pendingProgress));
    }
    pendingProgress.clear();
  }

  for (final row in rows) {
    if (_isRuntimeProgressRow(row)) {
      final previous = pendingProgress.lastOrNull;
      if (previous != null && !_sameRuntimeProgressGroup(previous, row)) {
        flushProgress();
      }
      pendingProgress.add(row);
      continue;
    }
    flushProgress();
    blocks.add(_TimelineDisplayBlock.single(row));
  }
  flushProgress();

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

bool _isRuntimeProgressRow(TimelineRow row) {
  return row.type == TimelineRowType.commentary && row.part?.synthetic == true;
}

bool _sameRuntimeProgressGroup(TimelineRow left, TimelineRow right) {
  return left.sessionId == right.sessionId &&
      left.messageId == right.messageId &&
      left.turnId == right.turnId;
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
    if ((activity == StudioTurnActivity.runningTool ||
            activity == StudioTurnActivity.waitingForApproval ||
            activity == StudioTurnActivity.waitingForUserInput ||
            activity == StudioTurnActivity.waitingForPlanConfirmation) &&
        row.toolGroup != null) {
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
  return 'turn-activity:${turn.sessionId}:${turn.turnId}';
}
