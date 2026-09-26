part of 'studio_shell.dart';

class AgentWorkspacePane extends ConsumerStatefulWidget {
  const AgentWorkspacePane({super.key});

  @override
  ConsumerState<AgentWorkspacePane> createState() => _AgentWorkspacePaneState();
}

class _AgentWorkspacePaneState extends ConsumerState<AgentWorkspacePane> {
  static const _todoPanelWidth = 304.0;
  static const _defaultPlanPanelWidth = 560.0;
  static const _minimumPlanPanelWidth = 320.0;
  static const _maximumPlanPanelWidth = 720.0;
  static const _minimumTimelineWidth = 560.0;
  static const _minimumPlanTimelineWidth = 600.0;

  /// footer 的高度上限 = 窗口高度 − 时间线**最小可视高度**。
  ///
  /// 这里刻意不用固定比例（例如半屏）：固定比例在矮窗口会把 composer 与状态栏之后的
  /// 剩余高度压到几像素，展开详情退化成「假展开」（实测 640×600 只剩 8.5px）。改为
  /// 先给时间线留出随窗口缩放的最小可视高度，再把其余高度交给 footer。
  static const _minimumTimelineHeight = 96.0;
  static const _maximumTimelineReserve = 200.0;
  static const _timelineReserveFraction = 0.3;

  final _scaffoldKey = GlobalKey<ScaffoldState>();
  final Map<String, bool> _todoExpandedByThread = {};
  final Map<String, String> _expandedPlanByThread = {};
  final Map<String, String> _autoOpenedPlanByThread = {};
  double? _planPanelWidth;

