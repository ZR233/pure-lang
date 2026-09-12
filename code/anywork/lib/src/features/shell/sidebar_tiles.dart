part of 'studio_shell.dart';

class _CompactSidebarTile extends StatefulWidget {
  const _CompactSidebarTile({
    required this.selected,
    required this.tooltip,
    required this.icon,
    required this.onTap,
    required this.actionTooltip,
    required this.actionIcon,
    required this.onAction,
    this.iconColor,
    this.actionKey,
    this.secondaryActionKey,
    this.secondaryActionTooltip,
    this.secondaryActionIcon,
    this.onSecondaryAction,
  });

  final bool selected;
  final String tooltip;
  final IconData icon;
  final Color? iconColor;
  final VoidCallback? onTap;
  final String actionTooltip;
  final IconData actionIcon;
  final VoidCallback? onAction;
  final Key? actionKey;
  final Key? secondaryActionKey;
  final String? secondaryActionTooltip;
  final IconData? secondaryActionIcon;
  final VoidCallback? onSecondaryAction;

  @override
  State<_CompactSidebarTile> createState() => _CompactSidebarTileState();
}

class _CompactSidebarTileState extends State<_CompactSidebarTile> {
  bool _hovering = false;

  @override
  Widget build(BuildContext context) {
    final actionVisible =
        widget.onAction != null && (widget.selected || _hovering);
    final secondaryActionVisible =
        widget.onSecondaryAction != null && (widget.selected || _hovering);
    return MouseRegion(
      onEnter: (_) => setState(() => _hovering = true),
      onExit: (_) => setState(() => _hovering = false),
      child: SizedBox(
        width: 44,
        height: 44,
        child: Stack(
          children: [
            Positioned.fill(
              child: Tooltip(
                message: widget.tooltip,
                child: IconButton(
                  isSelected: widget.selected,
                  icon: Icon(widget.icon, color: widget.iconColor),
                  onPressed: widget.onTap,
                ),
              ),
            ),
            if (widget.onAction != null)
              Positioned(
                right: 0,
                bottom: 0,
                child: IgnorePointer(
                  ignoring: !actionVisible,
                  child: AnimatedOpacity(
                    opacity: actionVisible ? 1 : 0,
                    duration: const Duration(milliseconds: 120),
                    child: IconButton(
                      key: widget.actionKey,
                      tooltip: widget.actionTooltip,
                      style: IconButton.styleFrom(
                        minimumSize: const Size.square(20),
                        maximumSize: const Size.square(20),
                        padding: EdgeInsets.zero,
                        backgroundColor: context.colors.surface,
                      ),
                      icon: Icon(widget.actionIcon, size: 12),
                      onPressed: widget.onAction,
                    ),
                  ),
                ),
              ),
            if (widget.onSecondaryAction != null &&
                widget.secondaryActionIcon != null)
              Positioned(
                right: 0,
                top: 0,
                child: IgnorePointer(
                  ignoring: !secondaryActionVisible,
                  child: AnimatedOpacity(
                    opacity: secondaryActionVisible ? 1 : 0,
                    duration: const Duration(milliseconds: 120),
                    child: IconButton(
                      key: widget.secondaryActionKey,
                      tooltip: widget.secondaryActionTooltip,
                      style: IconButton.styleFrom(
                        minimumSize: const Size.square(20),
                        maximumSize: const Size.square(20),
                        padding: EdgeInsets.zero,
                        backgroundColor: context.colors.surface,
                      ),
                      icon: Icon(widget.secondaryActionIcon, size: 12),
                      onPressed: widget.onSecondaryAction,
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _SidebarTile extends StatefulWidget {
  const _SidebarTile({
    required this.selected,
    required this.icon,
    required this.iconColor,
    required this.title,
    this.showTitleTooltip = true,
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
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: context.text.labelLarge?.copyWith(
        color: foreground,
        fontWeight: widget.selected ? FontWeight.w600 : FontWeight.w500,
      ),
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
                              ? Tooltip(message: widget.title, child: titleText)
                              : titleText,
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
                    if (trailingVisible)
                      IconTheme.merge(
                        data: IconThemeData(
                          color: context.colors.onSurfaceVariant,
                        ),
                        child: widget.trailing,
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
