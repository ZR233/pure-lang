part of 'studio_shell.dart';

/// 详情卡期望宽度；超过可用宽度时由共享锚定面按真实安全区收敛，
/// 窄抽屉中自动收窄。
const double _sidebarDetailCardWidth = 300.0;

/// 侧栏行的按需悬浮/键盘详情宿主。
///
/// 锚定、真实布局、开合、真实安全区收敛、打开期间的 canonical 数据刷新，
/// 以及路由被覆盖、触发器卸载、窗口 resize、祖先滚动时的关闭全部交给共享
/// [StudioAnchoredSurface]。本组件只内联悬停/聚焦/Escape 输入胶水，并把
/// 行本体**真实的焦点节点**交给该面作为唯一 trigger 焦点：不叠加额外可聚焦
/// 包装层，因此第一焦点停点仍是行本体（Enter 选择会话、聚焦即显示快捷操作），
/// 详情由同一次真实悬停或聚焦驱动。卡片自身可进入阅读与复制，全文不省略、
/// 内部有界滚动，也不新建第二份 Overlay 或位置生命周期。
class _SidebarHoverDetail extends StatefulWidget {
  const _SidebarHoverDetail({
    required this.childBuilder,
    required this.detailBuilder,
  });

  /// 构建行本体，并把共享焦点节点交给它挂到真实可聚焦控件上。
  /// [focusNode] 挂到真实可聚焦控件；[onMenuOpenChanged] 由行内更多菜单接到
  /// `onOpen`/`onClose`，仅在真实打开时暂停重开（`onClose` 复位，不自动重开）；
  /// [onMenuBeforeOpen] 由菜单的 `onBeforeOpen` 接到同步收起，在子菜单建立
  /// **之前**关闭父详情，避免 `RawMenuAnchor` 级联关掉刚打开的菜单。
  final Widget Function(
    BuildContext context,
    FocusNode focusNode,
    ValueChanged<bool> onMenuOpenChanged,
    VoidCallback onMenuBeforeOpen,
  )
  childBuilder;

  final WidgetBuilder detailBuilder;

  @override
  State<_SidebarHoverDetail> createState() => _SidebarHoverDetailState();
}

class _SidebarHoverDetailState extends State<_SidebarHoverDetail> {
  final GlobalKey<StudioAnchoredSurfaceState> _surfaceKey = GlobalKey(
    debugLabel: 'sidebar-row-detail-surface',
  );
  final FocusNode _focusNode = FocusNode(debugLabel: 'sidebar-row-trigger');
  final ScrollController _scrollController = ScrollController(
    debugLabel: 'sidebar-detail-content',
  );
  Timer? _hideTimer;
  bool _focused = false;

  /// 行内更多菜单是否正打开。菜单是详情锚定面的子浮层，框架不会因打开子菜单
  /// 而关闭父级详情：菜单建立前同步收起父详情（见 `onMenuBeforeOpen`），并在
  /// 真实打开期间暂停重开，避免详情卡盖住菜单项、拦截菜单点击。
  bool _menuOpen = false;

  StudioAnchoredSurfaceState? get _surface => _surfaceKey.currentState;

  @override
  void initState() {
    super.initState();
    _focusNode.addListener(_handleFocusChange);
  }

