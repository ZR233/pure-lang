import 'package:flutter/material.dart';

import '../app/theme/studio_tokens.dart';

class StudioPill extends StatelessWidget {
  const StudioPill({
    required this.label,
    this.icon,
    this.backgroundColor,
    this.foregroundColor,
    this.borderColor,
    super.key,
  });

  final String label;
  final IconData? icon;
  final Color? backgroundColor;
  final Color? foregroundColor;
  final Color? borderColor;

  @override
  Widget build(BuildContext context) {
    final foreground = foregroundColor ?? context.colors.onSurfaceVariant;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: backgroundColor ?? context.colors.surfaceContainerLow,
        border: Border.all(
          color:
              borderColor ??
              context.colors.outlineVariant.withValues(alpha: 0.72),
        ),
        borderRadius: BorderRadius.circular(StudioRadii.pill),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 4),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (icon != null) ...[
              Icon(icon, size: 14, color: foreground),
              const SizedBox(width: 5),
            ],
            Flexible(
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: context.text.labelSmall?.copyWith(color: foreground),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class StudioCompactChip extends StatelessWidget {
  const StudioCompactChip({
    required this.label,
    this.icon,
    this.trailingIcon,
    this.tooltip,
    this.enabled = true,
    this.maxWidth = 180,
    this.margin = EdgeInsets.zero,
    this.backgroundColor,
    this.foregroundColor,
    this.borderColor,
    super.key,
  });

  final String label;
  final IconData? icon;
  final IconData? trailingIcon;
  final String? tooltip;
  final bool enabled;
  final double maxWidth;
  final EdgeInsetsGeometry margin;
  final Color? backgroundColor;
  final Color? foregroundColor;
  final Color? borderColor;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final foreground = foregroundColor ?? colors.onSurfaceVariant;
    final chip = Padding(
      padding: margin,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color:
              backgroundColor ??
              (enabled
                  ? colors.surfaceContainerLowest.withValues(alpha: 0.78)
                  : colors.surfaceContainerHighest),
          border: Border.all(
            color: borderColor ?? colors.outlineVariant.withValues(alpha: 0.72),
          ),
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (icon != null) ...[
                Icon(icon, size: 15, color: foreground),
                const SizedBox(width: 5),
              ],
              ConstrainedBox(
                constraints: BoxConstraints(maxWidth: maxWidth),
                child: Text(
                  label,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.labelSmall?.copyWith(color: foreground),
                ),
              ),
              if (trailingIcon != null) ...[
                const SizedBox(width: 2),
                Icon(trailingIcon, size: 15, color: foreground),
              ],
            ],
          ),
        ),
      ),
    );
    final message = tooltip;
    if (message == null || message.isEmpty) {
      return chip;
    }
    return Tooltip(message: message, child: chip);
  }
}

class StudioIconBadge extends StatelessWidget {
  const StudioIconBadge({
    required this.icon,
    this.backgroundColor,
    this.foregroundColor,
    this.size = 32,
    super.key,
  });

  final IconData icon;
  final Color? backgroundColor;
  final Color? foregroundColor;
  final double size;

  @override
  Widget build(BuildContext context) {
    return SizedBox.square(
      dimension: size,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: backgroundColor ?? StudioColors.claySoft,
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          border: Border.all(color: context.studioLine),
        ),
        child: Icon(
          icon,
          size: size * 0.56,
          color: foregroundColor ?? StudioColors.clayDeep,
        ),
      ),
    );
  }
}

class StudioProgressDots extends StatelessWidget {
  const StudioProgressDots({
    required this.activeIndex,
    this.count = 3,
    super.key,
  });

  final int activeIndex;
  final int count;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (var index = 0; index < count; index++)
          Padding(
            padding: EdgeInsets.only(right: index == count - 1 ? 0 : 5),
            child: AnimatedContainer(
              duration: const Duration(milliseconds: 160),
              width: index == activeIndex ? 16 : 6,
              height: 6,
              decoration: BoxDecoration(
                color: index == activeIndex
                    ? StudioColors.clay
                    : context.colors.outlineVariant,
                borderRadius: BorderRadius.circular(StudioRadii.pill),
              ),
            ),
          ),
      ],
    );
  }
}
