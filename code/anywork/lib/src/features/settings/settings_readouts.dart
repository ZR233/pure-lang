import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
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
  const SettingsProviderStatusChip({
    super.key,
    required this.provider,
    this.usage,
  });

  final ProviderSettingsView provider;
  final ProviderUsageView? usage;

  @override
  Widget build(BuildContext context) {
    if (usage?.state is FailedProviderUsageView) {
      return StudioPill(
        icon: Icons.error_outline,
        label: context.l10n.settingsUsageFailed,
        tone: StudioTone.error,
      );
    }
    final configured = provider.status == 'ready';
    return StudioPill(
      icon: configured ? Icons.key_outlined : Icons.error_outline,
      label: context.providerStatusLabel(provider.status),
      tone: configured ? StudioTone.neutral : StudioTone.warning,
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
        Icon(icon, size: 13, color: context.colors.onSurfaceVariant),
        const SizedBox(width: 5),
        Flexible(
          child: Text(
            label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.text.bodySmall?.copyWith(
              color: context.colors.onSurfaceVariant,
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
    return StudioCompactChip(icon: icon, label: label, maxWidth: 220);
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
