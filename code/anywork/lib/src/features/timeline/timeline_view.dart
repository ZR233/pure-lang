import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:gpt_markdown/gpt_markdown.dart';

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

  final ScrollController _controller = ScrollController();
  final Map<String, _TimelineScrollSnapshot> _threadScroll = {};
  final Set<String> _expandedReasoningGroups = {};
  final Set<String> _expandedToolGroups = {};
  final _ThreadImageLoader _imageLoader = _ThreadImageLoader();
  bool _followingBottom = true;
  bool _detachedByUser = false;
  bool _programmaticScroll = false;
  bool _bottomScrollScheduled = false;
  bool _tailResumeScheduled = false;
  bool _scrollBoundsCorrectionScheduled = false;
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

  final _centerKey = GlobalKey();
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
  String? _centerId;

  /// 最近一次拖动方向是否朝更早的内容（`ScrollDirection.forward`）。
  ///
  /// 由 [_handleScrollPositionChanged] 维护，供 [_schedulePrefetch] 决定向前/向后补页。
  bool _scrollingOlder = true;
  int _pendingNewEvents = 0;
  int _contentVersion = 0;
  _TimelineRestore _pendingRestore = const _TimelineRestore.bottom();

  /// 待恢复的阅读意图（身份 + offset）是否还没被当前布局表达出来。
  ///
  /// 切回会话时正文可能先以**预览**布局出现（`omittedBytes > 0`）：目标内容偏移会被当前
  /// `maxScrollExtent` 钳位成更浅的位置。此时不能把钳位结果当成读者新的阅读位置——否则
  /// 完整正文到位、滚动范围变大后，读者会停在这个更浅的位置上（锚点被覆盖）。保持 `true`
  /// 直到目标能被布局表达；期间不据此判定“跟随最新”，也不重建/发布锚点。
  ///
  /// 意图建立时（[didUpdateWidget] / [_restoreThreadState] / [_rebaseToVisibleAnchor]）
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
      _expandedReasoningGroups.clear();
      _expandedToolGroups.clear();
      _rowWidgets.clear();
      _rowKeys.clear();
      _visibleItemBodySignature = null;
      _lastSelectedText = null;
      _imageLoader.clear();
      _bottomSlack = 0;
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
      widget.planConfirmation,
    );
    if (nextContentVersion == _contentVersion) return;
    final savedAnchor = widget.anchor;
    // 上一次恢复还没被几何表达（目标偏移被预览布局钳位）时，**意图优先**于当前可见位置：
    // 完整正文到达后要按同一身份 + offset 重新落位，而不是拿钳位后的 `_captureAnchor()`
    // 重建锚点，把读者真实的阅读位置覆盖掉。意图行还没进入窗口时同样保留意图：
    final restoreIntent = _restoreClamped ? _pendingRestore.anchor : null;
    final anchor =
        restoreIntent ??
        (savedAnchor != null &&
                !savedAnchor.followingBottom &&
                _anchorRowId(savedAnchor.itemId) != null &&
                !widget.rows.any((row) => row.id == _centerId)
            ? savedAnchor
            : _captureAnchor());
    final hasNewEvent = _hasNewTimelineEvent(oldWidget, widget);
    _contentVersion = nextContentVersion;
    // 贴在窗口末尾时，内容变化（追加 / 历史分页 / 窗口替换）都保持跟随：`hasNewer`
    // 只表示窗口之外还有更新条目（由「跳到最新」入口表达），不代表读者离开了末尾。
    // 之前这里要求 `!hasNewer`，于是历史分页会把跟随态写成非跟随锚点，重开时又围绕
    // 这条锚点重新取窗。
    if (!_detachedByUser && _followingBottom) {
      _pendingNewEvents = 0;
      _scheduleBottomScroll();
    } else {
      _followingBottom = false;
      _detachedByUser = true;
      if (hasNewEvent || widget.hasNewer) _pendingNewEvents += 1;
      if (anchor != null && _anchorRowId(anchor.itemId) != null) {
        _programmaticScroll = true;
        if (_controller.hasClients) {
          _controller.position.correctPixels(-anchor.offset);
        }
        _centerId = _anchorRowId(anchor.itemId);
        _pendingRestore = _TimelineRestore.anchor(anchor);
        // 意图刚建立/重建：先当作还没被本帧几何表达，帧末按实际滚动范围复核
        // （见 [_refreshRestorePending]）。正文还只有预览时这个标记会一直保持到
        // 完整正文撑高滚动范围的那一帧，恢复不会被钳位值顶替。
        _restoreClamped = true;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) _restorePendingPosition();
        });
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
    _loadingTimer?.cancel();
    _imageLoader.clear();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final activeTurn = widget.turn?.state.isBusy == true ? widget.turn : null;
    if (widget.rows.isEmpty &&
        activeTurn == null &&
        widget.planConfirmation == null) {
      return const _EmptyTimeline();
    }
    // 本帧结束后按实际几何做一次归一 + 诊断发布。
    _scheduleGeometrySync();
    final rows = widget.rows;
    final planSummary = widget.planConfirmation == null
        ? null
        : TimelinePlanSummaryCard(
            plan: widget.planConfirmation!,
            expanded: widget.planExpanded,
            onPressed: widget.onPlanToggle ?? () {},
          );
    final centerIndex = math.max(
      0,
      rows.indexWhere((row) => row.id == _centerId),
    );
    final activeIds = rows.map((row) => row.id).toSet();
    _rowKeys.removeWhere((id, _) => !activeIds.contains(id));
    _rowWidgets.removeWhere((id, _) => !activeIds.contains(id));
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
    return _ThreadImageCacheScope(
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
                      Expanded(
                        child: SizedBox(
                          key: _viewportKey,
                          child: NotificationListener<ScrollMetricsNotification>(
                            onNotification: _handleScrollMetricsChanged,
                            child: NotificationListener<ScrollUpdateNotification>(
                              onNotification: _handleScrollUpdate,
                              child: NotificationListener<ScrollEndNotification>(
                                onNotification: (_) {
                                  if (!_programmaticScroll) {
                                    WidgetsBinding.instance
                                        .addPostFrameCallback((_) {
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
                                    // 反向区：center 之前的历史行，按反序排布，不参与贴底几何。
                                    _itemSliver(
                                      rows
                                          .take(centerIndex)
                                          .toList()
                                          .reversed
                                          .toList(),
                                      isCenter: false,
                                    ),
                                    // 正向区（center 起）整体贴底：留白只在布局期以
                                    // `paintOrigin` 表达，不进 `scrollExtent`，因此既不会
                                    // 变成可滚动内容，也不会改变 item 锚点。只有反向区为空
                                    // （center 就是第一行）时才启用贴底；上翻历史拆出反向区后
                                    // 几何与普通 sliver 完全一致。
                                    _BottomAlignedSliver(
                                      key: _centerKey,
                                      alignShortContentToBottom:
                                          centerIndex == 0,
                                      onSlackChanged: _handleBottomSlackChanged,
                                      child: SliverMainAxisGroup(
                                        slivers: [
                                          _itemSliver(
                                            rows.skip(centerIndex).toList(),
                                            isCenter: true,
                                          ),
                                          SliverPadding(
                                            padding: const EdgeInsets.symmetric(
                                              horizontal: 24,
                                            ),
                                            sliver: SliverToBoxAdapter(
                                              child: _TimelineTail(
                                                planSummary: planSummary,
                                              ),
                                            ),
                                          ),
                                        ],
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
                        Padding(
                          padding: const EdgeInsets.fromLTRB(24, 0, 24, 12),
                          child: Align(
                            alignment: Alignment.centerRight,
                            child: _JumpToLatestButton(
                              pendingCount: _pendingNewEvents,
                              onPressed: _jumpToLatest,
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
  /// 内容比视口短（没有可滚动范围）时方向也会是 `forward`，但位置并不能离开末尾，因此
  /// 再用 `pixels > minScrollExtent` 收紧，避免把「拖不动的短会话」误标成脱离底部。
  bool get _userPullingAwayFromBottom {
    if (!_controller.hasClients) return false;
    final position = _controller.position;
    if (position.userScrollDirection != ScrollDirection.forward) return false;
    return position.pixels > position.minScrollExtent + 0.5;
  }

  /// 只读诊断：记录一次由真实指针拖动产生的滚动更新（见 [_userDragUpdates]）。
  ///
  /// 只累加计数，不改几何、不触发重建，也不会吞掉通知。
  bool _handleScrollUpdate(ScrollUpdateNotification notification) {
    if (notification.dragDetails != null) {
      _userDragUpdates += 1;
      // 读者主动接管位置：放弃尚未被布局表达的恢复意图（不再回落到原锚点）。
      _restoreClamped = false;
    } else if (!_programmaticScroll &&
        _controller.hasClients &&
        _controller.position.userScrollDirection != ScrollDirection.idle) {
      // 滚轮 / 触控板等非拖动滚动同样是读者接管。
      _restoreClamped = false;
    }
    return false;
  }

  void _handleScrollPositionChanged() {
    // 位置变化（含程序化 jump）后按帧做几何同步并发布只读诊断。
    _scheduleGeometrySync();
    if (!_controller.hasClients || _programmaticScroll) {
      return;
    }
    final direction = _controller.position.userScrollDirection;
    // 拖动方向既是"是否正在上翻历史"的判据，也决定向前/向后补页（见 [_schedulePrefetch]）：
    // `forward` = 手指下拖、朝更早内容；`reverse` = 朝更新的内容。`idle` 保持上一次方向。
    switch (direction) {
      case ScrollDirection.forward:
        _scrollingOlder = true;
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
    if (nearBottom && !pullingAway) {
      if (!_followingBottom ||
          _detachedByUser ||
          _pendingNewEvents != 0 ||
          _centerId != null) {
        _followLatestBottom();
      }
    } else if (direction != ScrollDirection.idle &&
        (_followingBottom || !_detachedByUser)) {
      setState(() {
        _followingBottom = false;
        _detachedByUser = true;
      });
    }
    _saveThreadState(widget.threadId);
    _schedulePrefetch();
  }

  bool _handleScrollMetricsChanged(ScrollMetricsNotification notification) {
    // 布局改变滚动范围后按帧做几何同步并发布只读诊断。
    _scheduleGeometrySync();
    _schedulePrefetch();
    if (_programmaticScroll) return false;
    final metrics = notification.metrics;
    // 读者正在主动上翻历史时，这一帧的指标变化正是这次拖动本身造成的（布局里
    // `pixels` 变了就会派发 `ScrollMetricsNotification`）。此时不能把「位置在末尾附近 /
    // 不再贴底」当成「想跟随最新」，否则会在拖动累积离开阈值之前每帧拉回末尾
    // （见 [_userPullingAwayFromBottom]）。
    if (!_userPullingAwayFromBottom) {
      // 内容或历史页改变布局后，滚动位置可能已经落在末尾：按与滚动事件相同的规则恢复
      // 跟随并清掉"新内容"计数。否则分页恢复/内容替换后会长久残留一个已经回到末尾的
      // 提示层，既误导读者，也会在右下角压住正文。
      if (!widget.hasNewer &&
          metrics.extentAfter <= _bottomThreshold &&
          (!_followingBottom ||
              _detachedByUser ||
              _pendingNewEvents != 0 ||
              _centerId != null)) {
        _scheduleTailResume();
      }
      if (_followingBottom &&
          !_detachedByUser &&
          !widget.hasNewer &&
          metrics.extentAfter > 0.5) {
        _scheduleBottomScroll();
      }
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

  /// 布局/内容变化后已经落在末尾时恢复跟随。
  ///
  /// 与 [`_handleScrollPositionChanged`] 的 `nearBottom` 规则一致：末尾（"没有更新条目"
  /// 且距内容末尾不超过 [`_bottomThreshold`]）就是跟随状态，必须清掉"脱离 + 新内容计数"，
  /// 否则历史分页/窗口替换后提示层会停留在一个已经回到末尾的位置上。这里只恢复状态，
  /// 也不主动移动阅读位置——真正的贴底由下一次 [ScrollMetricsNotification] 的跟随分支完成。
  ///
  /// 恢复跟随同时归一 `center` 拆分（见 [_followLatestBottom]）：只恢复标志位、留着
  /// 拆分会让正向区一直只覆盖内容尾部，几何退化且再也回不到末尾。
  void _scheduleTailResume() {
    if (_tailResumeScheduled) return;
    _tailResumeScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _tailResumeScheduled = false;
      if (!mounted || !_controller.hasClients || _programmaticScroll) return;
      // 帧末时读者可能仍在拖动（本帧的指标变化就是这次拖动造成的）：这时不能被
      // 「近底」规则拉回末尾。
      if (_userPullingAwayFromBottom) return;
      // 恢复意图还没表达出来（当前位置是预览布局下的钳位结果）时不恢复跟随，
      // 把阅读位置留给完整正文到位后的重新落位。
      if (_restoreClamped) return;
      if (widget.hasNewer ||
          _controller.position.extentAfter > _bottomThreshold ||
          (_followingBottom &&
              !_detachedByUser &&
              _pendingNewEvents == 0 &&
              _centerId == null)) {
        return;
      }
      _followLatestBottom();
    });
  }

  void _scrollToBottom() {
    if (!_controller.hasClients) return;
    if (_centerId != null) {
      // 有 `center` 拆分时“末尾”不能用当前 maxScrollExtent 表达：拆分下正向区只覆盖
      // 内容尾部，max 已被钳成 0。归一拆分并按清掉后的几何落到末尾。
      _followLatestBottom();
      return;
    }
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
        _followingBottom = true;
        _detachedByUser = false;
        _pendingNewEvents = 0;
      });
      _saveThreadState(widget.threadId);
    }
  }

  /// Keep a tool image visible when its inline preview changes the timeline height.
  void _revealExpandedToolImage(BuildContext entryContext) {
    if (_followingBottom || !_detachedByUser) {
      setState(() {
        _followingBottom = false;
        _detachedByUser = true;
      });
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !entryContext.mounted) return;
      Scrollable.ensureVisible(entryContext, alignment: 0.12);
      _saveThreadState(widget.threadId);
    });
  }

  void _jumpToLatest() {
    // 跟随最新必须归一 `center` 拆分，否则“跳到最新”只会落到拆分下那个被钳成 0 的
    // maxScrollExtent 上（也就是停在拆分点，而不是内容末尾）。
    _followLatestBottom();
    widget.onJumpToLatest?.call();
    _scheduleBottomScroll();
  }

  /// 归一为“跟随最新”并落到内容末尾。
  ///
  /// `center` 拆分只描述“已脱离底部、正在上翻历史”的阅读位置：跟随最新时正向区必须
  /// 独占整个消息列。否则正向区可能只剩末尾几条，屏幕上会退化成“内容贴在视口顶部 +
  /// 下方整片空白”，而且此时 `maxScrollExtent` 被钳成 0，滚动再也回不到末尾。这条路径
  /// 只走**明确的跟随意图**（跳最新 / 近底 / 结尾恢复 / 主动贴底）；同一种退化几何若出现在
  /// 阅读位置上，改由 [_collapseSplitToForwardLayout] 换布局表示，不经过这里。
  ///
  /// 清掉拆分会让滚动范围重算，所以末尾位置按**清掉之后**的几何估算
  /// （见 [_followBottomPixels]）并用 [ScrollPosition.correctPixels] 直接定位，
  /// 避免中间帧先落到顶部再跳回底部。
  void _followLatestBottom() {
    if (!_normalizeFollowLatest()) return;
    _saveThreadState(widget.threadId);
  }

  /// 把状态归一为“跟随最新”并落到内容末尾（几何说明见 [_followLatestBottom]）。
  ///
  /// 返回是否真的改动了状态。[didUpdateWidget] 里调用时本帧紧随其后就会 `build`，
  /// 直接改字段即可，不必再安排一次重建（大窗口下那是整棵消息列的重复构建）。
  bool _normalizeFollowLatest({bool rebuild = true}) {
    final needsReset =
        !_followingBottom ||
        _detachedByUser ||
        _pendingNewEvents != 0 ||
        _centerId != null;
    if (!needsReset) return false;
    if (_centerId != null && _controller.hasClients) {
      final target = _followBottomPixels(_controller.position);
      if (target != null) {
        _programmaticScroll = true;
        try {
          _controller.position.correctPixels(target);
        } finally {
          _programmaticScroll = false;
        }
      }
    }
    void apply() {
      _followingBottom = true;
      _detachedByUser = false;
      _pendingNewEvents = 0;
      _centerId = null;
      // 明确的“跟随最新”取代尚未表达的阅读锚点。
      _restoreClamped = false;
    }

    if (rebuild) {
      setState(apply);
    } else {
      apply();
    }
    return true;
  }

  /// 清掉 `center` 拆分后，内容末尾对应的滚动位置。
  ///
  /// 正向区独占内容后 `maxScrollExtent = 内容高度 - 视口高度`；内容高度 = 反向区滚动
  /// 范围（`minScrollExtent` 的绝对值）+ 正向区滚动范围（`center` sliver 的
  /// `geometry.scrollExtent`，含收束区）。内容比视口短时为 0：此时贴底由
  /// [_BottomAlignedSliver] 直接表达。
  double? _followBottomPixels(ScrollPosition position) {
    final forward = _forwardScrollExtent();
    if (forward == null) return null;
    final reverse = position.minScrollExtent < 0
        ? -position.minScrollExtent
        : 0.0;
    final bottom = reverse + forward - position.viewportDimension;
    return bottom > 0 ? bottom : 0.0;
  }

  /// `center` 起正向区（消息行 + 收束区）的滚动范围。
  double? _forwardScrollExtent() {
    final sliver = _centerKey.currentContext?.findRenderObject();
    return sliver is RenderSliver ? sliver.geometry?.scrollExtent : null;
  }

  void _toggleReasoning(String groupId) {
    setState(() {
      if (!_expandedReasoningGroups.remove(groupId)) {
        _expandedReasoningGroups.add(groupId);
      }
    });
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
    final position = _controller.hasClients ? _controller.position : null;
    if (position != null && _hasDegenerateSplit(position)) {
      // 拆分几何表达不了这个阅读位置：换成未拆分布局并按同一坐标换算（见
      // [_collapseSplitToForwardLayout]），不把它当成“想跟随最新”。
      _collapseSplitToForwardLayout(position);
      return;
    }
    // 布局定稿后按**实际几何**刷新“恢复意图是否已表达”，并在目标刚变得可表达的
    // 这一帧把位置真正落到目标。正文补齐后行变高才会出现这个转换，因此不会每帧
    // 重复 jump，也不会反复请求正文。
    if (_refreshRestorePending()) _applyRestoreTarget();
    _publishVisibleItemBodies();
    _publishScrollDiagnostic();
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

  /// 是否处于“有 `center` 拆分 + 正向区填不满视口”的退化几何。
  ///
  /// 判据只看本帧实际几何：`pixels >= 0` 时反向区绘制范围为 0（视口按
  /// `centerOffset = -pixels` 给负向区分配 paint extent），正向区自身又比视口短，
  /// 于是内容底部到视口底部之间必然空着。用户主动上翻（`pixels < 0`）时反向区可见，
  /// 不满足该判据，阅读位置不会被抢。`center` 落在首行时没有反向区，几何与普通正向
  /// 列表一致（贴底由 [_BottomAlignedSliver] 表达），同样不在此列。
  ///
  /// 这只描述几何能不能表达当前阅读位置，与“读者是否想跟随最新”无关：旧窗口
  /// （`hasNewer == true`）同样可能退化，修复方式是换布局表示而不是改跟随状态。
  bool _hasDegenerateSplit(ScrollPosition position) {
    if (_centerId == null || position.pixels < -0.5) return false;
    if (widget.rows.indexWhere((row) => row.id == _centerId) <= 0) return false;
    final forward = _forwardScrollExtent();
    if (forward == null) return false;
    return forward < position.viewportDimension - 0.5;
  }

  /// 正向区比视口短时换布局表示：整个窗口行序回到正向区，同一阅读位置按未拆分坐标换算。
  ///
  /// 只转换坐标与布局，不判定读者想跟随最新，也不改 `followingBottom/detachedByUser`：
  /// 拆分几何里视口顶部对应内容偏移 `pixels`，未拆分布局把反向区的内容也算进同一串行，
  /// 因此同一视觉位置就是 `-minScrollExtent + pixels`（`-minScrollExtent` 即拆分下排在
  /// `center` 之前的行高度之和）。换算结果按未拆分布局的滚动范围钳位——未拆分布局的
  /// `maxScrollExtent` 就是内容末尾（见 [_followBottomPixels]），所以正向区短于一屏时
  /// 落点是内容末尾：锚点行仍在视口内，只是不再在视口顶部硬留一片空白。
  ///
  /// 目标值用**清掉拆分后**的几何算好后直接写入 `pixels`，因此下一帧就是最终位置，
  /// 不存在“先停在拆分点再跳”的中间帧。
  void _collapseSplitToForwardLayout(ScrollPosition position) {
    final reverse = position.minScrollExtent < 0
        ? position.minScrollExtent
        : 0.0;
    final forwardMax = math.max(
      0.0,
      _followBottomPixels(position) ?? position.pixels,
    );
    final target = (position.pixels - reverse)
        .clamp(0.0, forwardMax)
        .toDouble();
    _programmaticScroll = true;
    try {
      position.correctPixels(target);
    } finally {
      _programmaticScroll = false;
    }
    setState(() => _centerId = null);
    // 落位后的阅读位置就是新锚点，同步给外层，避免下一次内容变化又拿旧的末行锚点重建拆分。
    _saveThreadState(widget.threadId);
  }

  /// 发布只读滚动几何诊断（驱动快照 `timelineScroll`）。
  ///
  /// 读取方式：`cargo xtask run-gui --driver` 后发 Flutter Driver 的 `snapshot` 命令
  /// （`flutter_driver_command: {"command":"snapshot"}`），取 `timelineScroll` 字段：
  /// `centerId/centerIndex`（是否仍有拆分）、`followingBottom/detachedByUser`、
  /// `pixels/minScrollExtent/maxScrollExtent/viewportDimension/extentAfter`（末尾是否
  /// 贴底、内容是否填满视口：贴底即 `extentAfter ≈ 0`，短内容即 `maxScrollExtent == 0`
  /// 且 `pixels == 0`）、`bottomSlack` 与当前 `anchor`；
  /// `programmaticScroll`（当前是否把滚动当成程序化滚动忽略）与 `userDragUpdates`
  /// （真实拖动累计次数）。定位“上翻历史不动”时先看后两个字段：拖动后 `userDragUpdates`
  /// 增长而 `pixels` 没变，说明手势到达了滚动视图、位置被本状态机复位；不增长则说明手势
  /// 没有到达滚动视图，与锚点/贴底几何无关。
  ///
  /// `restorePending`/`restoreAnchor` 描述“切回会话后锚点恢复是否还等正文长度”，语义是
  /// **帧末按实际滚动范围复核过的**（见 [_refreshRestorePending]）：正文先以有界预览出现时
  /// 目标 offset 会被钳位，此时 `restorePending` 为 `true`、`restoreAnchor` 是读者的原始
  /// 身份 + offset，而 `anchor` 只是钳位结果；完整正文把滚动范围撑高、UI 重新落位之后
  /// `restorePending` 归 `false`，`anchor` 就是读者原来的身份 + offset（`== restoreAnchor`）。
  /// 读者主动接管位置（拖动 / 滚轮 / 跳最新）会直接清掉待恢复意图，此时 `restorePending`
  /// 同样归 `false`，`anchor` 是读者接管后的位置。
  void _publishScrollDiagnostic() {
    final position = _controller.hasClients ? _controller.position : null;
    final centerIndex = widget.rows.indexWhere((row) => row.id == _centerId);
    StudioDriverState.publishTimelineScroll(
      TimelineScrollDiagnostic(
        threadId: widget.threadId,
        centerId: _centerId,
        centerIndex: _centerId == null ? -1 : math.max(0, centerIndex),
        rowCount: widget.rows.length,
        followingBottom: _followingBottom,
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
        // 只读诊断：当前是否把滚动通知当成程序化滚动忽略（见 [_restorePendingPosition]），
        // 以及真实拖动累计观测到的滚动更新次数（见 [_userDragUpdates]）。
        programmaticScroll: _programmaticScroll,
        userDragUpdates: _userDragUpdates,
        anchor: _captureAnchor(),
        // 只读诊断：恢复意图是否还没被当前布局表达（正文仍是预览 → 目标被钳位，等完整正文
        // 撑高滚动范围后重新落位，见 [_refreshRestorePending]）。
        restorePending: _restoreClamped,
        restoreAnchor: _pendingRestore.anchor,
      ),
    );
  }

  void _showLoadingIndicator() {
    if (mounted) setState(() => _showLoading = true);
  }

  void _rebaseToVisibleAnchor() {
    if (_followingBottom && !_detachedByUser) return;
    // 恢复意图还没被几何表达出来时，当前可见位置只是预览布局下的钳位结果：
    // 不能拿它重建锚点（否则原始身份 + offset 会被钳位值覆盖）。
    if (_restoreClamped) return;
    final anchor = _captureAnchor();
    if (anchor == null || _anchorRowId(anchor.itemId) == _centerId) return;
    _programmaticScroll = true;
    _controller.position.correctPixels(-anchor.offset);
    setState(() {
      _centerId = _anchorRowId(anchor.itemId);
      _pendingRestore = _TimelineRestore.anchor(anchor);
    });
    // 拆分是这一帧才建立的：先当作还没被几何表达，由帧末复核确认（见
    // [_refreshRestorePending]），避免在确认前把中间几何当成读者的阅读位置发布出去。
    _restoreClamped = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _restorePendingPosition();
    });
  }

  /// 恢复本线程的阅读位置。
  ///
  /// 跟随底部时**不做 center 拆分**：`_centerId` 留空，整个窗口都落在正向区，
  /// 贴底几何（[_BottomAlignedSliver]）才能成立，短会话重开后也贴底而不是“内容在
  /// 顶部 + 下方大块空白”。`center` 拆分只对“已脱离底部、正在上翻历史”的阅读位置
  /// 有意义，那种状态按记录下来的**身份 + offset** 原样恢复，反向区与 item 锚点语义
  /// 不变；锚点行就是窗口末行时同样成立（那条末行可能是一条高于视口的长答复，读者正停
  /// 在它的开头/中间）。
  ///
  /// 拆分几何表达不了这个位置（正向区比视口短）时不丢锚点：由 [_syncTimelineGeometry]
  /// 换成未拆分布局并按同一坐标换算（见 [_collapseSplitToForwardLayout]）。锚点行还没
  /// 进入窗口时先留空 `center`，由 [didUpdateWidget] 在行到位后按同一身份补建。
  void _restoreThreadState() {
    _programmaticScroll = true;
    final snapshot = _threadScroll[widget.threadId];
    final anchor = widget.anchor ?? snapshot?.anchor;
    final detachedAnchor = anchor != null && !anchor.followingBottom
        ? anchor
        : null;
    final anchorRowId = detachedAnchor == null
        ? null
        : _anchorRowId(detachedAnchor.itemId);
    if (detachedAnchor == null) {
      _followingBottom = true;
      _detachedByUser = false;
      _pendingNewEvents = 0;
      _centerId = null;
      _pendingRestore = const _TimelineRestore.bottom();
      _restoreClamped = false;
    } else {
      // 读者的明确阅读意图：身份 + offset，且非跟随。
      _followingBottom = false;
      _detachedByUser = true;
      _pendingNewEvents = snapshot?.pendingNewEvents ?? 0;
      if (anchorRowId != null && _controller.hasClients) {
        _controller.position.correctPixels(-detachedAnchor.offset);
      }
      _centerId = anchorRowId;
      _pendingRestore = _TimelineRestore.anchor(detachedAnchor);
      // 换会话/重开时不能沿用上一会话的旧标记：意图刚建立，还没经本会话的几何复核，
      // 先当作“未被表达”，由帧末的 [_refreshRestorePending] 按实际滚动范围确认或保持。
      _restoreClamped = true;
    }
    _olderLoadRequested = false;
    _newerLoadRequested = false;
    _updateLoadingIndicator();
  }

  void _restorePendingPosition() {
    // 恢复结束统一放开程序化滚动标记：有的分支不需要移动位置（已经贴底、锚点行还没到位），
    // 调用方设置的标记必须在这里归零，否则用户滚动会被当成程序化滚动忽略。
    //
    // 归零必须**无条件**执行：时间线首帧可能是空态（[_EmptyTimeline]：没有
    // CustomScrollView，控制器尚未附着），此时恢复被推迟，而 [_restoreThreadState] 只在
    // 建态/换会话时调用一次，标记一旦留成 `true` 就再也没有第二次机会归零，
    // `_handleScrollPositionChanged` / `_handleScrollMetricsChanged` 会把此后每一次真实
    // 滚动都当成程序化滚动忽略：既不脱离底部（`detachedByUser` 恒为 false），也不上报锚点。
    try {
      if (!_controller.hasClients) return;
      final anchor = _pendingRestore.anchor;
      if (anchor == null) {
        _restoreClamped = false;
        _scrollToBottom();
      } else if (_centerId != null) {
        // 拆分布局：锚点行就是正向区首行，内容偏移为 0，`-offset` 即目标滚动量。
        //
        // 目标超出当前滚动范围（正文还是预览、还没长到该偏移）时先钳位落位，但把这次
        // 恢复标成“未表达”：完整正文到位、范围变大后按同一身份 + offset 重新落位，而不是
        // 让钳位值成为新的阅读位置。
        final target = -anchor.offset;
        final position = _controller.position;
        final clamped = target
            .clamp(position.minScrollExtent, position.maxScrollExtent)
            .toDouble();
        _restoreClamped = (clamped - target).abs() > 0.5;
        _programmaticScroll = true;
        _controller.jumpTo(clamped);
      } else {
        // 锚点行还没进入窗口：意图保留，等它到位后由 [didUpdateWidget] 按同一身份补建拆分。
        _restoreClamped = true;
      }
    } finally {
      // 锚点行还没进入窗口（尚未建立拆分）时无需换算：本帧不猜位置，[didUpdateWidget] 会
      // 在行到位后按同一身份补建拆分，退化几何由 [_collapseSplitToForwardLayout] 换算。
      _programmaticScroll = false;
    }
    _schedulePrefetch();
  }

  /// 当前布局下把锚点行落到记录 offset 需要的滚动位置；本帧表达不了时为 `null`。
  ///
  /// 锚点行必须就是 `center` 拆分点（正向区首行，其内容偏移为 0），此时行顶的视觉偏移
  /// 恒为 `-pixels`，所以 `-offset` 就是目标滚动量。行还没进入窗口、或退化几何已经把拆分
  /// 换成未拆分布局（见 [_collapseSplitToForwardLayout]）时返回 `null`：前者等 [didUpdateWidget]
  /// 在行到位后按同一身份补建拆分，后者由同一入口在内容变化时按同一身份重建。
  double? _restoreTargetPixels(TimelineAnchor anchor) {
    if (_centerId == null || _centerId != _anchorRowId(anchor.itemId)) {
      return null;
    }
    return -anchor.offset;
  }

  /// 帧末按**本帧实际几何**复核待恢复意图（见 [_restoreClamped]），返回是否刚刚变得可表达。
  ///
  /// [_restorePendingPosition] 只在恢复被触发的那一帧估算一次：重开时正文先以有界预览到达，
  /// 那时的 `maxScrollExtent` 只覆盖预览，目标 offset 会被钳位；完整正文把 sliver 撑高是
  /// **后续帧**才发生的事，没有任何代码在那时重新判断一次，恢复就停在钳位值上（而标记也
  /// 不能反映它）。这里每帧定稿后用新的 `min/maxScrollExtent` 复核：目标仍超出滚动范围就
  /// 继续 pending——期间不发布锚点、不判定“跟随最新”、不把读者拉回底部；一旦能表达就让
  /// 调用方落位一次。
  ///
  /// 只在 [_restoreClamped] 为 true 时工作：读者主动接管（拖动、滚轮、跳最新都会清掉该标记）
  /// 之后不会再被拉回旧锚点。本方法只读几何，不请求正文、不重建窗口，也不会自己安排下一帧
  /// （帧末同步本来每帧最多一次）。
  bool _refreshRestorePending() {
    if (!_restoreClamped) return false;
    final anchor = _pendingRestore.anchor;
    if (anchor == null) {
      // 没有阅读意图（跟随底部）：没有待表达的东西。
      _restoreClamped = false;
      return false;
    }
    if (!_controller.hasClients) return false;
    // 明确的“跟随最新”已经取代尚未表达的阅读意图（例如恢复期间按了「跳到最新」）。
    if (_followingBottom && !_detachedByUser) {
      _restoreClamped = false;
      return false;
    }
    final target = _restoreTargetPixels(anchor);
    if (target == null) return false;
    final position = _controller.position;
    final clamped = target
        .clamp(position.minScrollExtent, position.maxScrollExtent)
        .toDouble();
    if ((clamped - target).abs() > 0.5) {
      // 正文仍是有界预览：滚动范围还表达不了目标，保持 pending，等它撑高后的帧末复核。
      return false;
    }
    _restoreClamped = false;
    return true;
  }

  /// 把锚点行落到待恢复意图记录的视觉偏移，并把落位后的阅读位置同步给外层。
  ///
  /// 目标按拆分几何算出（锚点行是正向区首行，`-offset` 即目标滚动量），落到目标位置后
  /// [_saveThreadState] 才会真正发布锚点——此时 `restorePending` 已归 `false`，发布出去的
  /// 就是读者原来的身份 + offset，而不是预览布局下的钳位值。
  void _applyRestoreTarget() {
    final anchor = _pendingRestore.anchor;
    if (anchor == null || !_controller.hasClients) return;
    final target = _restoreTargetPixels(anchor);
    if (target == null) return;
    final position = _controller.position;
    final clamped = target
        .clamp(position.minScrollExtent, position.maxScrollExtent)
        .toDouble();
    if ((position.pixels - clamped).abs() > 0.5) {
      _programmaticScroll = true;
      try {
        position.jumpTo(clamped);
      } finally {
        _programmaticScroll = false;
      }
    }
    _saveThreadState(widget.threadId);
    // 这一帧的诊断/锚点测量还可能来自 jump 之前的布局：下一帧按落位后的几何再同步一次
    // （每帧最多一次，不会自激）。
    _scheduleGeometrySync();
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
