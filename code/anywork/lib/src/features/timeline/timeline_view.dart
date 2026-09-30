import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import 'package:super_sliver_list/super_sliver_list.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../data/repositories/studio_repository.dart';
import '../../l10n/studio_l10n.dart';
import '../../platform/external_url_launcher.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/studio_driver_state.dart';
import '../../shared/typed_json.dart';
import 'markdown_repair.dart';

part 'timeline_blocks.dart';
part 'timeline_bottom_aligned_sliver.dart';
part 'timeline_image_blocks.dart';
part 'timeline_markdown_blocks.dart';
part 'timeline_plan_blocks.dart';
part 'timeline_agent_blocks.dart';
part 'timeline_remote_image_blocks.dart';
part 'timeline_tool_blocks.dart';
part 'timeline_paging.dart';
part 'timeline_scroll_position.dart';
part 'timeline_layout_changes.dart';

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
    this.onExtendLatest,
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
    this.previewedItemIds = const {},
    this.loadingItemIds = const {},
    this.itemBodyErrors = const {},
    this.pendingItemBodyIds = const {},
    this.unavailableItemIds = const {},
    this.onLoadItemBody,
    this.onVisibleItemBodies,
    super.key,
  });

  final String? threadId;
  final List<TimelineRow> rows;
  final StudioTurnView? turn;
  final PlanConfirmationView? planConfirmation;
  final bool planExpanded;
  final VoidCallback? onPlanToggle;
  final VoidCallback? onLoadOlder;

  /// Canonical capacity to fill a short latest viewport without browsing away from the tail.
  final VoidCallback? onExtendLatest;
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

  /// 页面只以预览返回的超大条目 ID：需要按 identity 回源完整正文。
  final Set<String> previewedItemIds;

  /// 正在回源完整正文的条目 ID。
  final Set<String> loadingItemIds;

  /// 回源失败的条目 ID 与其错误文案。
  final Map<String, String> itemBodyErrors;

  /// 回源已发出但完整正文尚未可取的条目 ID（例如历史事务尚未 durable）。
  ///
  /// 与 [unavailableItemIds] 不同：这是“在途”，入口继续可见可重试。
  final Set<String> pendingItemBodyIds;

  /// 数据源无法提供完整正文的条目 ID。
  final Set<String> unavailableItemIds;

  /// 按 item identity 回源完整正文；为空时对应条目只显示不可用提示。
  final ValueChanged<String>? onLoadItemBody;

  /// 当前视口内的文本条目身份（按行几何判定，进入视口即上报）。
  ///
  /// 智能体正文永不折叠：正文只以数据源预览到达时，外层据此按同一 ChatView 的 identity
  /// 自动补齐完整正文，不需要读者点击“加载完整内容”。
  final ValueChanged<List<String>>? onVisibleItemBodies;

  @override
  State<TimelineView> createState() => _TimelineViewState();
}

class _TimelineViewState extends State<TimelineView> {
  static const _bottomThreshold = 80.0;

  late final ScrollController _controller = _TimelineScrollController(
    () => _followingBottom && !_detachedByUser && !_pointerHeld,
  );
  final ListController _listController = ListController();
  Timer? _streamingIdleTimer;
  bool _deferStreamingUpdates = false;
  final Set<String> _deferredRowIds = {};
  int _restoreAttempts = 0;
  final Map<String, _TimelineScrollSnapshot> _threadScroll = {};
  final Set<String> _expandedReasoningGroups = {};
  final Set<String> _expandedToolGroups = {};
  final _ThreadImageLoader _imageLoader = _ThreadImageLoader();
  TimelineReadingIntent _readingIntent = TimelineReadingIntent.followLatest;
  bool get _followingBottom =>
      _readingIntent == TimelineReadingIntent.followLatest;
  bool get _detachedByUser => !_followingBottom;
  int _readingGeneration = 0;
  TimelineAnchor? _settledAnchor;
  bool _programmaticScroll = false;
  bool _pointerHeld = false;
  bool _keyboardScrolling = false;
  bool _userScrollActive = false;
  double _textScale = 1;
  bool _resumeAfterNewerPage = false;
  bool _bottomScrollScheduled = false;
  bool _olderLoadRequested = false;
  bool _newerLoadRequested = false;
  Timer? _loadingTimer;
  bool _showLoading = false;
  bool _prefetchScheduled = false;
  bool _anchorPublishScheduled = false;

  /// 贴底 sliver 在最近一次布局里上报的前导留白（内容比视口长时为 0）。
  ///
  /// 只用于把视觉位置换算成内容自身偏移（见 [_captureAnchor]）；几何本身由
  /// [_BottomAlignedSliver] 在同一帧算出，因此这里不需要 setState。
  double _bottomSlack = 0;
  double? _viewportWidth;
  bool _geometrySyncScheduled = false;

  /// 上一次上报的可见文本条目签名（身份 + 正文状态），用于去重（见 [_publishVisibleItemBodies]）。
  String? _visibleItemBodySignature;

  /// 只读诊断计数：真实指针拖动产生的滚动更新次数（`ScrollUpdateNotification.dragDetails`
  /// 非空；程序化 jump/animate 与滚轮不会+1）。
  ///
  /// 与 [_publishScrollDiagnostic] 里的 `programmaticScroll` 一起读，用来区分下面两种
  /// 「拖动后位置没变」：
  /// - 计数增长 → 手势确实到达了滚动视图，位置是本状态机（跟随/锚点恢复）改回去的；
  /// - 计数不增长 → 手势/命中测试没有到达滚动视图，问题不在锚点逻辑里。
  int _userDragUpdates = 0;

