import 'package:flutter/material.dart';

import '../app/theme/studio_tokens.dart';
import 'studio_badges.dart';

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
              color: context.colors.onSurface,
              fontWeight: FontWeight.w700,
            ),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 6),
          Text(
            message,
            style: Theme.of(context).textTheme.bodySmall
                ?.copyWith(color: colors.onSurfaceVariant),
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
    final intent = switch (tone) {
      StudioNoticeTone.neutral => StudioTone.neutral,
      StudioNoticeTone.info => StudioTone.brand,
      StudioNoticeTone.warning => StudioTone.warning,
      StudioNoticeTone.danger => StudioTone.error,
      StudioNoticeTone.success => StudioTone.success,
    };
    final foreground = intent.foreground(context);
    final background = intent.background(context);
    final border = colors.outlineVariant;
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
                        color: context.colors.onSurface,
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
