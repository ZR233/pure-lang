import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';

class StatusBarItem extends StatefulWidget {
  const StatusBarItem({
    required this.label,
    this.icon,
    this.trailingIcon,
    this.tooltip,
    this.detailBuilder,
    this.detailWidth = 300,
    this.enabled = true,
    this.enableHover = true,
    this.interactive = false,
    this.maxWidth = 180,
    super.key,
  });

  final String label;
  final IconData? icon;
  final IconData? trailingIcon;
  final String? tooltip;
  final WidgetBuilder? detailBuilder;
  final double detailWidth;
  final bool enabled;
  final bool enableHover;
  final bool interactive;
  final double maxWidth;

  @override
  State<StatusBarItem> createState() => _StatusBarItemState();
}

class _StatusBarItemState extends State<StatusBarItem> {
  final GlobalKey _targetKey = GlobalKey();
  final Object _tapRegionGroup = Object();
  OverlayEntry? _entry;
  Timer? _hideTimer;
  bool _hovering = false;
  bool _focused = false;

  @override
  void dispose() {
    _hideTimer?.cancel();
    _hideDetail();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final foreground = widget.enabled
        ? context.studioInkSoft
        : context.studioInkSoft.withValues(alpha: 0.52);
    var row = Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (widget.icon != null) ...[
          Icon(
            widget.icon,
            size: 13,
            color: foreground.withValues(alpha: 0.76),
          ),
          const SizedBox(width: 7),
        ],
        ConstrainedBox(
          constraints: BoxConstraints(maxWidth: widget.maxWidth),
          child: Text(
            widget.label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.text.labelSmall?.copyWith(
              color: foreground,
              height: 1,
            ),
          ),
        ),
        if (widget.trailingIcon != null) ...[
          const SizedBox(width: 3),
          Icon(
            widget.trailingIcon,
            size: 14,
            color: foreground.withValues(alpha: 0.56),
          ),
        ],
      ],
    );
    final detailBuilder = widget.detailBuilder;
    if (detailBuilder != null) {
      row = Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          MouseRegion(
            onEnter: (_) {
              setState(() => _hovering = true);
              _showDetail();
            },
            onExit: (_) {
              setState(() => _hovering = false);
              _scheduleHideDetail();
            },
            child: Listener(
              behavior: HitTestBehavior.translucent,
              onPointerDown: (_) => _handlePointerDown(),
              onPointerHover: (_) => _showDetail(),
              child: row,
            ),
          ),
        ],
      );
    }
    final highlighted = (_hovering || _focused) && widget.enabled;
    final content = AnimatedContainer(
      key: _targetKey,
      duration: const Duration(milliseconds: 120),
      height: 26,
      padding: const EdgeInsets.symmetric(horizontal: 7),
      decoration: BoxDecoration(
        color: highlighted
            ? context.studioPaper.withValues(alpha: 0.76)
            : Colors.transparent,
        borderRadius: BorderRadius.circular(StudioRadii.xs),
      ),
      child: row,
    );
    final hoverable = widget.enableHover
        ? MouseRegion(
            onEnter: (_) => setState(() => _hovering = true),
            onExit: (_) => setState(() => _hovering = false),
            child: content,
          )
        : content;
    final interactive = Focus(
      onFocusChange: _handleFocusChange,
      child: hoverable,
    );
    final chip = Padding(
      padding: const EdgeInsets.only(right: 2),
      child: interactive,
    );
    final groupedChip = widget.interactive && detailBuilder != null
        ? TapRegion(groupId: _tapRegionGroup, child: chip)
        : chip;
    final tooltip = widget.tooltip;
    if (tooltip == null || tooltip.isEmpty) {
      return groupedChip;
    }
    return Tooltip(message: tooltip, child: groupedChip);
  }

  void _toggleDetail() {
    if (_entry == null) {
      _showDetail();
    } else {
      _hideDetail();
    }
  }

  void _handleFocusChange(bool focused) {
    setState(() => _focused = focused);
    if (focused && widget.detailBuilder != null) {
      _showDetail();
    } else if (!focused) {
      _scheduleHideDetail();
    }
  }

  void _handlePointerDown() {
    if (widget.interactive && _hovering && _entry != null) {
      _cancelHideDetail();
      return;
    }
    _toggleDetail();
  }

  void _showDetail() {
    _hideTimer?.cancel();
    if (_entry != null || widget.detailBuilder == null) {
      return;
    }
    final overlay = Overlay.of(context);
    final target = _targetKey.currentContext?.findRenderObject();
    final overlayRenderObject = overlay.context.findRenderObject();
    if (target is! RenderBox || overlayRenderObject is! RenderBox) {
      return;
    }
    final targetTopLeft = target.localToGlobal(
      Offset.zero,
      ancestor: overlayRenderObject,
    );
    final overlaySize = overlayRenderObject.size;
    final maxLeft = math.max(8.0, overlaySize.width - widget.detailWidth - 8);
    final left = targetTopLeft.dx.clamp(8.0, maxLeft).toDouble();
    final bottom = math.max(8.0, overlaySize.height - targetTopLeft.dy + 8);
    final theme = Theme.of(context);
    final detailBuilder = widget.detailBuilder!;
    final interactive = widget.interactive;
    _entry = OverlayEntry(
      builder: (context) {
        final card = _detailCard(theme, detailBuilder);
        final positioned = Positioned(
          left: left,
          bottom: bottom,
          width: widget.detailWidth,
          child: interactive
              ? MouseRegion(
                  onEnter: (_) => _cancelHideDetail(),
                  onExit: (_) => _scheduleHideDetail(),
                  child: TapRegion(
                    groupId: _tapRegionGroup,
                    onTapOutside: (_) => _hideDetail(),
                    child: card,
                  ),
                )
              : IgnorePointer(child: card),
        );
        return Theme(
          data: theme,
          child: Positioned.fill(child: Stack(children: [positioned])),
        );
      },
    );
    overlay.insert(_entry!);
  }

  void _cancelHideDetail() {
    _hideTimer?.cancel();
    _hideTimer = null;
  }

  void _scheduleHideDetail() {
    _hideTimer?.cancel();
    _hideTimer = Timer(const Duration(milliseconds: 120), _hideDetail);
  }

  void _hideDetail() {
    _entry?.remove();
    _entry = null;
  }

  Widget _detailCard(ThemeData theme, WidgetBuilder detailBuilder) {
    return Material(
      color: Colors.transparent,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: theme.colorScheme.surfaceContainerLowest,
          border: Border.all(color: theme.colorScheme.outlineVariant),
          borderRadius: BorderRadius.circular(StudioRadii.md),
          boxShadow: StudioShadows.lifted(theme.colorScheme.shadow),
        ),
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: detailBuilder(context),
        ),
      ),
    );
  }
}
