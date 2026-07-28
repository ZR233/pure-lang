import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../shared/studio_chrome.dart';

class InteractionDockShell extends StatelessWidget {
  const InteractionDockShell({
    required this.kind,
    required this.title,
    required this.child,
    this.subtitle,
    this.footer,
    this.footerHint,
    this.trailing,
    super.key,
  });

  final InteractionDockKind kind;
  final String title;
  final String? subtitle;
  final Widget child;
  final Widget? footer;
  final String? footerHint;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 460;
        final bottomPadding = footer == null ? (compact ? 12.0 : 16.0) : 0.0;
        return StudioPanel(
          backgroundColor: colors.surfaceContainerLowest,
          borderColor: colors.outlineVariant.withValues(alpha: 0.86),
          radius: StudioRadii.lg,
          shadow: true,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Padding(
                padding: EdgeInsets.fromLTRB(
                  compact ? 12 : 18,
                  compact ? 12 : 16,
                  compact ? 12 : 18,
                  bottomPadding,
                ),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    _DockHeader(
                      kind: kind,
                      title: title,
                      subtitle: subtitle,
                      trailing: trailing,
                    ),
                    const SizedBox(height: 13),
                    child,
                  ],
                ),
              ),
              if (footer != null) _DockFooter(hint: footerHint, child: footer!),
            ],
          ),
        );
      },
    );
  }
}

class _DockHeader extends StatelessWidget {
  const _DockHeader({
    required this.kind,
    required this.title,
    required this.subtitle,
    required this.trailing,
  });

  final InteractionDockKind kind;
  final String title;
  final String? subtitle;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        StudioIconBadge(
          icon: _iconFor(kind),
          size: 34,
          backgroundColor: _badgeBackgroundFor(kind),
          foregroundColor: Colors.white,
        ),
        const SizedBox(width: 11),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                title,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.titleSmall?.copyWith(
                  color: context.studioInk,
                  fontWeight: FontWeight.w600,
                ),
              ),
              if (subtitle?.trim().isNotEmpty ?? false)
                Text(
                  subtitle!.trim(),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.labelSmall?.copyWith(
                    color: context.studioInkSoft.withValues(alpha: 0.68),
                  ),
                ),
            ],
          ),
        ),
        if (trailing != null) ...[const SizedBox(width: 8), trailing!],
      ],
    );
  }
}

class _DockFooter extends StatelessWidget {
  const _DockFooter({required this.child, this.hint});

  final Widget child;
  final String? hint;

  @override
  Widget build(BuildContext context) {
    final hintText = hint?.trim();
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.colors.surfaceContainerLow,
        border: Border(top: BorderSide(color: context.studioLine)),
      ),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final compact = constraints.maxWidth < 560;
          final hintWidget = (hintText?.isNotEmpty ?? false)
              ? Text(
                  hintText!,
                  maxLines: compact ? 3 : 2,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.labelSmall?.copyWith(
                    color: context.studioInkSoft.withValues(alpha: 0.66),
                  ),
                )
              : null;
          return Padding(
            padding: const EdgeInsets.fromLTRB(18, 12, 18, 12),
            child: compact
                ? Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (hintWidget != null) ...[
                        hintWidget,
                        const SizedBox(height: 10),
                      ],
                      Align(alignment: Alignment.centerRight, child: child),
                    ],
                  )
                : Row(
                    children: [
                      if (hintWidget != null) ...[
                        Expanded(child: hintWidget),
                        const SizedBox(width: 12),
                      ] else
                        const Spacer(),
                      Flexible(child: child),
                    ],
                  ),
          );
        },
      ),
    );
  }
}

enum InteractionDockKind { question, permission, plan }

class DockActions extends StatelessWidget {
  const DockActions({required this.children, super.key});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Wrap(
      alignment: WrapAlignment.end,
      spacing: 8,
      runSpacing: 8,
      children: children,
    );
  }
}

class InfoChip extends StatelessWidget {
  const InfoChip({required this.icon, required this.label, super.key});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return StudioPill(
      icon: icon,
      label: label,
      backgroundColor: Theme.of(context).colorScheme.surfaceContainerLow,
    );
  }
}

class InteractionCodeBlock extends StatelessWidget {
  const InteractionCodeBlock({required this.text, super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return StudioCodeBlock(
      text: text,
      maxHeight: 180,
      padding: const EdgeInsets.all(10),
      horizontalScroll: false,
      backgroundColor: colors.surfaceContainerLow,
      borderColor: colors.outlineVariant,
      textStyle: Theme.of(context).textTheme.bodySmall?.copyWith(
        fontFamily: 'JetBrains Mono',
        fontFamilyFallback: const ['Consolas', 'monospace'],
      ),
    );
  }
}

class DockOptionRow extends StatelessWidget {
  const DockOptionRow({
    required this.title,
    required this.selected,
    required this.onPressed,
    this.subtitle,
    this.leading,
    super.key,
  });

  final String title;
  final String? subtitle;
  final bool selected;
  final Widget? leading;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Material(
      color: selected ? StudioColors.claySoft : colors.surfaceContainerLowest,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        side: BorderSide(
          color: selected ? StudioColors.clay : context.studioLine,
        ),
      ),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onPressed,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(10, 8, 12, 8),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (leading != null) ...[leading!, const SizedBox(width: 10)],
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(title, style: Theme.of(context).textTheme.labelLarge),
                    if (subtitle?.isNotEmpty ?? false) ...[
                      const SizedBox(height: 2),
                      Text(
                        subtitle!,
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: context.studioInkSoft,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

IconData _iconFor(InteractionDockKind kind) {
  return switch (kind) {
    InteractionDockKind.question => Icons.help_outline,
    InteractionDockKind.permission => Icons.admin_panel_settings_outlined,
    InteractionDockKind.plan => Icons.task_alt_outlined,
  };
}

Color _badgeBackgroundFor(InteractionDockKind kind) {
  return switch (kind) {
    InteractionDockKind.question => StudioColors.sage,
    InteractionDockKind.permission => StudioColors.clay,
    InteractionDockKind.plan => StudioColors.clay,
  };
}
