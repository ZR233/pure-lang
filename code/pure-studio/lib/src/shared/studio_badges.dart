import 'package:flutter/material.dart';

import '../app/theme/studio_tokens.dart';

class StudioPill extends StatelessWidget {
  const StudioPill({
    required this.label,
    this.icon,
    this.tone = StudioTone.neutral,
    super.key,
  });

  final String label;
  final IconData? icon;
  final StudioTone tone;

  @override
  Widget build(BuildContext context) {
    final foreground = tone.foreground(context);
    return DecoratedBox(
      decoration: BoxDecoration(
        color: tone.background(context),
        border: Border.all(color: context.colors.outlineVariant),
        borderRadius: BorderRadius.circular(StudioRadii.pill),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 4),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (icon != null) ...[
              Icon(icon, size: 14, color: tone.indicator(context)),
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
    this.tone = StudioTone.neutral,
    super.key,
  });

  final String label;
  final IconData? icon;
  final IconData? trailingIcon;
  final String? tooltip;
  final bool enabled;
  final double maxWidth;
  final EdgeInsetsGeometry margin;
  final StudioTone tone;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final foreground = tone.foreground(context);
    final chip = Padding(
      padding: margin,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: (enabled
              ? tone.background(context)
              : colors.surfaceContainerHighest),
          border: Border.all(color: colors.outlineVariant),
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (icon != null) ...[
                Icon(icon, size: 15, color: tone.indicator(context)),
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
    this.tone = StudioTone.brand,
    this.filled = false,
    this.size = 32,
    super.key,
  });

  final IconData icon;
  final StudioTone tone;
  final bool filled;
  final double size;

  @override
  Widget build(BuildContext context) {
    return SizedBox.square(
      dimension: size,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: filled ? context.colors.primary : tone.background(context),
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          border: Border.all(color: context.colors.outlineVariant),
        ),
        child: Icon(
          icon,
          size: size * 0.56,
          color: filled ? context.colors.onPrimary : tone.indicator(context),
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
                    ? context.statusColors.activeIndicator
                    : context.colors.outlineVariant,
                borderRadius: BorderRadius.circular(StudioRadii.pill),
              ),
            ),
          ),
      ],
    );
  }
}

/// A supplementary eye-blue marker; the selected surface and label carry state too.
class StudioSelectionMarker extends StatelessWidget {
  const StudioSelectionMarker({
    required this.selected,
    required this.child,
    super.key,
  });
  final bool selected;
  final Widget child;

  @override
  Widget build(BuildContext context) => Container(
    foregroundDecoration: BoxDecoration(
      border: BorderDirectional(
        start: BorderSide(
          color: selected ? context.statusColors.eyeAccent : Colors.transparent,
          width: 2,
        ),
      ),
    ),
    child: child,
  );
}
