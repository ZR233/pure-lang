import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';
import 'settings_model_labels.dart';
import 'settings_provider_model_readout.dart';
import 'settings_provider_usage.dart';

class ProviderDetails extends StatelessWidget {
  const ProviderDetails({
    super.key,
    required this.provider,
    required this.usage,
    required this.usageLoading,
    required this.usageError,
    required this.onBack,
    required this.onEdit,
    required this.onRefreshUsage,
  });

  final ProviderSettingsView? provider;
  final ProviderUsageView? usage;
  final bool usageLoading;
  final String? usageError;
  final VoidCallback onBack;
  final ValueChanged<ProviderSettingsView> onEdit;
  final VoidCallback? onRefreshUsage;

  @override
  Widget build(BuildContext context) {
    final provider = this.provider;
    if (provider == null) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          TextButton.icon(
            icon: const Icon(Icons.arrow_back),
            label: Text(context.l10n.settingsProvidersTitle),
            onPressed: onBack,
          ),
          Expanded(
            child: Center(child: Text(context.l10n.settingsNoProviderSelected)),
          ),
        ],
      );
    }
    return ListView(
      key: StudioDriverKeys.providerEditor,
      children: [
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton.icon(
            icon: const Icon(Icons.arrow_back),
            label: Text(context.l10n.settingsProvidersTitle),
            onPressed: onBack,
          ),
        ),
        const SizedBox(height: 4),
        SettingsHeader(
          title: provider.name,
          subtitle: provider.baseUrl,
          trailing: FilledButton.tonalIcon(
            key: StudioDriverKeys.providerEdit,
            icon: const Icon(Icons.edit_outlined),
            label: Text(context.l10n.settingsEdit),
            onPressed: () => onEdit(provider),
          ),
        ),
        const SizedBox(height: 12),
        ProviderUsagePanel(
          provider: provider,
          usage: usage,
          loading: usageLoading,
          error: usageError,
          onRefresh: onRefreshUsage ?? () {},
        ),
        const SizedBox(height: 12),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            SettingsInfoPill(icon: Icons.key_outlined, label: provider.status),
            SettingsInfoPill(
              icon: Icons.hub_outlined,
              label: provider.allModels
                  .map((model) => modelProtocolLabel(model.wireProtocol))
                  .toSet()
                  .join(' / '),
            ),
            SettingsInfoPill(
              icon: Icons.memory_outlined,
              label: provider.modelCount,
            ),
          ],
        ),
        const SizedBox(height: 16),
        SettingsSectionPanel(
          title: context.l10n.settingsProviderTitle,
          children: [
            SettingsReadout(
              label: context.l10n.settingsProviderKey,
              value: provider.id,
            ),
            SettingsReadout(
              label: context.l10n.settingsTemplate,
              value: provider.templateKind.isEmpty
                  ? context.l10n.settingsCustomProvider
                  : provider.templateKind,
            ),
            SettingsReadout(
              label: context.l10n.settingsDefaultModel,
              value: provider.defaultModel,
            ),
            SettingsReadout(
              label: context.l10n.settingsApiKey,
              value: provider.hasBearerToken
                  ? context.l10n.settingsConfigured
                  : context.l10n.settingsMissing,
            ),
          ],
        ),
        const SizedBox(height: 12),
        SettingsSectionPanel(
          title: context.l10n.settingsProviderModelsTitle,
          children: [
            for (final model in provider.allModels)
              ProviderModelReadout(
                model: model,
                providerId: provider.id,
                framed: true,
              ),
          ],
        ),
      ],
    );
  }
}
