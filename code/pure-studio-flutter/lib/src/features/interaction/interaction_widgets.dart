import 'package:flutter/material.dart';

class InteractionDockShell extends StatelessWidget {
  const InteractionDockShell({
    required this.kind,
    required this.header,
    required this.child,
    this.footer,
    this.trailing,
    super.key,
  });

  final InteractionDockKind kind;
  final Widget header;
  final Widget child;
  final Widget? footer;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 460;
        return DecoratedBox(
          decoration: BoxDecoration(
            color: colors.surface,
            border: Border.all(
              color: colors.outlineVariant.withValues(alpha: 0.9),
            ),
            borderRadius: BorderRadius.circular(8),
            boxShadow: [
              BoxShadow(
                color: colors.shadow.withValues(alpha: 0.07),
                blurRadius: 18,
                offset: const Offset(0, 8),
              ),
            ],
          ),
          child: Padding(
            padding: EdgeInsets.fromLTRB(compact ? 10 : 14, 12, 12, 12),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (!compact) ...[
                  Icon(
                    _iconFor(kind),
                    size: 20,
                    color: colors.onSurfaceVariant,
                  ),
                  const SizedBox(width: 12),
                ],
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      DefaultTextStyle.merge(
                        style: Theme.of(context).textTheme.titleSmall,
                        child: header,
                      ),
                      const SizedBox(height: 10),
                      child,
                      if (footer != null) ...[
                        const SizedBox(height: 12),
                        footer!,
                      ],
                    ],
                  ),
                ),
                if (trailing != null) ...[
                  SizedBox(width: compact ? 6 : 8),
                  trailing!,
                ],
              ],
            ),
          ),
        );
      },
    );
  }

  IconData _iconFor(InteractionDockKind kind) {
    return switch (kind) {
      InteractionDockKind.question => Icons.help_outline,
      InteractionDockKind.permission => Icons.admin_panel_settings_outlined,
      InteractionDockKind.plan => Icons.route_outlined,
    };
  }
}

enum InteractionDockKind { question, permission, plan }

class DockTitle extends StatelessWidget {
  const DockTitle({
    required this.icon,
    required this.label,
    this.trailing,
    super.key,
  });

  final IconData icon;
  final String label;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Icon(icon, size: 17),
        const SizedBox(width: 7),
        Expanded(child: Text(label, overflow: TextOverflow.ellipsis)),
        if (trailing != null) ...[const SizedBox(width: 8), trailing!],
      ],
    );
  }
}

class DockActions extends StatelessWidget {
  const DockActions({required this.children, super.key});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: double.infinity,
      child: Wrap(
        alignment: WrapAlignment.end,
        spacing: 8,
        runSpacing: 8,
        children: children,
      ),
    );
  }
}

class InfoChip extends StatelessWidget {
  const InfoChip({required this.icon, required this.label, super.key});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return Chip(
      avatar: Icon(icon, size: 18),
      label: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: Text(label, overflow: TextOverflow.ellipsis),
      ),
    );
  }
}

class InteractionCodeBlock extends StatelessWidget {
  const InteractionCodeBlock({required this.text, super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Container(
      width: double.infinity,
      constraints: const BoxConstraints(maxHeight: 180),
      decoration: BoxDecoration(
        color: colors.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      padding: const EdgeInsets.all(10),
      child: SingleChildScrollView(
        child: SelectableText(
          text,
          style: Theme.of(
            context,
          ).textTheme.bodySmall?.copyWith(fontFamily: 'monospace'),
        ),
      ),
    );
  }
}
