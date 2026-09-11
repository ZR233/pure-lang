import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';

/// A resource's identity, state and actions stay visible while its details scroll.
/// Callers own all commands and canonical data; this widget only arranges content.
class SettingsResourceRow extends StatelessWidget {
  const SettingsResourceRow({
    required this.title,
    this.subtitle,
    this.icon,
    this.status,
    this.actions = const [],
    this.children = const [],
    super.key,
  });

  final String title;
  final String? subtitle;
  final IconData? icon;
  final Widget? status;
  final List<Widget> actions;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          LayoutBuilder(
            builder: (context, constraints) {
              final identity = Row(
                children: [
                  if (icon != null) ...[
                    Icon(
                      icon,
                      size: 20,
                      color: context.colors.onSurfaceVariant,
                    ),
                    const SizedBox(width: 12),
                  ],
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          title,
                          style: context.text.titleSmall?.copyWith(
                            fontWeight: FontWeight.w600,
                          ),
                        ),
                        if (subtitle?.isNotEmpty ?? false) ...[
                          const SizedBox(height: 4),
                          Text(
                            subtitle!,
                            style: context.text.bodySmall?.copyWith(
                              color: context.colors.onSurfaceVariant,
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                ],
              );
              final controls = Wrap(
                spacing: 8,
                runSpacing: 6,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [?status, ...actions],
              );
              if (constraints.maxWidth < 650) {
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    identity,
                    if (status != null || actions.isNotEmpty) ...[
                      const SizedBox(height: 10),
                      controls,
                    ],
                  ],
                );
              }
              return Row(
                children: [
                  Expanded(child: identity),
                  if (status != null || actions.isNotEmpty) ...[
                    const SizedBox(width: 16),
                    ConstrainedBox(
                      constraints: BoxConstraints(
                        maxWidth: constraints.maxWidth * 0.5,
                      ),
                      child: controls,
                    ),
                  ],
                ],
              );
            },
          ),
          if (children.isNotEmpty) ...[const SizedBox(height: 12), ...children],
        ],
      ),
    );
  }
}