  final _viewportKey = GlobalKey();
  final Map<String, GlobalKey> _rowKeys = {};
  final Map<
    String,
    ({
      int version,
      bool expanded,
      bool toolExpanded,
      String? body,
      Widget child,
    })
  >
  _rowWidgets = {};

  /// 最近一次拖动方向是否朝更早的内容（`ScrollDirection.forward`）。
  ///
  /// 由 [_handleScrollPositionChanged] 维护，供 [_schedulePrefetch] 决定向前/向后补页。
  bool _scrollingOlder = true;
  int _pendingNewEvents = 0;
  int _contentVersion = 0;
  int _rowStructureVersion = 0;
  _TimelineRestore _pendingRestore = const _TimelineRestore.bottom();

  /// 待恢复的阅读意图（身份 + offset）是否还没被当前布局表达出来。
  ///
  /// 切回会话时正文可能先以**预览**布局出现（`omittedBytes > 0`）：目标内容偏移会被当前
  /// `maxScrollExtent` 钳位成更浅的位置。此时不能把钳位结果当成读者新的阅读位置——否则
  /// 完整正文到位、滚动范围变大后，读者会停在这个更浅的位置上（锚点被覆盖）。保持 `true`
  /// 直到目标能被布局表达；期间不据此判定“跟随最新”，也不重建/发布锚点。
  ///
  /// 意图建立时（[didUpdateWidget] / [_restoreThreadState]）
  /// 一律先置 `true`，再由帧末的 [_refreshRestorePending] 按**每帧定稿后**的滚动范围复核：
  /// 正文补齐是后续帧才把 sliver 撑高的，只在建立那一帧估算一次的话，之后范围变大也不会
  /// 有人再看一眼（标记会停在“已表达”的假象上）。读者主动接管（拖动 / 滚轮 / 跳最新）
  /// 会直接清掉它，因此待表达的意图不会把用户已经接管的位置拉回去。
  bool _restoreClamped = false;

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
      _cancelLayoutRestore();
      _settledAnchor = null;
      _expandedReasoningGroups.clear();
      _expandedToolGroups.clear();
      _rowWidgets.clear();
      _deferredRowIds.clear();
      _rowKeys.clear();
      _visibleItemBodySignature = null;
      _lastSelectedText = null;
      _imageLoader.clear();
      _bottomSlack = 0;
      _streamingIdleTimer?.cancel();
      _streamingIdleTimer = null;
      _deferStreamingUpdates = false;
      _restoreThreadState();
      _contentVersion = _timelineContentVersion(
        widget.rows,
        widget.planConfirmation,
      );
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) {
          _restorePendingPosition();
        }
      });
      return;
    }
    final inspection = _pendingRestore.anchor ?? _settledAnchor;
    if (oldWidget.windowEpoch != widget.windowEpoch) _cancelLayoutRestore();
    if (_readingIntent == TimelineReadingIntent.inspectItem &&
        inspection != null &&
        _anchorRowId(inspection.itemId) == null) {
      _readingIntent = TimelineReadingIntent.followLatest;
      _cancelLayoutRestore();
    }
    _retainExpandedGroups(oldWidget.rows);
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
    if (_detachedByUser &&
        oldWidget.hasNewer &&
        !widget.hasNewer &&
        !_scrollingOlder) {
      _resumeAfterNewerPage = true;
    }
    _updateLoadingIndicator();
    _schedulePrefetch();
    final nextContentVersion = _timelineContentVersion(
      widget.rows,
      widget.planConfirmation,
    );
    if (nextContentVersion == _contentVersion) return;
    // 上一次恢复还没被几何表达（目标偏移被预览布局钳位）时，**意图优先**于当前可见位置：
    // 完整正文到达后要按同一身份 + offset 重新落位，而不是拿钳位后的 `_captureAnchor()`
    // 重建锚点，把读者真实的阅读位置覆盖掉。意图行还没进入窗口时同样保留意图：
    final restoreIntent = _restoreClamped ? _pendingRestore.anchor : null;
    // 后端 focus 只决定取窗范围，不是读者刚刚看到的位置。分页换掉分组
    // 首条时不能退回旧 focus 的 offset；切换会话的恢复意图已由上面的
    // _pendingRestore 单独保留，其余变化始终捕获更新前实际可见的身份。
    final anchor = restoreIntent ?? _captureAnchor(rows: oldWidget.rows);
    final hasNewEvent = _hasNewTimelineEvent(oldWidget, widget);
    _contentVersion = nextContentVersion;
    final structureChanged =
        oldWidget.rows.length != widget.rows.length ||
        oldWidget.rows.indexed.any(
          (entry) => entry.$2.id != widget.rows[entry.$1].id,
        );
    if (structureChanged) {
      // SuperSliverList stores heights by index. Dirtying an index does not
      // replace its previous height, so a new window needs a fresh extent map.
      // Stable row GlobalKeys retain mounted content; the existing identity
      // anchor restores the reader after the new list has laid out.
      _rowStructureVersion++;
    } else if (_listController.isAttached) {
      for (var index = 0; index < oldWidget.rows.length; index++) {
        if (index >= _listController.numberOfItems) break;
        if (oldWidget.rows[index].renderVersion !=
            widget.rows[index].renderVersion) {
          _listController.invalidateExtent(index);
        }
      }
    }

    // 贴在窗口末尾时，内容变化（追加 / 历史分页 / 窗口替换）都保持跟随：`hasNewer`
    // 只表示窗口之外还有更新条目（由「跳到最新」入口表达），不代表读者离开了末尾。
    // 之前这里要求 `!hasNewer`，于是历史分页会把跟随态写成非跟随锚点，重开时又围绕
    // 这条锚点重新取窗。
    if (!_detachedByUser && _followingBottom) {
      _pendingNewEvents = 0;
      // ScrollPosition applies the new bottom during layout.
    } else {
      if (hasNewEvent || widget.hasNewer) _pendingNewEvents += 1;
      if (anchor != null &&
          _anchorRowId(anchor.itemId) != null &&
          (_restoreClamped ||
              structureChanged ||
              _needsAnchorRebase(anchor, oldWidget.rows))) {
        _prepareAnchorRestore(anchor);
      }
    }
  }

  @override
  void dispose() {
    _saveThreadState(widget.threadId);
    // 诊断投影随视图一起失效，避免驱动快照残留已销毁视图的几何。
    StudioDriverState.publishTimelineScroll(null);
    _controller.removeListener(_handleScrollPositionChanged);
    _controller.dispose();
    _listController.dispose();
    _streamingIdleTimer?.cancel();
    _loadingTimer?.cancel();
    _imageLoader.clear();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final textScale = MediaQuery.textScalerOf(context).scale(14) / 14;
    if (textScale != _textScale) {
      if (_detachedByUser && !_restoreClamped) {
        final anchor = _captureAnchor();
        if (anchor != null) _prepareAnchorRestore(anchor);
      }
      _textScale = textScale;
      _rowStructureVersion++;
    }
    _schedulePrefetch();
    final activeTurn = widget.turn?.state.isBusy == true ? widget.turn : null;
    if (widget.rows.isEmpty &&
        activeTurn == null &&
        widget.planConfirmation == null &&
        widget.onLoadOlder == null &&
        widget.onLoadNewer == null &&
        widget.olderError == null &&
        widget.newerError == null) {
      return const _EmptyTimeline();
    }
    // 本帧结束后按实际几何做一次归一 + 诊断发布。
    _scheduleGeometrySync();
    final rows = widget.rows;
    final planSummary = widget.planConfirmation == null
        ? null
        : _layoutItem(
            'plan:${widget.planConfirmation!.interactionId}',
            TimelinePlanSummaryCard(
              plan: widget.planConfirmation!,
              expanded: widget.planExpanded,
              onPressed: widget.onPlanToggle ?? () {},
            ),
          );
    final activeIds = rows.map((row) => row.id).toSet();
    _rowKeys.removeWhere((id, _) => !activeIds.contains(id));
    _rowWidgets.removeWhere((id, _) => !activeIds.contains(id));
    _deferredRowIds.retainAll(activeIds);
    _imageLoader.retainWindow(widget.threadId, rows);
    // 展开态按稳定分组身份保存，但只保留仍在窗口内的身份：历史分页淘汰/切回
    // 不留下再也用不到的 id，避免集合无界增长。
    _expandedReasoningGroups.retainAll({
      for (final row in rows)
        if (row.reasoningGroup case final group?) group.id,
    });
    _expandedToolGroups.retainAll({
      for (final row in rows)
        if (row.toolGroup case final group?) group.id,
    });
    return PrimaryScrollController(
      controller: _controller,
      child: Actions(
        actions: <Type, Action<Intent>>{
          ScrollIntent: _TimelineScrollAction(
            _controller,
            _handleKeyboardScroll,
          ),
        },
        child: _ThreadImageCacheScope(
          loader: _imageLoader,
          child: SelectionArea(
            onSelectionChanged: _handleTimelineSelectionChanged,
            contextMenuBuilder: _buildTimelineContextMenu,
            child: Stack(
              children: [
                // "跳到最新"占用滚动区之外的独立横条，而不是浮在消息列上：用户气泡右对齐，
                // 恰好落在底部角落，悬浮覆盖会挡住正文末尾。无提示时该横条不占高度。
                Positioned.fill(
                  child: Align(
                    alignment: Alignment.topCenter,
                    child: ConstrainedBox(
                      constraints: const BoxConstraints(
                        maxWidth: StudioLayout.conversationWidth,
                      ),
                      child: Column(
                        children: [
                          Expanded(child: _buildViewport(rows, planSummary)),
                          Visibility(
                            visible: _showJumpToLatest,
                            maintainSize: true,
                            maintainAnimation: true,
                            maintainState: true,
                            child: Padding(
                              padding: const EdgeInsets.fromLTRB(24, 0, 24, 12),
                              child: Align(
                                alignment: Alignment.centerRight,
                                child: _JumpToLatestButton(
                                  pendingCount: _pendingNewEvents,
                                  onPressed: _jumpToLatest,
                                ),
                              ),
                            ),
                          ),
                        ],
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
        ),
      ),
    );
  }

  Widget _buildViewport(List<TimelineRow> rows, Widget? planSummary) {
    return LayoutBuilder(
      builder: (context, constraints) {
        if (_viewportWidth != null &&
            _viewportWidth != constraints.maxWidth &&
            _detachedByUser &&
            !_restoreClamped) {
          final anchor = _captureAnchor();
          if (anchor != null) _prepareAnchorRestore(anchor);
        }
        _viewportWidth = constraints.maxWidth;
        return SizedBox(
          key: _viewportKey,
          child: Listener(
            onPointerSignal: _handlePointerSignal,
            onPointerDown: (_) {
              // 按下即暂停跟随，给滚动条与内容手势相同的优先级。
              _pointerHeld = true;
              _cancelLayoutRestore();
              _deferStreamingDuringScroll();
            },
            onPointerUp: (_) => _releasePointer(),
            onPointerCancel: (_) => _releasePointer(),
            child: NotificationListener<ScrollMetricsNotification>(
              onNotification: _handleScrollMetricsChanged,
              child: NotificationListener<ScrollUpdateNotification>(
                onNotification: _handleScrollUpdate,
                child: NotificationListener<ScrollNotification>(
                  onNotification: (notification) {
                    if (notification is UserScrollNotification) {
                      return _handleUserScroll(notification);
                    }
                    if (notification is ScrollEndNotification &&
                        notification.depth == 0 &&
                        !_programmaticScroll) {
                      _keyboardScrolling = false;
                      _userScrollActive = false;
                      _resumeStreamingAfterIdle();
                      _saveThreadState(widget.threadId);
                    }
                    return false;
                  },
                  child: Scrollbar(
                    controller: _controller,
                    thumbVisibility: true,
                    child: CustomScrollView(
                      scrollBehavior: ScrollConfiguration.of(context)
                          .copyWith(scrollbars: false),
                      physics: const AlwaysScrollableScrollPhysics(),
                      key: StudioDriverKeys.timeline,
                      controller: _controller,
                      scrollCacheExtent: const ScrollCacheExtent.pixels(600),
                      slivers: [
                        _BottomAlignedSliver(
                          onSlackChanged: _handleBottomSlackChanged,
                          child: SliverPadding(
                            padding: const EdgeInsets.symmetric(horizontal: 24),
                            sliver: SuperSliverList(
                              key: ValueKey((
                                widget.threadId,
                                _rowStructureVersion,
                              )),
                              listController: _listController,
                              extentEstimation: _estimateRowExtent,
                              delayPopulatingCacheArea: false,
                              delegate: SliverChildBuilderDelegate(
                                (context, index) => index == rows.length
                                    ? _TimelineTail(planSummary: planSummary)
                                    : _buildRow(rows[index]),
                                childCount: rows.length + 1,
                                addAutomaticKeepAlives: false,
                                findChildIndexCallback: (key) {
                                  if (key is! ValueKey<String>) return null;
                                  final index = rows.indexWhere(
                                    (row) => row.id == key.value,
                                  );
                                  return index < 0 ? null : index;
                                },
                              ),
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
        );
      },
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

  /// 用户是否正在主动把内容拉离末尾。
  ///
  /// 判据是滚动方向而不是单帧 `extentAfter`：Driver / 触摸拖动会被拆成每帧十几像素的
  /// 小步，单帧的 `extentAfter` 在累积离开 [_bottomThreshold] 之前一直落在阈值内。若
  /// 「近底即跟随」不看方向，就会在拖动还没真正离开阈值时把位置每帧拉回末尾——表现为
  /// `pixels` 恒定、`detachedByUser` 恒为 false，历史永远翻不动。
  ///
  /// `ScrollDirection.forward` = 手指下拖、内容向更早方向移动（位置朝 `minScrollExtent`）。
  /// 内容不足一屏但还有旧页时，上翻意图仍然有效；真正没有旧页的短会话才保持贴底。
  bool get _userPullingAwayFromBottom {
    if (!_controller.hasClients) return false;
    final position = _controller.position;
    if (position.userScrollDirection != ScrollDirection.forward) return false;
    return widget.onLoadOlder != null ||
        position.pixels > position.minScrollExtent + 0.5;
  }

  void _handlePointerSignal(PointerSignalEvent event) {
    if (event is! PointerScrollEvent ||
        event.scrollDelta.dy == 0 ||
        !_controller.hasClients) {
      return;
    }
    // Match Scrollable's axis modifiers: Shift+wheel may belong to a horizontal
    // code/tool scroller and must not change the timeline's reading intent.
    if (event.kind == PointerDeviceKind.mouse &&
        ScrollConfiguration.of(context).pointerAxisModifiers
            .any(HardwareKeyboard.instance.logicalKeysPressed.contains)) {
      return;
    }
    final older = event.scrollDelta.dy < 0;
    final canPage = (older ? widget.onLoadOlder : widget.onLoadNewer) != null;
    final canResumeLatest =
        !older &&
        !widget.hasNewer &&
        (!_followingBottom || _detachedByUser || _restoreClamped);
    if (!canPage && !canResumeLatest) return;
    // Scrollable only claims wheel events that move pixels. At a window edge
    // (including an underfull list), the unclaimed direction still means page
    // onward or resume Latest. Inner scrollers and normal timeline movement
    // register first, so they retain ownership of any event they can consume.
    GestureBinding.instance.pointerSignalResolver.register(event, (_) {
      if (!mounted) return;
      _cancelLayoutRestore();
      _handleScrollPositionChanged(
        direction: older ? ScrollDirection.forward : ScrollDirection.reverse,
      );
      event.respond(allowPlatformDefault: false);
    });
  }

  void _handleKeyboardScroll(ScrollDirection direction) {
    _deferStreamingDuringScroll();
    _resumeStreamingAfterIdle();
    _keyboardScrolling = true;
    _cancelLayoutRestore();
    _programmaticScroll = false;
    _handleScrollPositionChanged(direction: direction);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_controller.hasClients) return;
      if (!_controller.position.isScrollingNotifier.value) {
        _keyboardScrolling = false;
      }
    });
  }

  void _releasePointer() {
    _pointerHeld = false;
    _resumeStreamingAfterIdle();
    _scheduleGeometrySync();
    _schedulePrefetch();
    if (_followingBottom && !_detachedByUser) _scheduleBottomScroll();
  }

  bool _handleUserScroll(UserScrollNotification notification) {
    if (notification.depth == 0) {
      _userScrollActive = notification.direction != ScrollDirection.idle;
      if (!_userScrollActive) _resumeStreamingAfterIdle();
    }
    if (notification.depth == 0 &&
        notification.direction != ScrollDirection.idle) {
      _deferStreamingDuringScroll();
      _resumeStreamingAfterIdle();
      // 不足一屏时 pixels 不会变化，但用户仍能选择向旧/向新补页。
      _cancelLayoutRestore();
      _handleScrollPositionChanged(direction: notification.direction);
    }
    return false;
  }

  /// 只读诊断：记录一次由真实指针拖动产生的滚动更新（见 [_userDragUpdates]）。
  ///
  /// 同时识别键盘/滚动条引起的位移；内层滚动不改变外层阅读意图。
  bool _handleScrollUpdate(ScrollUpdateNotification notification) {
    if (notification.depth != 0) return false;
    if (!_programmaticScroll) _resumeStreamingAfterIdle();
    if (notification.dragDetails != null) {
      _userDragUpdates += 1;
      // 读者主动接管位置：放弃尚未被布局表达的恢复意图（不再回落到原锚点）。
      _cancelLayoutRestore();
    } else if (!_programmaticScroll &&
        _controller.hasClients &&
        _controller.position.userScrollDirection != ScrollDirection.idle) {
      // 滚轮 / 触控板等非拖动滚动同样是读者接管。
      _cancelLayoutRestore();
    }
    final delta = notification.scrollDelta;
    if (!_programmaticScroll &&
        (_pointerHeld || _keyboardScrolling) &&
        delta != null &&
        delta != 0 &&
        _controller.hasClients &&
        _controller.position.userScrollDirection == ScrollDirection.idle) {
      // 键盘和滚动条也能移动位置，但不一定设置 userScrollDirection。
      // 只在真实指针/键盘操作期间采纳，排除布局修正触发的空闲位移。
      _cancelLayoutRestore();
      _handleScrollPositionChanged(
        direction: delta < 0
            ? ScrollDirection.forward
            : ScrollDirection.reverse,
      );
    }
    return false;
  }

  void _handleScrollPositionChanged({ScrollDirection? direction}) {
    // 位置变化（含程序化 jump）后按帧做几何同步并发布只读诊断。
    _scheduleGeometrySync();
    if (!_controller.hasClients || _programmaticScroll) {
      return;
    }
    direction ??= _controller.position.userScrollDirection;
    // 拖动方向既是"是否正在上翻历史"的判据，也决定向前/向后补页（见 [_schedulePrefetch]）：
    // `forward` = 手指下拖、朝更早内容；`reverse` = 朝更新的内容。`idle` 保持上一次方向。
    switch (direction) {
      case ScrollDirection.forward:
        _scrollingOlder = true;
        _resumeAfterNewerPage = false;
      case ScrollDirection.reverse:
        _scrollingOlder = false;
      case ScrollDirection.idle:
        break;
    }
    // 读者正在主动上翻历史时，即使本帧仍在「近底」阈值内，也不能当成想跟随最新：
    // 拖动分步到达，单帧距离可能永远不超过阈值（见 [_userPullingAwayFromBottom]）。
    final pullingAway = _userPullingAwayFromBottom;
    // 待恢复意图还没被布局表达时，当前位置只是预览布局下的钳位结果，不构成“读者在末尾”
    // 的事实：据此恢复跟随会直接吞掉待恢复的锚点。
    final nearBottom = _isNearBottom() && !widget.hasNewer && !_restoreClamped;
    if (nearBottom && direction == ScrollDirection.reverse && !pullingAway) {
      if (!_followingBottom || _detachedByUser || _pendingNewEvents != 0) {
        if (_pointerHeld) {
          _resumeAfterNewerPage = true;
          _saveThreadState(widget.threadId);
          return;
        }
        _followLatestBottom();
        // 用户滚回最新与点击“跳到最新”使用同一命令，恢复 ChatView 的 Latest
        // 聚焦；只改 UI 标志会让后续新消息继续留在历史窗口之外。
        widget.onJumpToLatest?.call();
      }
    } else if (direction != ScrollDirection.idle &&
        _readingIntent != TimelineReadingIntent.browseHistory) {
      setState(() {
        _readingIntent = TimelineReadingIntent.browseHistory;
      });
    }
    _saveThreadState(widget.threadId);
    _schedulePrefetch();
  }

  bool _handleScrollMetricsChanged(ScrollMetricsNotification notification) {
    if (notification.depth != 0) return false;
    // 布局改变滚动范围后按帧做几何同步并发布只读诊断。
    _scheduleGeometrySync();
    _schedulePrefetch();
    return false;
  }

  void _scheduleBottomScroll() {
    if (_bottomScrollScheduled) return;
    _bottomScrollScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _bottomScrollScheduled = false;
      if (mounted && !_pointerHeld && _followingBottom && !_detachedByUser) {
        _scrollToBottom();
      }
    });
  }

  void _scrollToBottom() {
    if (!_controller.hasClients || _pointerHeld) return;
    final position = _controller.position;
    if ((position.pixels - position.maxScrollExtent).abs() > 0.5) {
      _programmaticScroll = true;
      try {
        _controller.jumpTo(position.maxScrollExtent);
      } finally {
        _programmaticScroll = false;
      }
    }
    if (!_followingBottom || _detachedByUser || _pendingNewEvents != 0) {
      setState(() {
        _readingIntent = TimelineReadingIntent.followLatest;
        _pendingNewEvents = 0;
      });
      _saveThreadState(widget.threadId);
    }
  }

  void _jumpToLatest() {
    // 阅读方向与最新窗口填充互不影响；扩充由布局与 canonical 容量决定。
    _scrollingOlder = false;
    _followLatestBottom();
    widget.onJumpToLatest?.call();
    _scheduleBottomScroll();
  }

  void _beginItemInspection() {
    setState(() => _readingIntent = TimelineReadingIntent.inspectItem);
  }

  void _followLatestBottom() {
    setState(() {
      _readingIntent = TimelineReadingIntent.followLatest;
      _pendingNewEvents = 0;
      _cancelLayoutRestore();
      _resumeAfterNewerPage = false;
      _deferStreamingUpdates = false;
    });
    _scheduleBottomScroll();
    _saveThreadState(widget.threadId);
  }

  void _deferStreamingDuringScroll() {
    _streamingIdleTimer?.cancel();
    _streamingIdleTimer = null;
    if (_detachedByUser || !_isNearBottom()) _deferStreamingUpdates = true;
  }

  void _resumeStreamingAfterIdle() {
    _streamingIdleTimer?.cancel();
    late final Timer timer;
    timer = Timer(const Duration(milliseconds: 160), () {
      if (!mounted || !_deferStreamingUpdates || _pointerHeld) return;
      // Static history has nothing to flush. Creating a restore intent here
      // would compete with the wheel's position even though no body changed.
      if (_deferredRowIds.isEmpty) {
        _deferStreamingUpdates = false;
        return;
      }
      // A timer can fire between pointerScroll and the next layout. Capture the
      // painted anchor only after that layout, otherwise it describes the old
      // viewport and restores the very position the user just scrolled away from.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted ||
            !identical(_streamingIdleTimer, timer) ||
            !_deferStreamingUpdates ||
            _pointerHeld) {
          return;
        }
        final anchor = _captureAnchor();
        setState(() {
          _deferStreamingUpdates = false;
          if (_detachedByUser && anchor != null) _prepareAnchorRestore(anchor);
        });
      });
      WidgetsBinding.instance.ensureVisualUpdate();
    });
    _streamingIdleTimer = timer;
  }

  void _toggleReasoning(String groupId) {
    setState(() {
      if (!_expandedReasoningGroups.remove(groupId)) {
        _expandedReasoningGroups.add(groupId);
      }
    });
  }

  /// 分页或聚焦会改变分组首条身份；仍有共同条目的组保留展开意图。
  /// 只迁移当前窗口的身份，不保留被淘汰的正文或远端分组状态。
  void _retainExpandedGroups(List<TimelineRow> previousRows) {
    final reasoningItems = <String>{};
    final toolItems = <String>{};
    for (final row in previousRows) {
      final reasoning = row.reasoningGroup;
      if (reasoning != null &&
          _expandedReasoningGroups.contains(reasoning.id)) {
        reasoningItems.addAll(reasoning.parts.map((part) => part.id));
      }
      final tools = row.toolGroup;
      if (tools != null && _expandedToolGroups.contains(tools.id)) {
        toolItems.addAll(tools.items.map((item) => item.id));
      }
    }
    for (final row in widget.rows) {
      final reasoning = row.reasoningGroup;
      if (reasoning != null &&
          reasoning.parts.any((part) => reasoningItems.contains(part.id))) {
        _expandedReasoningGroups.add(reasoning.id);
      }
      final tools = row.toolGroup;
      if (tools != null &&
          tools.items.any((item) => toolItems.contains(item.id))) {
        _expandedToolGroups.add(tools.id);
      }
    }
  }

  void _toggleToolGroup(String groupId) {
    setState(() {
      if (!_expandedToolGroups.remove(groupId)) {
        _expandedToolGroups.add(groupId);
      }
    });
  }

  /// 记录 [_BottomAlignedSliver] 在**布局期**算出的贴底留白。
  ///
  /// 留白已经参与本帧几何，这里只是记下来供锚点换算使用，因此不 setState、
  /// 不安排下一帧，也不会造成「先顶部再跳底」。
  void _handleBottomSlackChanged(double slack) {
    final clamped = slack.isFinite && slack > 0 ? slack : 0.0;
    if (clamped == _bottomSlack) return;
    _bottomSlack = clamped;
  }

  /// 帧末做一次几何同步并发布只读滚动诊断（每帧最多一次）。
  ///
  /// 放在帧末是因为两者都依赖本帧最终布局出来的滚动几何：`build` 期间
  /// [ScrollPosition] 的 min/max/pixels 还是上一帧的值。
  void _scheduleGeometrySync() {
    if (_geometrySyncScheduled) return;
    _geometrySyncScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _geometrySyncScheduled = false;
      if (!mounted) return;
      _syncTimelineGeometry();
    });
  }

  void _syncTimelineGeometry() {
    _restoreVisibleAnchor();
    // 索引定位和真实几何校正后确认恢复完成，再发布位置。
    if (_refreshRestorePending()) _saveThreadState(widget.threadId);
    if (_resumeAfterNewerPage &&
        !_programmaticScroll &&
        !_restoreClamped &&
        !_pointerHeld &&
        _controller.hasClients) {
      _resumeAfterNewerPage = false;
      // 用户向新翻页后真正抵达最新端，分页响应代替最后一次滚动事件完成跟随。
      // 只响应 hasNewer 从 true 到 false，不因普通内容增长或窗口缩放抢占阅读。
      if (_detachedByUser &&
          !_scrollingOlder &&
          !widget.hasNewer &&
          _isNearBottom()) {
        _followLatestBottom();
        widget.onJumpToLatest?.call();
      }
    }
    _publishVisibleItemBodies();
    _publishScrollDiagnostic();
    if (!_restoreClamped) _settledAnchor = _captureAnchor();
  }

  /// 上报当前视口内的文本条目身份；身份与正文状态都没变化时不上报，避免无谓的重建。
  ///
  /// 只上报文本行（用户 / parentAgent / 智能体正文）：它们的正文由窗口按身份自动补齐。
  /// 折叠的推理与工具分组不在此列，读者展开时的入口仍由分组行自己给出。
  ///
  /// 签名里同时带上该身份的正文状态与补齐标记，因此**新交付的窗口预览**（含会话重开后的
  /// 首窗）、**身份进入预览**与**补齐完成**都会重新上报一次，不需要读者滚动去触发补齐；
  /// 上报只在状态真正变化时发生，因此稳定的流式帧不会重复上报、不会自激。
  void _publishVisibleItemBodies() {
    final onVisible = widget.onVisibleItemBodies;
    if (onVisible == null) return;
    final viewport = _viewportKey.currentContext?.findRenderObject();
    if (viewport is! RenderBox || !viewport.hasSize) return;
    final ids = <String>[];
    final signature = StringBuffer();
    for (final row in widget.rows) {
      if (row.type != TimelineRowType.userMessage &&
          row.type != TimelineRowType.parentAgentMessage &&
          row.type != TimelineRowType.commentary &&
          row.type != TimelineRowType.finalAnswer) {
        continue;
      }
      final part = row.part;
      if (part == null) continue;
      final box = _rowKeys[row.id]?.currentContext?.findRenderObject();
      if (box is! RenderBox || !box.hasSize || !box.attached) continue;
      final top = box.localToGlobal(Offset.zero, ancestor: viewport).dy;
      if (top + box.size.height <= 0 || top >= viewport.size.height) continue;
      ids.add(part.id);
      signature
        ..write(part.id)
        // 补齐按身份发起：进入/离开预览、在途、待落盘都会改变签名而重新上报一次，
        // 因此运行中的流式正文补齐后不会因内容继续增长而反复重发同一请求。
        ..write(part.status)
        ..write(widget.previewedItemIds.contains(part.id) ? '1' : '0')
        ..write(widget.loadingItemIds.contains(part.id) ? '1' : '0')
        ..write(widget.pendingItemBodyIds.contains(part.id) ? '1' : '0')
        ..write('|');
    }
    final next = signature.toString();
    if (next == _visibleItemBodySignature) return;
    _visibleItemBodySignature = next;
    if (ids.isNotEmpty) onVisible(ids);
  }

  /// Driver reports both window size and actually mounted lazy rows.
  void _publishScrollDiagnostic() {
    final position = _controller.hasClients ? _controller.position : null;
    StudioDriverState.publishTimelineScroll(
      TimelineScrollDiagnostic(
        threadId: widget.threadId,
        mountedRowCount: _rowKeys.values
            .where((key) => key.currentContext != null)
            .length,
        rowCount: widget.rows.length,
        readingIntent: _readingIntent,
        detachedByUser: _detachedByUser,
        pendingNewEvents: _pendingNewEvents,
        pixels: position?.pixels,
        minScrollExtent: position?.minScrollExtent,
        maxScrollExtent: position?.maxScrollExtent,
        viewportDimension: position?.viewportDimension,
        extentAfter: position?.extentAfter,
        bottomSlack: _bottomSlack,
        hasNewer: widget.hasNewer,
        showJumpToLatest: _showJumpToLatest,
        programmaticScroll: _programmaticScroll,
        userDragUpdates: _userDragUpdates,
        anchor: _captureAnchor(),
        restorePending: _restoreClamped,
        restoreAnchor: _pendingRestore.anchor,
      ),
    );
  }

  void _showLoadingIndicator() {
    if (mounted) setState(() => _showLoading = true);
  }

  void _prepareAnchorRestore(TimelineAnchor anchor) {
    if (_pointerHeld || _keyboardScrolling || _userScrollActive) return;
    _pendingRestore = _TimelineRestore.anchor(
      anchor,
      target: _validLayoutTarget,
    );
    _restoreClamped = true;
    _restoreAttempts = 0;
    _scheduleGeometrySync();
  }

  void _restoreThreadState() {
    _programmaticScroll = false;
    _pointerHeld = false;
    _keyboardScrolling = false;
    _userScrollActive = false;
    _resumeAfterNewerPage = false;
    _scrollingOlder = true;
    final snapshot = _threadScroll[widget.threadId];
    final anchor = widget.anchor ?? snapshot?.anchor;
    _readingIntent =
        anchor?.readingIntent ?? TimelineReadingIntent.followLatest;
    final detached = !_followingBottom;
    _pendingNewEvents = detached ? snapshot?.pendingNewEvents ?? 0 : 0;
    _pendingRestore = detached
        ? _TimelineRestore.anchor(anchor)
        : const _TimelineRestore.bottom();
    _restoreClamped = detached;
    _restoreAttempts = 0;
    _olderLoadRequested = false;
    _newerLoadRequested = false;
    _updateLoadingIndicator();
  }

  void _restorePendingPosition() {
    _scheduleGeometrySync();
    _schedulePrefetch();
  }

  /// Locate an unbuilt row by index, then correct against its painted position.
  /// A bounded number of frames settles estimates. A still-short preview retains
  /// the intent until its canonical body arrives; it never spins a frame loop.
  void _restoreVisibleAnchor() {
    if (!_restoreClamped ||
        _pointerHeld ||
        _userScrollActive ||
        _keyboardScrolling ||
        !_controller.hasClients ||
        !_listController.isAttached ||
        _restoreAttempts >= 3) {
      return;
    }
    final anchor = _pendingRestore.anchor;
    final layoutTarget = _validLayoutTarget;
    if (anchor == null && layoutTarget == null) return;
    final rowId = anchor == null ? null : _anchorRowId(anchor.itemId);
    final index = widget.rows.indexWhere((row) => row.id == rowId);
    if (index < 0 && layoutTarget == null) return;
    _restoreAttempts++;
    _programmaticScroll = true;
    try {
      final target =
          (layoutTarget == null ? null : _layoutTargetPixels(layoutTarget)) ??
          (anchor == null ? null : _restoreTargetPixels(anchor));
      if (target == null) {
        if (index < 0) return;
        _listController.jumpToItem(
          index: index,
          scrollController: _controller,
          alignment: 0,
        );
      } else {
        final position = _controller.position;
        final clamped = target
            .clamp(position.minScrollExtent, position.maxScrollExtent)
            .toDouble();
        if ((position.pixels - clamped).abs() > 0.5) {
          _controller.jumpTo(clamped);
        }
      }
    } finally {
      _programmaticScroll = false;
    }
    _scheduleGeometrySync();
    WidgetsBinding.instance.ensureVisualUpdate();
  }

  double? _restoreTargetPixels(TimelineAnchor anchor) {
    final rowId = _anchorRowId(anchor.itemId);
    final box = _rowKeys[rowId]?.currentContext?.findRenderObject();
    final viewport = _viewportKey.currentContext?.findRenderObject();
    if (box is! RenderBox ||
        !box.hasSize ||
        !box.attached ||
        viewport is! RenderBox ||
        !_controller.hasClients) {
      return null;
    }
    final top =
        box.localToGlobal(Offset.zero, ancestor: viewport).dy - _bottomSlack;
    return _controller.position.pixels + top - anchor.offset;
  }

  bool _refreshRestorePending() {
    if (!_restoreClamped || _pointerHeld) return false;
    final anchor = _pendingRestore.anchor;
    if (!_controller.hasClients) return false;
    final layoutTarget = _validLayoutTarget;
    final rawTarget =
        (layoutTarget == null ? null : _layoutTargetPixels(layoutTarget)) ??
        (anchor == null ? null : _restoreTargetPixels(anchor));
    final target = layoutTarget != null
        ? rawTarget
              ?.clamp(
                _controller.position.minScrollExtent,
                _controller.position.maxScrollExtent,
              )
              .toDouble()
        : rawTarget;
    if (target == null || (_controller.position.pixels - target).abs() > 0.5) {
      return false;
    }
    _restoreClamped = false;
    return true;
  }

  void _saveThreadState(String? threadId) {
    if (threadId == null || !_controller.hasClients) return;
    if (threadId != widget.threadId) return;
    if (_anchorPublishScheduled) return;
    // 恢复意图还没被几何表达出来时，当前可见位置只是钳位结果，不能把它当成读者的阅读
    // 位置发布出去覆盖原锚点（原锚点要留给完整正文到位后的重新落位）。
    if (_restoreClamped) return;
    _anchorPublishScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _anchorPublishScheduled = false;
      if (!mounted || widget.threadId != threadId || _restoreClamped) return;
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

/// Retains Flutter's keyboard scrolling and observes only the outer timeline's
/// actions. Keyboard navigation inside a nested tool-output scroller stays local.
class _TimelineScrollAction extends ScrollAction {
  _TimelineScrollAction(this.controller, this.onScroll);

  final ScrollController controller;
  final ValueChanged<ScrollDirection> onScroll;

  @override
  void invoke(ScrollIntent intent, [BuildContext? context]) {
    final nested = context == null ? null : Scrollable.maybeOf(context);
    if (controller.hasClients &&
        (nested == null || nested.position == controller.position) &&
        axisDirectionToAxis(intent.direction) == Axis.vertical) {
      onScroll(
        intent.direction == AxisDirection.up
            ? ScrollDirection.forward
            : ScrollDirection.reverse,
      );
    }
    super.invoke(intent, context);
  }
}

class _TimelineRestore {
  const _TimelineRestore.bottom() : anchor = null, target = null;
  const _TimelineRestore.anchor(this.anchor, {this.target});
  final TimelineAnchor? anchor;
  final _TimelineLayoutTarget? target;
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
  PlanConfirmationView? plan,
) {
  return Object.hashAll([
    // 当前活动只在固定活动条里渲染，Turn 阶段帧不再影响 Timeline 布局；
    // 因此这里只跟踪消息行与计划，阶段变化不会重排/抢走阅读锚点。
    plan?.interactionId,
    plan?.markdown,
    rows.length,
    for (final row in rows) ...[row.id, row.type, row.renderVersion],
  ]);
}

/// 一条条目在窗口中的“完整正文”状态：预览 / 正在回源 / 回源失败。
///
/// 身份、顺序与 ordinal 由条目自身携带；这里只表达载荷是否需要回源，因此不会改变阅读位置。
class _ItemBodyState {
  const _ItemBodyState({
    required this.itemId,
    required this.isPreviewed,
    required this.isLoading,
    required this.isPending,
    required this.isUnavailable,
    this.label,
    this.error,
    this.onLoad,
  });

  /// canonical item 身份；回源与去重都以它为准，分组行的合成行身份不参与。
  final String itemId;

  /// 可选的数据来源标签（例如工具名），用于在同一分组里区分多条回源入口。
  final String? label;
  final bool isPreviewed;
  final bool isLoading;

  /// 回源已发出但完整正文尚未可取（例如历史事务尚未 durable）：入口仍可见可重试。
  final bool isPending;

  /// 数据源无法提供完整正文；此时不提供重试入口（重试也不会取到完整内容）。
  final bool isUnavailable;
  final String? error;
  final VoidCallback? onLoad;
}

bool _hasNewTimelineEvent(TimelineView oldWidget, TimelineView newWidget) {
  if (newWidget.planConfirmation?.interactionId != null &&
      newWidget.planConfirmation?.interactionId !=
          oldWidget.planConfirmation?.interactionId) {
    return true;
  }
  final oldIds = oldWidget.rows.map((row) => row.id).toSet();
  // 现在活动只出现在固定活动条里，不再从 Timeline 推导“当前活动”，因此
  // “有新内容”只由新条目/新计划决定。
  return newWidget.rows.any((row) => !oldIds.contains(row.id));
}
