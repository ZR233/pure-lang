import 'package:flutter/material.dart';

import '../app/theme/studio_tokens.dart';

class StudioPanel extends StatelessWidget {
  const StudioPanel({
    required this.child,
    this.padding,
    this.backgroundColor,
    this.borderColor,
    this.radius = StudioRadii.md,
    this.shadow = false,
    super.key,
  });

  final Widget child;
  final EdgeInsetsGeometry? padding;
  final Color? backgroundColor;
  final Color? borderColor;
  final double radius;
  final bool shadow;

  @override
  Widget build(BuildContext context) {
    final color = backgroundColor ?? context.colors.surfaceContainerLowest;
    return Material(
      color: color,
      elevation: shadow ? 3 : 0,
      shadowColor: context.colors.shadow.withValues(alpha: 0.16),
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(radius),
        side: BorderSide(color: borderColor ?? context.studioLine),
      ),
      clipBehavior: Clip.antiAlias,
      child: padding == null ? child : Padding(padding: padding!, child: child),
    );
  }
}
