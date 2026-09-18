part of 'studio_shell.dart';

class _SidebarTile extends StatefulWidget {
  const _SidebarTile({
    required this.selected,
    required this.icon,
    required this.iconColor,
    required this.title,
    this.showTitleTooltip = true,
    this.titleIcon,
    required this.subtitle,
    required this.dense,
    required this.onTap,
    required this.trailing,
    this.markerColor,
  });

  final bool selected;
  final IconData icon;
  final Color iconColor;
  final String title;

  /// 是否在标题文本上提供完整名称 Tooltip；存在 recovery issue 时由
  /// 整行诊断 Tooltip 接管，避免名称提示覆盖诊断。
  final bool showTitleTooltip;

  /// 标题旁的只读标识（例如会话工作树图标）；不承载可交互状态。
  final Widget? titleIcon;

  final String subtitle;
  final bool dense;
  final VoidCallback? onTap;
  final Widget trailing;
  final Color? markerColor;

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
    final trailingVisible =
        widget.markerColor == null || widget.selected || _hovering || _focused;
    final titleText = Text(
      widget.title,
      maxLines: 2,
      overflow: TextOverflow.ellipsis,
      style: context.text.labelLarge?.copyWith(
        color: foreground,
        fontWeight: widget.selected ? FontWeight.w600 : FontWeight.w500,
      ),
    );
    final titleContent = widget.titleIcon == null
        ? titleText
        : Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Flexible(child: titleText),
              const SizedBox(width: 4),
              widget.titleIcon!,
            ],
          );
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
              onFocusChange: (focused) => setState(() => _focused = focused),
              onTap: widget.onTap,
              hoverColor: context.colors.surface.withValues(alpha: 0.72),
              child: Padding(
                padding: EdgeInsets.fromLTRB(
                  10,
                  widget.dense ? 6 : 8,
                  4,
                  widget.dense ? 6 : 8,
                ),
                child: Row(
                  children: [
                    if (widget.markerColor == null)
                      Icon(widget.icon, size: 17, color: widget.iconColor)
                    else
                      SizedBox(
                        width: 17,
                        child: Center(
                          child: DecoratedBox(
                            decoration: BoxDecoration(
                              color: widget.markerColor,
                              borderRadius: BorderRadius.circular(
                                StudioRadii.pill,
                              ),
                            ),
                            child: const SizedBox.square(dimension: 5),
                          ),
                        ),
                      ),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          widget.showTitleTooltip
                              ? Tooltip(
                                  message: widget.title,
                                  child: titleContent,
                                )
                              : titleContent,
                          if (widget.subtitle.isNotEmpty) ...[
                            const SizedBox(height: 1),
                            Text(
                              widget.subtitle,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: context.text.bodySmall?.copyWith(
                                color: widget.selected
                                    ? context.colors.onSurfaceVariant
                                    : context.colors.onSurfaceVariant,
                              ),
                            ),
                          ],
                        ],
                      ),
                    ),
                    Visibility(
                      visible: trailingVisible,
                      maintainSize: true,
                      maintainAnimation: true,
                      maintainState: true,
                      child: IconTheme.merge(
                        data: IconThemeData(
                          color: context.colors.onSurfaceVariant,
                        ),
                        child: widget.trailing,
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
