import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../app/theme/studio_tokens.dart';

class StatusDetailPopover extends StatefulWidget {
  const StatusDetailPopover({
    required this.child,
    required this.detailBuilder,
    required this.semanticsLabel,
    required this.semanticsValue,
    this.onFocusChange,
    this.width = 300,
    super.key,
  });

  final Widget child;
  final WidgetBuilder detailBuilder;
  final String semanticsLabel;
  final String semanticsValue;
  final ValueChanged<bool>? onFocusChange;
  final double width;

  @override
  State<StatusDetailPopover> createState() => _StatusDetailPopoverState();
}

class _StatusDetailPopoverState extends State<StatusDetailPopover> {
  final GlobalKey _targetKey = GlobalKey();
  OverlayEntry? _entry;
  Timer? _hideTimer;
  bool _focused = false;

  @override
  void dispose() {
    _hideTimer?.cancel();
    _hide();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Semantics(
      container: true,
      label: widget.semanticsLabel,
      value: widget.semanticsValue,
      button: true,
      focusable: true,
      focused: _focused,
      onTap: _toggle,
      child: FocusableActionDetector(
        onFocusChange: _handleFocusChange,
        shortcuts: const {
          SingleActivator(LogicalKeyboardKey.enter): ActivateIntent(),
          SingleActivator(LogicalKeyboardKey.space): ActivateIntent(),
          SingleActivator(LogicalKeyboardKey.escape):
              _DismissStatusDetailIntent(),
        },
        actions: {
          ActivateIntent: CallbackAction<ActivateIntent>(
            onInvoke: (_) {
              _toggle();
              return null;
            },
          ),
          _DismissStatusDetailIntent:
              CallbackAction<_DismissStatusDetailIntent>(
                onInvoke: (_) {
                  _hide();
                  return null;
                },
              ),
        },
        child: KeyedSubtree(
          key: _targetKey,
          child: Listener(
            behavior: HitTestBehavior.translucent,
            onPointerDown: (_) => _toggle(),
            onPointerHover: (_) => _show(),
            child: MouseRegion(
              onEnter: (_) => _show(),
              onExit: (_) => _scheduleHide(),
              child: widget.child,
            ),
          ),
        ),
      ),
    );
  }

  void _handleFocusChange(bool focused) {
    setState(() => _focused = focused);
    widget.onFocusChange?.call(focused);
    if (focused) {
      _cancelHide();
    } else {
      _scheduleHide();
    }
  }

  void _toggle() {
    if (_entry == null) {
      _show();
    } else {
      _hide();
    }
  }

  void _show() {
    _cancelHide();
    if (_entry != null) {
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
    final maxLeft = math.max(8.0, overlaySize.width - widget.width - 8);
    final left = targetTopLeft.dx.clamp(8.0, maxLeft).toDouble();
    final bottom = math.max(8.0, overlaySize.height - targetTopLeft.dy + 8);
    final theme = Theme.of(context);
    _entry = OverlayEntry(
      builder: (context) {
        return Theme(
          data: theme,
          child: Positioned.fill(
            child: IgnorePointer(
              child: Stack(
                children: [
                  Positioned(
                    left: left,
                    bottom: bottom,
                    width: widget.width,
                    child: Material(
                      color: Colors.transparent,
                      child: DecoratedBox(
                        decoration: BoxDecoration(
                          color: theme.colorScheme.surfaceContainerLowest,
                          border: Border.all(
                            color: theme.colorScheme.outlineVariant,
                          ),
                          borderRadius: BorderRadius.circular(StudioRadii.md),
                          boxShadow: StudioShadows.lifted(
                            theme.colorScheme.shadow,
                          ),
                        ),
                        child: Padding(
                          padding: const EdgeInsets.all(16),
                          child: widget.detailBuilder(context),
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
    overlay.insert(_entry!);
  }

  void _scheduleHide() {
    if (_focused) {
      _cancelHide();
      return;
    }
    _cancelHide();
    _hideTimer = Timer(const Duration(milliseconds: 120), _hide);
  }

  void _cancelHide() {
    _hideTimer?.cancel();
    _hideTimer = null;
  }

  void _hide() {
    _entry?.remove();
    _entry = null;
  }
}

class _DismissStatusDetailIntent extends Intent {
  const _DismissStatusDetailIntent();
}

class StatusDetailPanel extends StatelessWidget {
  const StatusDetailPanel({
    required this.title,
    required this.children,
    super.key,
  });

  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          title.toUpperCase(),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: context.text.labelSmall?.copyWith(
            color: context.studioInkSoft.withValues(alpha: 0.72),
            fontFamily: 'Consolas',
            fontSize: 9.5,
            fontWeight: FontWeight.w600,
            letterSpacing: 1,
          ),
        ),
        const SizedBox(height: 10),
        ...children,
      ],
    );
  }
}

class StatusDetailRow extends StatelessWidget {
  const StatusDetailRow({
    required this.label,
    required this.value,
    this.valueMaxLines = 1,
    super.key,
  });

  final String label;
  final String value;
  final int valueMaxLines;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3.5),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                color: context.studioInkSoft.withValues(alpha: 0.78),
                height: 1.25,
              ),
            ),
          ),
          const SizedBox(width: 14),
          Flexible(
            flex: 2,
            child: Text(
              value,
              maxLines: valueMaxLines,
              overflow: TextOverflow.ellipsis,
              textAlign: TextAlign.right,
              style: context.text.bodySmall?.copyWith(
                color: context.studioInk,
                fontWeight: FontWeight.w600,
                height: 1.25,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class StatusDetailIconRow extends StatelessWidget {
  const StatusDetailIconRow({
    required this.icon,
    required this.title,
    required this.detail,
    required this.iconColor,
    required this.backgroundColor,
    super.key,
  });

  final IconData icon;
  final String title;
  final String detail;
  final Color iconColor;
  final Color backgroundColor;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox.square(
            dimension: 22,
            child: DecoratedBox(
              decoration: BoxDecoration(
                color: backgroundColor,
                borderRadius: BorderRadius.circular(6),
              ),
              child: Icon(icon, size: 13, color: iconColor),
            ),
          ),
          const SizedBox(width: 9),
          Expanded(
            child: Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: title,
                    style: const TextStyle(fontWeight: FontWeight.w600),
                  ),
                  TextSpan(text: ' · $detail'),
                ],
              ),
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                color: context.studioInk,
                height: 1.35,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
