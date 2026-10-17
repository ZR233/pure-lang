import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/provider_brand.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';
import 'settings_provider_model_readout.dart';
import 'settings_provider_usage.dart';
import 'settings_model_catalog.dart';
import 'settings_provider_auto_compact.dart';

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
    required this.autoCompactEdits,
    required this.onAutoCompactChanged,
    required this.onAutoCompactCommit,
    required this.onAutoCompactInvalid,
    required this.onRemoveManualModel,
  });

  final ProviderSettingsView? provider;
  final ProviderUsageView? usage;
  final bool usageLoading;
  final String? usageError;
  final VoidCallback onBack;
  final ValueChanged<ProviderSettingsView> onEdit;
  final VoidCallback? onRefreshUsage;
  final Map<String, ModelAutoCompactEdit> autoCompactEdits;
  final void Function(String slug, int? value) onAutoCompactChanged;
  final ValueChanged<String> onAutoCompactCommit;
  final ValueChanged<String> onAutoCompactInvalid;
  final ValueChanged<ProviderModelView> onRemoveManualModel;

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
          leading: ProviderBrandMark(
            iconKey: provider.iconKey,
            fallbackName: provider.name,
          ),
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
            SettingsProviderStatusChip(provider: provider, usage: usage),
            SettingsInfoPill(
              icon: Icons.hub_outlined,
              label: provider.models
                  .map(
                    (model) => context.modelProtocolLabel(model.wireProtocol),
                  )
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
            if (provider.modelCatalog.supported)
              ModelCatalogReadout(status: provider.modelCatalog),
            if (provider.models.isEmpty)
              Text(context.l10n.settingsModelCatalogEmpty)
            else ...[
              _originSection(
                context,
                title: context.l10n.settingsProviderOnlineModelsTitle,
                emptyLabel: context.l10n.settingsNoOnlineModels,
                models: provider.onlineModels,
                provider: provider,
              ),
              _originSection(
                context,
                title: context.l10n.settingsProviderDefaultModelsTitle,
                emptyLabel: context.l10n.settingsNoBundledModels,
                models: provider.bundledModels,
                provider: provider,
              ),
              _originSection(
                context,
                title: context.l10n.settingsProviderCustomModelsTitle,
                emptyLabel: context.l10n.settingsNoCustomModels,
                models: provider.customModels,
                provider: provider,
                onRemove: onRemoveManualModel,
              ),
            ],
          ],
        ),
      ],
    );
  }

  Widget _originSection(
    BuildContext context, {
    required String title,
    required String emptyLabel,
    required List<ProviderModelView> models,
    required ProviderSettingsView provider,
    ValueChanged<ProviderModelView>? onRemove,
  }) {
    return Padding(
      padding: const EdgeInsets.only(top: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(title, style: Theme.of(context).textTheme.titleSmall),
          const SizedBox(height: 8),
          if (models.isEmpty)
            Text(emptyLabel)
          else
            for (final model in models)
              ProviderModelReadout(
                key: ValueKey((provider.id, model.slug)),
                model: model,
                providerId: provider.id,
                framed: true,
                autoCompact: provider.autoCompactLimitFor(model),
                autoCompactEdit: autoCompactEdits[model.slug],
                onAutoCompactChanged: (value) =>
                    onAutoCompactChanged(model.slug, value),
                onAutoCompactCommit: () => onAutoCompactCommit(model.slug),
                onAutoCompactInvalid: () => onAutoCompactInvalid(model.slug),
                onDelete: onRemove == null ? null : () => onRemove(model),
              ),
        ],
      ),
    );
  }
}
