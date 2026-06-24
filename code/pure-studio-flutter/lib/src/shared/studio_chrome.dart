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
            Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.labelSmall?.copyWith(color: foreground),
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

class StudioEmptyState extends StatelessWidget {
  const StudioEmptyState({
    required this.icon,
    required this.title,
    required this.message,
    this.maxWidth = 380,
    this.iconSize = 44,
    super.key,
  });

  final IconData icon;
  final String title;
  final String message;
  final double maxWidth;
  final double iconSize;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return ConstrainedBox(
      constraints: BoxConstraints(maxWidth: maxWidth),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          StudioIconBadge(icon: icon, size: iconSize),
          const SizedBox(height: 14),
          Text(
            title,
            style: Theme.of(context).textTheme.titleMedium?.copyWith(
              color: context.studioInk,
              fontWeight: FontWeight.w700,
            ),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 6),
          Text(
            message,
            style: Theme.of(
              context,
            ).textTheme.bodySmall?.copyWith(color: colors.onSurfaceVariant),
            textAlign: TextAlign.center,
          ),
        ],
      ),
    );
  }
}

class StudioInlineMessage extends StatelessWidget {
  const StudioInlineMessage({
    required this.icon,
    required this.message,
    this.color,
    super.key,
  });

  final IconData icon;
  final String message;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final foreground = color ?? Theme.of(context).colorScheme.onSurfaceVariant;
    return Row(
      children: [
        Icon(icon, size: 16, color: foreground),
        const SizedBox(width: 7),
        Expanded(
          child: Text(
            message,
            style: context.text.bodySmall?.copyWith(color: foreground),
          ),
        ),
      ],
    );
  }
}

enum StudioNoticeTone { neutral, info, warning, danger, success }

class StudioNotice extends StatelessWidget {
  const StudioNotice({
    required this.icon,
    required this.message,
    this.title,
    this.tone = StudioNoticeTone.neutral,
    this.padding = const EdgeInsets.all(12),
    this.iconSize,
    super.key,
  });

  final IconData icon;
  final String? title;
  final String message;
  final StudioNoticeTone tone;
  final EdgeInsetsGeometry padding;
  final double? iconSize;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final foreground = switch (tone) {
      StudioNoticeTone.danger => colors.error,
      StudioNoticeTone.warning => colors.tertiary,
      StudioNoticeTone.success => colors.secondary,
      StudioNoticeTone.info => colors.primary,
      StudioNoticeTone.neutral => colors.onSurfaceVariant,
    };
    final background = switch (tone) {
      StudioNoticeTone.danger => colors.errorContainer.withValues(alpha: 0.5),
      StudioNoticeTone.warning => colors.tertiaryContainer.withValues(
        alpha: 0.46,
      ),
      StudioNoticeTone.success => colors.secondaryContainer.withValues(
        alpha: 0.46,
      ),
      StudioNoticeTone.info => colors.primaryContainer.withValues(alpha: 0.32),
      StudioNoticeTone.neutral => colors.surfaceContainerLowest,
    };
    final border = switch (tone) {
      StudioNoticeTone.danger => colors.error.withValues(alpha: 0.35),
      StudioNoticeTone.warning => colors.tertiary.withValues(alpha: 0.22),
      StudioNoticeTone.success => colors.secondary.withValues(alpha: 0.22),
      StudioNoticeTone.info => colors.primary.withValues(alpha: 0.22),
      StudioNoticeTone.neutral => colors.outlineVariant,
    };
    return DecoratedBox(
      decoration: BoxDecoration(
        color: background,
        border: Border.all(color: border),
        borderRadius: BorderRadius.circular(StudioRadii.sm),
      ),
      child: Padding(
        padding: padding,
        child: Row(
          children: [
            Icon(icon, size: iconSize, color: foreground),
            const SizedBox(width: 10),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  if (title?.isNotEmpty ?? false) ...[
                    Text(
                      title!,
                      style: context.text.titleSmall?.copyWith(
                        color: context.studioInk,
                      ),
                    ),
                    const SizedBox(height: 3),
                  ],
                  Text(
                    message,
                    style: context.text.bodySmall?.copyWith(
                      color: tone == StudioNoticeTone.danger
                          ? colors.onErrorContainer
                          : foreground,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class StudioCodeBlock extends StatelessWidget {
  const StudioCodeBlock({
    required this.text,
    this.language,
    this.maxHeight,
    this.margin = EdgeInsets.zero,
    this.padding = const EdgeInsets.all(12),
    this.horizontalScroll = true,
    this.backgroundColor,
    this.headerBackgroundColor,
    this.borderColor,
    this.textStyle,
    this.languageTextStyle,
    super.key,
  });

  final String text;
  final String? language;
  final double? maxHeight;
  final EdgeInsetsGeometry margin;
  final EdgeInsetsGeometry padding;
  final bool horizontalScroll;
  final Color? backgroundColor;
  final Color? headerBackgroundColor;
  final Color? borderColor;
  final TextStyle? textStyle;
  final TextStyle? languageTextStyle;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final textTheme = Theme.of(context).textTheme;
    final languageLabel = language?.trim() ?? '';
    final codeTextStyle =
        textStyle ??
        textTheme.bodyMedium?.copyWith(
          color: colors.onSurface,
          fontFamily: 'JetBrains Mono',
          fontFamilyFallback: const ['Consolas', 'monospace'],
          fontSize: (textTheme.bodyMedium?.fontSize ?? 14) * 0.92,
          height: 1.35,
        );

    Widget codeBody = Padding(
      padding: padding,
      child: SelectableText(text, style: codeTextStyle),
    );
    if (horizontalScroll) {
      codeBody = SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: codeBody,
      );
    }
    if (maxHeight != null) {
      codeBody = ConstrainedBox(
        constraints: BoxConstraints(maxHeight: maxHeight!),
        child: SingleChildScrollView(child: codeBody),
      );
    }

    return Container(
      width: double.infinity,
      margin: margin,
      decoration: BoxDecoration(
        color: backgroundColor ?? colors.surfaceContainerLow,
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        border: Border.all(color: borderColor ?? colors.outlineVariant),
      ),
      clipBehavior: Clip.antiAlias,
      child: languageLabel.isEmpty
          ? codeBody
          : Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                DecoratedBox(
                  decoration: BoxDecoration(
                    color:
                        headerBackgroundColor ?? colors.surfaceContainerHighest,
                  ),
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(12, 7, 12, 6),
                    child: Text(
                      languageLabel,
                      style:
                          languageTextStyle ??
                          textTheme.labelSmall?.copyWith(
                            color: colors.onSurfaceVariant,
                            fontFamily: 'JetBrains Mono',
                            fontFamilyFallback: const ['Consolas', 'monospace'],
                          ),
                    ),
                  ),
                ),
                codeBody,
              ],
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