  @override
  Widget build(BuildContext context) {
    final asyncLayout = ref.watch(selectedWorkspaceLayoutProvider);
    final asyncStartPage = ref.watch(startPageProvider);
    return asyncLayout.when(
      loading: () => const StudioWorkspaceLoading(),
      error: (error, stackTrace) => Center(child: Text(error.toString())),
      data: (layout) {
        if (layout == null) {
          return asyncStartPage.when(
            loading: () => const StudioWorkspaceLoading(),
            error: (error, stackTrace) => Center(child: Text(error.toString())),
            data: (startPage) => startPage.isStartPage
                ? _StudioStartPage(view: startPage)
                : const StudioWorkspaceLoading(),
          );
        }
        return LayoutBuilder(
          builder: (context, constraints) {
            final todo = layout.todo;
            final threadId = layout.threadId;
            final plan = layout.planConfirmation;
            if (plan == null) {
              _expandedPlanByThread.remove(threadId);
            }
            // 覆盖与否只取决于窗口能否并排放置默认宽度面板与最小对话区，
            // 不受用户当前目标宽度影响，避免拖拽跨阈值时布局跳变打断手势。
            final planOverlaysTimeline =
                constraints.maxWidth <
                _defaultPlanPanelWidth + _minimumPlanTimelineWidth;
            final planExpanded =
                plan != null &&
                _expandedPlanByThread[threadId] == plan.interactionId;
            // 并排保留对话区最小可用宽度；窄窗覆盖时面板可占满可用宽度。
            final planMaximumWidth = planOverlaysTimeline
                ? constraints.maxWidth
                : constraints.maxWidth - _minimumPlanTimelineWidth;
            final planPanelWidth = _clampPlanPanelWidth(
              _planPanelWidth ?? _defaultPlanPanelWidth,
              planMaximumWidth,
            );
            final todoInDrawer =
                constraints.maxWidth < _todoPanelWidth + _minimumTimelineWidth;
            final todoExpanded = _todoExpandedByThread[threadId] ?? false;
            final footerMaxHeight = constraints.hasBoundedHeight
                ? _footerMaxHeight(constraints.maxHeight)
                : null;
            if (plan != null &&
                _autoOpenedPlanByThread[threadId] != plan.interactionId) {
              _autoOpenedPlanByThread[threadId] = plan.interactionId;
              WidgetsBinding.instance.addPostFrameCallback((_) {
                if (!mounted) return;
                final current = ref.read(selectedWorkspaceLayoutProvider).value;
                if (current?.threadId != threadId ||
                    current?.planConfirmation?.interactionId !=
                        plan.interactionId) {
                  return;
                }
                setState(
                  () => _expandedPlanByThread[threadId] = plan.interactionId,
                );
              });
            }
            return Scaffold(
              key: _scaffoldKey,
              backgroundColor: context.colors.surface,
              endDrawerEnableOpenDragGesture: false,
              endDrawer: !planExpanded && todoInDrawer && todo != null
                  ? Drawer(
                      width: 328,
                      backgroundColor: context.colors.surfaceContainer,
                      child: TodoPanel(
                        key: const ValueKey('todo-drawer-panel'),
                        todo: todo,
                        inDrawer: true,
                        onClose: () =>
                            _scaffoldKey.currentState?.closeEndDrawer(),
                      ),
                    )
                  : null,
              body: Column(
                children: [
                  // 首帧后自动打开当前选择；打开前仍保留显式入口与输入区，
                  // 便于用户直接开始交互或在失败后重试。
                  if (layout.needsOpen) _OpenThreadBanner(threadId: threadId),
                  Expanded(
                    child: Stack(
                      children: [
                        Positioned.fill(
                          child: Row(
                            children: [
                              Expanded(
                                child: Column(
                                  children: [
                                    Expanded(
                                      child: Stack(
                                        children: [
                                          Positioned.fill(
                                            child: _AgentTimelineHost(
                                              threadId: threadId,
                                              planConfirmation: plan,
                                              planExpanded: planExpanded,
                                              onPlanToggle: plan == null
                                                  ? null
                                                  : () => _togglePlan(
                                                      threadId,
                                                      plan.interactionId,
                                                    ),
                                            ),
                                          ),
                                          if (layout.loadError
                                              case final error?)
                                            Positioned(
                                              top: 0,
                                              left: 0,
                                              right: 0,
                                              child: Material(
                                                color: context
                                                    .colors
                                                    .errorContainer,
                                                child: Padding(
                                                  padding: const EdgeInsets.all(
                                                    16,
                                                  ),
                                                  child: Row(
                                                    children: [
                                                      Expanded(
                                                        child: Text(error),
                                                      ),
                                                      TextButton(
                                                        onPressed: () => ref
                                                            .read(
                                                              studioControllerProvider
                                                                  .notifier,
                                                            )
                                                            .retryThreadLoad(
                                                              threadId,
                                                            ),
                                                        child: Text(
                                                          context
                                                              .l10n
                                                              .runtimeFatalRetry,
                                                        ),
                                                      ),
                                                    ],
                                                  ),
                                                ),
                                              ),
                                            ),
                                          if (layout.isLoading)
                                            Positioned.fill(
                                              child: StudioWorkspaceLoading(
                                                key: const ValueKey(
                                                  'agent-workspace-loading',
                                                ),
                                                hasContent:
                                                    ref
                                                        .watch(
                                                          agentTimelineProvider(
                                                            threadId,
                                                          ),
                                                        )
                                                        .value
                                                        ?.rows
                                                        .isNotEmpty ??
                                                    false,
                                              ),
                                            ),
                                          if (planOverlaysTimeline &&
                                              plan != null &&
                                              planExpanded)
                                            Positioned(
                                              top: 0,
                                              right: 0,
                                              bottom: 0,
                                              width: planPanelWidth,
                                              child: _buildPlanPanel(
                                                plan: plan,
                                                threadId: threadId,
                                                maximumWidth: planMaximumWidth,
                                                overlay: true,
                                              ),
                                            ),
                                        ],
                                      ),
                                    ),
                                    _AdaptiveFooter(
                                      threadId: threadId,
                                      maxHeight: footerMaxHeight,
                                      showTodo: !planExpanded && todo != null,
                                      todoExpanded:
                                          !planExpanded && todoExpanded,
                                      onToggleTodo: planExpanded || todo == null
                                          ? null
                                          : () {
                                              if (todoInDrawer) {
                                                _scaffoldKey.currentState
                                                    ?.openEndDrawer();
                                              } else {
                                                setState(
                                                  () =>
                                                      _todoExpandedByThread[threadId] =
                                                          !todoExpanded,
                                                );
                                              }
                                            },
                                    ),
                                  ],
                                ),
                              ),
                              if (!planOverlaysTimeline &&
                                  plan != null &&
                                  planExpanded)
                                SizedBox(
                                  width: planPanelWidth,
                                  child: _buildPlanPanel(
                                    plan: plan,
                                    threadId: threadId,
                                    maximumWidth: planMaximumWidth,
                                    overlay: false,
                                  ),
                                ),
                              if (!planExpanded &&
                                  !todoInDrawer &&
                                  todo != null &&
                                  todoExpanded)
                                SizedBox(
                                  width: _todoPanelWidth,
                                  child: TodoPanel(
                                    key: const ValueKey('todo-side-panel'),
                                    todo: todo,
                                    onClose: () => setState(
                                      () => _todoExpandedByThread[threadId] =
                                          false,
                                    ),
                                  ),
                                ),
                            ],
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            );
          },
        );
      },
    );
  }

  void _togglePlan(String threadId, String interactionId) {
    setState(() {
      if (_expandedPlanByThread[threadId] == interactionId) {
        _expandedPlanByThread.remove(threadId);
      } else {
        _expandedPlanByThread[threadId] = interactionId;
      }
    });
  }

  void _closePlan(String threadId) {
    setState(() => _expandedPlanByThread.remove(threadId));
  }

  /// footer 可用高度上限：先给时间线留下最小可视高度，剩余高度交给 footer。
  double _footerMaxHeight(double viewportHeight) {
    final reserve = (viewportHeight * _timelineReserveFraction)
        .clamp(_minimumTimelineHeight, _maximumTimelineReserve)
        .toDouble();
    final available = viewportHeight - reserve;
    return available > 0 ? available : 0;
  }

  /// 计划详情面板：左边缘分隔条 + 内容。并排与覆盖共用，避免语义偏差。
  Widget _buildPlanPanel({
    required PlanConfirmationView plan,
    required String threadId,
    required double maximumWidth,
    required bool overlay,
  }) {
    return Row(
      children: [
        _PlanResizeHandle(
          key: StudioDriverKeys.planResizeHandle,
          label: context.l10n.planResize,
          onDelta: (delta) => _applyPlanPanelDelta(delta, maximumWidth),
          onReset: _resetPlanPanelWidth,
        ),
        Expanded(
          child: PlanDetailPanel(
            plan: plan,
            overlay: overlay,
            onClose: () => _closePlan(threadId),
          ),
        ),
      ],
    );
  }

  /// 在当前目标宽度基础上累计增量，而非从本次 build 的宽度重算：
  /// 先把当前状态收敛到当前限制，再加 delta 并再次收敛，确保连续拖拽或连按
  /// 方向键的增量逐步累加，不会相互覆盖。
  void _applyPlanPanelDelta(double delta, double maximumWidth) {
    setState(() {
      final base = _clampPlanPanelWidth(
        _planPanelWidth ?? _defaultPlanPanelWidth,
        maximumWidth,
      );
      _planPanelWidth = _clampPlanPanelWidth(base + delta, maximumWidth);
    });
  }

  void _resetPlanPanelWidth() {
    setState(() => _planPanelWidth = _defaultPlanPanelWidth);
  }

  /// 将期望宽度收敛到 [最小值, 可用空间]；可用空间极小时直接填满，避免溢出窗口。
  static double _clampPlanPanelWidth(double desired, double available) {
    final maximum = available < _minimumPlanPanelWidth
        ? available
        : available.clamp(_minimumPlanPanelWidth, _maximumPlanPanelWidth);
    final minimum = maximum < _minimumPlanPanelWidth
        ? maximum
        : _minimumPlanPanelWidth;
    return desired.clamp(minimum, maximum);
  }
}

/// 计划详情面板左边缘分隔条（design/13-plan.md §13）。
///
/// 拖拽或方向键调整宽度，Home 复位；宽度只属于当前 GUI 的临时视图状态，
/// 不写入 Plan 状态机或持久化。
const double _planResizeHandleWidth = 5.0;
const double _planResizeStep = 24.0;

class _PlanResizeHandle extends StatefulWidget {
  const _PlanResizeHandle({
    required this.label,
    required this.onDelta,
    required this.onReset,
    super.key,
  });

  /// 无障碍语义与 tooltip 文案。
  final String label;

  /// 宽度增量，正值加宽、负值收窄。
  final ValueChanged<double> onDelta;

  /// 恢复到默认宽度。
  final VoidCallback onReset;

  @override
  State<_PlanResizeHandle> createState() => _PlanResizeHandleState();
}

class _PlanResizeHandleState extends State<_PlanResizeHandle> {
  bool _hovering = false;
  bool _focused = false;

  @override
  Widget build(BuildContext context) {
    final active = _hovering || _focused;
    final color = active
        ? context.colors.primary.withValues(alpha: 0.6)
        : context.colors.outlineVariant.withValues(alpha: 0.5);
    return Semantics(
      label: widget.label,
      child: Tooltip(
        message: widget.label,
        child: Focus(
          onFocusChange: (value) => setState(() => _focused = value),
          onKeyEvent: (_, event) {
            if (event is! KeyDownEvent) {
              return KeyEventResult.ignored;
            }
            final key = event.logicalKey;
            if (key == LogicalKeyboardKey.home) {
              widget.onReset();
              return KeyEventResult.handled;
            }
            if (key == LogicalKeyboardKey.arrowLeft) {
              widget.onDelta(_planResizeStep);
              return KeyEventResult.handled;
            }
            if (key == LogicalKeyboardKey.arrowRight) {
              widget.onDelta(-_planResizeStep);
              return KeyEventResult.handled;
            }
            return KeyEventResult.ignored;
          },
          child: MouseRegion(
            cursor: SystemMouseCursors.resizeColumn,
            onEnter: (_) => setState(() => _hovering = true),
            onExit: (_) => setState(() => _hovering = false),
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onHorizontalDragUpdate: (event) =>
                  widget.onDelta(-event.delta.dx),
              onDoubleTap: widget.onReset,
              child: Container(width: _planResizeHandleWidth, color: color),
            ),
          ),
        ),
      ),
    );
  }
}

/// footer 预算低于该值时启用紧凑布局：让 composer 适度收起留白与输入行数（保留操作），
/// 把省下的高度让给展开详情，同时避免矮窗口 `RenderFlex` 溢出。
const _compactFooterBudget = 400.0;

/// 在窗口较矮时把可伸缩的活动条压进剩余空间，而不是让整条 footer 滚动。
///
/// 整条 footer 放进滚动容器会把「装不下」变成「输入区被裁到窗口外」：容器只保证
/// `maxHeight` 内的内容可见，超出的部分（正好是底部的输入区与发送/停止按钮）被静默
/// 裁掉。这里只给出高度上限，footer 内部把唯一可伸缩的活动条压到剩余高度里（见
/// [_Footer]），因此输入区始终完整可见，活动条详情在自身范围内滚动。
///
/// footer 的总高度上限由 [AgentWorkspacePane] 按「窗口高度 − 时间线最小可视高度」给出；
/// 这里仅据该预算决定是否进入紧凑布局（唯一所有者，不在别处重复读窗口尺寸）。
class _AdaptiveFooter extends StatelessWidget {
  const _AdaptiveFooter({
    required this.threadId,
    required this.maxHeight,
    required this.showTodo,
    required this.todoExpanded,
    required this.onToggleTodo,
  });

  final String threadId;
  final double? maxHeight;
  final bool showTodo;
  final bool todoExpanded;
  final VoidCallback? onToggleTodo;

  @override
  Widget build(BuildContext context) {
    if (maxHeight == null) {
      return _Footer(
        threadId: threadId,
        showTodo: showTodo,
        todoExpanded: todoExpanded,
        onToggleTodo: onToggleTodo,
        compact: false,
        contentKey: StudioDriverKeys.workspaceFooterScroll,
      );
    }
    return ConstrainedBox(
      constraints: BoxConstraints(maxHeight: maxHeight!),
      child: LayoutBuilder(
        // ConstrainedBox 已把窗口预算收成有界 maxHeight，这里据此决定紧凑与否，避免在
        // composer 里再读一次窗口尺寸形成第二套事实源。
        builder: (context, constraints) => _Footer(
          threadId: threadId,
          showTodo: showTodo,
          todoExpanded: todoExpanded,
          onToggleTodo: onToggleTodo,
          compact: constraints.maxHeight < _compactFooterBudget,
          // footer 区域的可定位句柄：内容不再整体滚动，但保持同一 key，便于驱动按区域定位。
          contentKey: StudioDriverKeys.workspaceFooterScroll,
        ),
      ),
    );
  }
}

/// 已选中但尚未打开的会话：显式打开入口。
///
/// 此处不读取会话状态、不打开 history 数据库、不加载历史（§6.1）。用户点击“打开会话”
/// （或在输入区输入/提交、滚动历史）才会激活会话；打开只恢复当前状态与订阅，不自动
/// 续跑模型/工具执行。
class _OpenThreadBanner extends ConsumerWidget {
  const _OpenThreadBanner({required this.threadId});

  final String threadId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return ColoredBox(
      key: StudioDriverKeys.unopenedThread,
      color: context.colors.surfaceContainer,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 7),
        child: Row(
          children: [
            Tooltip(
              message: context.l10n.threadUnopenedTitle,
              child: Icon(
                Icons.play_circle_outline,
                size: 18,
                color: context.colors.onSurfaceVariant,
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                context.l10n.threadUnopenedBody,
                style: Theme.of(context).textTheme.labelSmall
                    ?.copyWith(color: context.colors.onSurfaceVariant),
              ),
            ),
            KeyedSubtree(
              key: StudioDriverKeys.openSelectedThread,
              child: FilledButton(
                key: StudioDriverKeys.openThread(threadId),
                onPressed: () => unawaited(
                  ref
                      .read(studioControllerProvider.notifier)
                      .openThread(threadId),
                ),
                child: Text(context.l10n.threadOpenAction),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _StudioStartPage extends StatelessWidget {
  const _StudioStartPage({required this.view});

  final StartPageView view;

  @override
  Widget build(BuildContext context) {
    final project = view.project;
    return Scaffold(
      key: StudioDriverKeys.startPage,
      backgroundColor: context.colors.surface,
      body: LayoutBuilder(
        builder: (context, constraints) {
          return SingleChildScrollView(
            child: ConstrainedBox(
              constraints: BoxConstraints(minHeight: constraints.maxHeight),
              child: Center(
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 24,
                    vertical: 32,
                  ),
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(
                      maxWidth: StudioLayout.conversationWidth,
                    ),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          project == null
                              ? context.l10n.startPageOpenProjectTitle
                              : context.l10n.startPageWelcome,
                          textAlign: TextAlign.center,
                          style: Theme.of(context).textTheme.headlineSmall
                              ?.copyWith(
                                color: context.colors.onSurface,
                                fontWeight: FontWeight.w600,
                              ),
                        ),
                        const SizedBox(height: 10),
                        Text(
                          project == null
                              ? context.l10n.startPageOpenProjectBody
                              : context.l10n.startPageProject(project.name),
                          textAlign: TextAlign.center,
                          style: Theme.of(context).textTheme.bodyMedium
                              ?.copyWith(
                                color: context.colors.onSurfaceVariant,
                              ),
                        ),
                        const SizedBox(height: 28),
                        StartPageComposerDock(view: view),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}

class _AgentTimelineHost extends ConsumerWidget {
  const _AgentTimelineHost({
    required this.threadId,
    required this.planConfirmation,
    required this.planExpanded,
    required this.onPlanToggle,
  });

  final String threadId;
  final PlanConfirmationView? planConfirmation;
  final bool planExpanded;
  final VoidCallback? onPlanToggle;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncTimeline = ref.watch(agentTimelineProvider(threadId));
    return asyncTimeline.when(
      loading: () => const SizedBox.shrink(),
      error: (error, stackTrace) => Center(child: Text(error.toString())),
      data: (timeline) {
        if (timeline == null) {
          return const SizedBox.shrink();
        }
        return TimelineView(
          threadId: threadId,
          rows: timeline.rows,
          turn: timeline.turn,
          planConfirmation: planConfirmation,
          planExpanded: planExpanded,
          onPlanToggle: onPlanToggle,
          isLoadingOlder: timeline.isLoadingOlderHistory,
          isLoadingNewer:
              timeline.history.isLoading &&
              timeline.history.direction == TimelineDirection.newer,
          olderError: timeline.history.errorMessage,
          newerError: timeline.history.newerError,
          anchor: timeline.history.anchor,
          hasNewer: timeline.history.hasNewer,
          olderCursor: timeline.history.olderCursor,
          newerCursor: timeline.history.newerCursor,
          windowEpoch: timeline.history.epoch,
          previewedItemIds: timeline.history.previewedItemIds,
          loadingItemIds: timeline.history.loadingItemIds,
          itemBodyErrors: timeline.history.itemBodyErrors,
          pendingItemBodyIds: timeline.history.pendingItemBodyIds,
          unavailableItemIds: timeline.history.unavailableItemIds,
          onLoadItemBody: (itemId) => unawaited(
            ref
                .read(studioControllerProvider.notifier)
                .loadItemBody(threadId, itemId),
          ),
          onVisibleItemBodies: (itemIds) => unawaited(
            ref
                .read(studioControllerProvider.notifier)
                .ensureItemBodies(threadId, itemIds),
          ),
          onAnchorChanged: (anchor) => ref
              .read(studioControllerProvider.notifier)
              .updateTimelineAnchor(threadId, anchor),
          onJumpToLatest: () => unawaited(
            ref.read(studioControllerProvider.notifier).jumpToLatest(threadId),
          ),
          onLoadNewer: timeline.history.hasNewer
              ? () => unawaited(
                  ref
                      .read(studioControllerProvider.notifier)
                      .loadNewerHistory(threadId),
                )
              : null,
          onLoadOlder: timeline.hasOlderHistory
              ? () => unawaited(
                  ref
                      .read(studioControllerProvider.notifier)
                      .loadOlderHistory(threadId),
                )
              : null,
        );
      },
    );
  }
}
