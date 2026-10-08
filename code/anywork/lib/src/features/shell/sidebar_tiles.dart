part of 'studio_shell.dart';

/// 侧栏紧凑单行条目：标题单行省略，右侧固定状态区与按需快捷操作区。
///
/// 快捷操作在悬停或键盘聚焦时可见，但通过 [Visibility] 保持占位，
/// 出现与消失不挤动标题。触控输入模式下（最近指针输入为触摸/触笔，
/// 见 `_SidebarState` 的输入模式检测）所有行的操作常显并放大命中区，
/// 整个侧栏使用同一种输入模式，不做逐行切换。选中使用拿铁底色与
/// [StudioSelectionMarker] 的细蓝标记。本组件不承载业务状态；状态指示
/// 由调用方在 [trailing] 中以各自的 canonical 事实组装。
class _SidebarTile extends StatefulWidget {
  const _SidebarTile({
    required this.selected,
    required this.title,
    required this.onTap,
    required this.trailing,
    this.actions,
    this.focusNode,
    this.touchMode = false,
  });

  final bool selected;
  final String title;
  final VoidCallback? onTap;

  /// 右侧常驻状态区（例如会话工作树标识与行级运行状态）。
  final Widget trailing;

  /// 悬停或聚焦时出现的快捷操作；为 null 时不占位。
  final Widget? actions;

  /// 行本体的真实焦点节点。存在时由外部（按需详情宿主）与锚定面共享同一
  /// 节点，避免叠加额外可聚焦包装层而改变第一焦点停点；为 null 时由
  /// [InkWell] 自建内部节点。
  final FocusNode? focusNode;

  /// 触控输入模式：操作常显，行高与命中区按触控密度放大。
  final bool touchMode;

  @override
  State<_SidebarTile> createState() => _SidebarTileState();
}

class _SidebarTileState extends State<_SidebarTile> {
  bool _hovering = false;
  bool _focused = false;

  @override
  Widget build(BuildContext context) {
    final foreground = widget.selected
        ? context.colors.onPrimaryContainer
        : context.colors.onSurface;
    final actions = widget.actions;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: MouseRegion(
        onEnter: (_) => setState(() => _hovering = true),
        onExit: (_) => setState(() => _hovering = false),
        child: Material(
          color: widget.selected
              ? context.colors.surfaceContainerHigh
              : Colors.transparent,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(StudioRadii.md),
          ),
          clipBehavior: Clip.antiAlias,
          child: StudioSelectionMarker(
            selected: widget.selected,
            child: InkWell(
              focusNode: widget.focusNode,
              onFocusChange: (focused) => setState(() => _focused = focused),
              onTap: widget.onTap,
              hoverColor: context.colors.surface.withValues(alpha: 0.72),
              child: Padding(
                padding: EdgeInsets.fromLTRB(
                  10,
                  widget.touchMode ? 12 : 7,
                  4,
                  widget.touchMode ? 12 : 7,
                ),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        widget.title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: context.text.labelLarge?.copyWith(
                          color: foreground,
                          fontWeight: widget.selected
                              ? FontWeight.w600
                              : FontWeight.w500,
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                    IconTheme.merge(
                      data: IconThemeData(
                        color: context.colors.onSurfaceVariant,
                      ),
                      child: widget.trailing,
                    ),
                    if (actions != null)
                      Visibility(
                        visible:
                            widget.touchMode ||
                            widget.selected ||
                            _hovering ||
                            _focused,
                        maintainSize: true,
                        maintainAnimation: true,
                        maintainState: true,
                        child: IconTheme.merge(
                          data: IconThemeData(
                            color: context.colors.onSurfaceVariant,
                          ),
                          child: actions,
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
  }
}
