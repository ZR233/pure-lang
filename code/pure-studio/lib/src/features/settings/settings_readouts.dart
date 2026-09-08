import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../shared/studio_chrome.dart';

class SettingsReadout extends StatelessWidget {
  const SettingsReadout({super.key, required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 7),
      child: Row(
        children: [
          SizedBox(
            width: 140,
            child: Text(
              label,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          ),
          Expanded(
            child: Text(value, maxLines: 1, overflow: TextOverflow.ellipsis),
          ),
        ],
      ),
    );
  }
}

class SettingsProviderStatusChip extends StatelessWidget {
  const SettingsProviderStatusChip({super.key, required this.provider});

  final ProviderSettingsView provider;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final ready = provider.status == 'ready';
    return StudioPill(
      icon: ready ? Icons.check_circle_outline : Icons.error_outline,
      label: ready ? 'ready' : 'setup',
      backgroundColor: ready
          ? colors.secondaryContainer.withValues(alpha: 0.42)
          : colors.tertiaryContainer.withValues(alpha: 0.38),
      foregroundColor: ready ? colors.secondary : colors.tertiary,
      borderColor: ready
          ? colors.secondary.withValues(alpha: 0.24)
          : colors.tertiary.withValues(alpha: 0.22),
    );
  }
}

class SettingsMiniMeta extends StatelessWidget {
  const SettingsMiniMeta({super.key, required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(icon, size: 13, color: context.studioInkSoft),
        const SizedBox(width: 5),
        Flexible(
          child: Text(
            label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.text.bodySmall?.copyWith(
              color: context.studioInkSoft,
            ),
          ),
        ),
      ],
    );
  }
}

class SettingsInfoPill extends StatelessWidget {
  const SettingsInfoPill({super.key, required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return StudioCompactChip(
      icon: icon,
      label: label,
      backgroundColor: context.studioPaper2,
      borderColor: context.studioLine,
      maxWidth: 220,
    );
  }
}

class SettingsMetric extends StatelessWidget {
  const SettingsMetric(this.label, this.value, {super.key});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(label, style: Theme.of(context).textTheme.labelSmall),
        Text(value, style: Theme.of(context).textTheme.bodyMedium),
      ],
    );
  }
}
