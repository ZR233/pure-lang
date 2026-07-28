import 'dart:math' as math;

import 'package:flutter/material.dart';

class UpwardPopupMenu<T> extends StatelessWidget {
  const UpwardPopupMenu({
    required this.tooltip,
    required this.itemBuilder,
    required this.child,
    this.initialValue,
    this.onSelected,
    this.enabled = true,
    this.gap = 8,
    this.constraints,
    super.key,
  });

  final String tooltip;
  final PopupMenuItemBuilder<T> itemBuilder;
  final Widget child;
  final T? initialValue;
  final PopupMenuItemSelected<T>? onSelected;
  final bool enabled;
  final double gap;
  final BoxConstraints? constraints;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: enabled ? () => _showMenu(context) : null,
        child: child,
      ),
    );
  }

  Future<void> _showMenu(BuildContext context) async {
    final items = itemBuilder(context);
    if (items.isEmpty) {
      return;
    }
    final button = context.findRenderObject();
    final overlay = Navigator.of(context).overlay?.context.findRenderObject();
    if (button is! RenderBox || overlay is! RenderBox) {
      return;
    }
    final buttonTopLeft = button.localToGlobal(Offset.zero, ancestor: overlay);
    final estimatedHeight = items.fold<double>(
      16,
      (height, item) => height + item.height,
    );

    final selected = await showMenu<T>(
      context: context,
      initialValue: initialValue,
      constraints: constraints,
      items: items,
      positionBuilder: (context, layout) {
        final overlaySize = layout.biggest;
        final availableTop = math.max(
          8.0,
          buttonTopLeft.dy - estimatedHeight - gap,
        );
        final maxTop = math.max(8.0, overlaySize.height - estimatedHeight - 8);
        final top = math.min(availableTop, maxTop);
        final rect = Rect.fromLTWH(
          buttonTopLeft.dx,
          top,
          button.size.width,
          button.size.height,
        );
        return RelativeRect.fromRect(rect, Offset.zero & overlaySize);
      },
    );
    if (selected != null) {
      onSelected?.call(selected);
    }
  }
}