  @override
  void dispose() {
    _hideTimer?.cancel();
    _focusNode.removeListener(_handleFocusChange);
    _focusNode.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  /// 键盘聚焦/离开行本体：聚焦立即展示详情，离开后延迟收起。关闭会把焦点
  /// 恢复回行本体时消费一次性抑制，避免把这次恢复当成新的键盘入口而重开。
  void _handleFocusChange() {
    // 只有行本体自己拿到主焦点才展开详情：行内后代控件（快捷操作、更多菜单
    // 触发按钮）聚焦不应自动开详情，否则打开菜单时详情会盖住菜单项。
    final focused = _focusNode.hasPrimaryFocus;
    if (focused == _focused) return;
    _focused = focused;
    if (focused) {
      _cancelHide();
      if (!(_surface?.consumeAutoOpenSuppression() ?? false)) {
        _show();
      }
    } else {
      _scheduleHide();
    }
  }

  void _handleHover(bool hovering) {
    if (hovering) {
      _show();
    } else {
      _scheduleHide();
    }
  }

  void _show() {
    _cancelHide();
    if (!mounted || _menuOpen) return;
    _surface?.open(focusTrigger: false);
  }

  /// 行内更多菜单真实开合通知：`onOpen` 置暂停重开 flag，`onClose` 复位。
  /// 不在此处收起父详情——即使父详情已关闭，`RawMenuAnchor.close` 的级联仍可能
  /// 关闭刚建立的子菜单；`open` 时只取消待收起延时，`close` 只清 flag，不自动
  /// 重开（交给后续 hover/聚焦）。
  void _handleMenuOpenChanged(bool open) {
    _menuOpen = open;
    if (open) _cancelHide();
  }

  /// 菜单建立**之前**的同步钩子（接共享 `onBeforeOpen`）：立即收起父详情，使
  /// 即将建立的子菜单不被详情卡遮挡。此处**不**置 `_menuOpen`——菜单仍可能因
  /// 空间不足被拒绝打开，暂停重开的 flag 只能由真实 `onOpen` 决定，避免被拒后
  /// 永久暂停重开。
  void _handleMenuBeforeOpen() {
    _hide();
  }

  void _cancelHide() {
    _hideTimer?.cancel();
    _hideTimer = null;
  }

  void _scheduleHide() {
    _cancelHide();
    if (_focused) return;
    _hideTimer = Timer(const Duration(milliseconds: 120), _hide);
  }

  void _hide() {
    _cancelHide();
    _surface?.close();
  }

  @override
  Widget build(BuildContext context) {
    return StudioAnchoredSurface(
      key: _surfaceKey,
      fixedWidth: _sidebarDetailCardWidth,
      // 侧栏详情贴行**侧向**展开（LTR 优先文字尾侧），不覆盖同列相邻行，
      // 相邻行仍可悬停/点击选择；空间不足换侧并按安全区收敛，若两侧都不足
      // 最小可读宽度则由共享面拒绝打开而不是回退成遮挡行。
      placement: StudioAnchorPlacement.beside,
      userConstraints: const BoxConstraints(minWidth: 180),
      focusTriggerNode: _focusNode,
      onClose: _cancelHide,
      triggerBuilder: _buildTrigger,
      contentBuilder: _buildContent,
    );
  }

  Widget _buildTrigger(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => _handleHover(true),
      onExit: (_) => _handleHover(false),
      child: Actions(
        actions: <Type, Action<Intent>>{
          // 共享锚定面只为浮层内的内容安装关闭动作；行本体仍持有焦点时由
          // 这里消费 Escape，避免冒泡触发更外层（如覆盖层）的默认关闭。
          DismissIntent: CallbackAction<DismissIntent>(
            onInvoke: (_) {
              _surface?.close(reason: StudioSurfaceCloseReason.keyboardDismiss);
              return null;
            },
          ),
        },
        child: widget.childBuilder(
          context,
          _focusNode,
          _handleMenuOpenChanged,
          _handleMenuBeforeOpen,
        ),
      ),
    );
  }

  Widget _buildContent(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => _cancelHide(),
      onExit: (_) => _scheduleHide(),
      child: Scrollbar(
        controller: _scrollController,
        thumbVisibility: true,
        child: SingleChildScrollView(
          controller: _scrollController,
          // 详情卡内容使用明确的内缩网格。共享锚定面负责卡片外框，
          // 这里负责内容本身的留白，使标题、前置图标和复制按钮共用
          // 同一组左右边界。
          padding: const EdgeInsets.fromLTRB(14, 4, 10, 4),
          child: widget.detailBuilder(context),
        ),
      ),
    );
  }
}

/// 详情卡中的一行：前置标识（图标或状态点）+ 值，可选尾部动作。
class _SidebarDetailRow extends StatelessWidget {
  const _SidebarDetailRow({
    required this.leading,
    required this.value,
    this.trailing,
  });

  final Widget leading;
  final String value;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox.square(dimension: 18, child: Center(child: leading)),
          const SizedBox(width: 9),
          Expanded(
            child: Text(
              value,
              style: context.text.bodySmall?.copyWith(height: 1.3),
            ),
          ),
          if (trailing != null) ...[const SizedBox(width: 8), trailing!],
        ],
      ),
    );
  }
}

/// 相对更新时间：一分钟内、分钟、小时、天内使用本地化相对文案，
/// 更早的更新回退为既有列表的绝对时间写法（月/日 时:分），不引入新的
/// 日期格式化依赖。
String _relativeUpdatedLabel(BuildContext context, DateTime updatedAt) {
  final age = DateTime.now().difference(updatedAt);
  if (age.isNegative || age.inSeconds < 60) {
    return context.l10n.sidebarDetailUpdatedNow;
  }
  if (age.inMinutes < 60) {
    return context.l10n.sidebarDetailUpdatedMinutesAgo(age.inMinutes);
  }
  if (age.inHours < 24) {
    return context.l10n.sidebarDetailUpdatedHoursAgo(age.inHours);
  }
  if (age.inDays <= 7) {
    return context.l10n.sidebarDetailUpdatedDaysAgo(age.inDays);
  }
  final hour = updatedAt.hour.toString().padLeft(2, '0');
  final minute = updatedAt.minute.toString().padLeft(2, '0');
  return '${updatedAt.month}/${updatedAt.day} $hour:$minute';
}
