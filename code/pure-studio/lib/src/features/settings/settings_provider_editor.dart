import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';
import 'settings_provider_drafts.dart';
import 'settings_provider_model_readout.dart';

class ProviderEditor extends StatelessWidget {
  const ProviderEditor({
    super.key,
    required this.draft,
    required this.presets,
    required this.saving,
    required this.error,
    required this.onCancel,
    required this.onSave,
    required this.onChangeTemplate,
    required this.onUpdate,
    required this.onAddCustomModel,
    required this.onUpdateCustomModel,
    required this.onRemoveCustomModel,
  });

  final ProviderDraft draft;
  final List<ProviderPresetView> presets;
  final bool saving;
  final String? error;
  final VoidCallback onCancel;
  final VoidCallback onSave;
  final ValueChanged<String> onChangeTemplate;
  final ValueChanged<ProviderSettingsView Function(ProviderSettingsView)>
  onUpdate;
  final VoidCallback onAddCustomModel;
  final void Function(int index, ProviderModelView model) onUpdateCustomModel;
  final ValueChanged<int> onRemoveCustomModel;

  @override
  Widget build(BuildContext context) {
    final provider = draft.provider;
    final models = provider.allModels;
    return Column(
      children: [
        Expanded(
          child: ListView(
            key: StudioDriverKeys.providerEditorScroll,
            children: [
              SettingsHeader(
                title: draft.mode == ProviderDraftMode.create
                    ? context.l10n.settingsNewProvider
                    : provider.name,
                subtitle: provider.baseUrl,
              ),
              if (error != null) ...[
                const SizedBox(height: 12),
                SettingsInlineError(message: error!),
              ],
              const SizedBox(height: 12),
              SettingsSectionPanel(
                title: context.l10n.settingsProviderConnectionTitle,
                children: [
                  SettingsResponsiveFieldGrid(
                    children: [
                      DropdownButtonFormField<String>(
                        key: StudioDriverKeys.providerPreset,
                        initialValue: provider.templateKind,
                        decoration: InputDecoration(
                          labelText: context.l10n.settingsTemplate,
                        ),
                        items: [
                          for (final template in presets)
                            DropdownMenuItem(
                              value: template.id,
                              child: Text(template.displayName),
                            ),
                        ],
                        onChanged: saving
                            ? null
                            : (value) {
                                if (value != null) {
                                  onChangeTemplate(value);
                                }
                              },
                      ),
                      SettingsTextEdit(
                        label: context.l10n.settingsDisplayName,
                        value: provider.name,
                        enabled: !saving,
                        onChanged: (value) =>
                            onUpdate((item) => item.copyWith(name: value)),
                      ),
                    ],
                  ),
                  const SizedBox(height: 12),
                  SettingsTextEdit(
                    key: StudioDriverKeys.providerBaseUrl,
                    label: context.l10n.settingsBaseUrl,
                    value: provider.baseUrl,
                    enabled: !saving,
                    onChanged: (value) =>
                        onUpdate((item) => item.copyWith(baseUrl: value)),
                  ),
                  const SizedBox(height: 10),
                  SettingsTextEdit(
                    key: StudioDriverKeys.providerApiKey,
                    label: provider.hasBearerToken
                        ? context.l10n.settingsApiKeyKeepCurrent
                        : provider.credentialLabel,
                    value: provider.bearerToken,
                    enabled: !saving,
                    obscureText: true,
                    onChanged: (value) =>
                        onUpdate((item) => item.copyWith(bearerToken: value)),
                  ),
                  if (provider.credentialEnv.isNotEmpty) ...[
                    const SizedBox(height: 6),
                    Text(
                      provider.credentialEnv,
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                  const SizedBox(height: 10),
                  DropdownButtonFormField<String>(
                    isExpanded: true,
                    initialValue:
                        models.any(
                          (model) => model.slug == provider.defaultModel,
                        )
                        ? provider.defaultModel
                        : models.firstOrNull?.slug,
                    decoration: InputDecoration(
                      labelText: context.l10n.settingsDefaultModel,
                    ),
                    items: [
                      for (final model in models)
                        DropdownMenuItem(
                          value: model.slug,
                          child: Text(
                            '${model.displayName} (${model.slug})',
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                    ],
                    onChanged: saving
                        ? null
                        : (value) {
                            if (value != null) {
                              onUpdate(
                                (item) => item.copyWith(defaultModel: value),
                              );
                            }
                          },
                  ),
                ],
              ),
              const SizedBox(height: 12),
              SwitchListTile(
                key: StudioDriverKeys.providerPricing,
                title: Text(context.l10n.settingsPricingEnabled),
                subtitle: Text(context.l10n.settingsPricingHelp),
                value: provider.pricingEnabled,
                onChanged: saving
                    ? null
                    : (enabled) => onUpdate(
                        (item) => item.copyWith(pricingEnabled: enabled),
                      ),
              ),
              const SizedBox(height: 12),
              SettingsSectionPanel(
                title: context.l10n.settingsProviderDefaultModelsTitle,
                trailing: Text(
                  context.l10n.settingsBundledModels(
                    provider.defaultModels.length,
                  ),
                ),
                children: [
                  for (final model in provider.defaultModels)
                    ProviderModelReadout(
                      model: model,
                      providerId: provider.id,
                      onConnectionModeChanged:
                          saving || model.supportedConnectionModes.length <= 1
                          ? null
                          : (mode) => onUpdate(
                              (item) =>
                                  item.withModelConnection(model.slug, mode),
                            ),
                    ),
                ],
              ),
              const SizedBox(height: 12),
              SettingsSectionPanel(
                title: context.l10n.settingsProviderCustomModelsTitle,
                trailing: OutlinedButton.icon(
                  key: StudioDriverKeys.providerModelAdd,
                  icon: const Icon(Icons.add),
                  label: Text(context.l10n.settingsAddModel),
                  onPressed: saving ? null : onAddCustomModel,
                ),
                children: [
                  if (provider.customModels.isEmpty)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 8),
                      child: Text(context.l10n.settingsNoCustomModels),
                    )
                  else
                    for (
                      var index = 0;
                      index < provider.customModels.length;
                      index++
                    )
                      _CustomModelEditor(
                        index: index,
                        model: provider.customModels[index],
                        enabled: !saving,
                        onChanged: (model) => onUpdateCustomModel(index, model),
                        onRemove: () => onRemoveCustomModel(index),
                      ),
                ],
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        Padding(
          padding: const EdgeInsets.only(top: 12),
          child: Align(
            alignment: Alignment.centerRight,
            child: Wrap(
              spacing: 8,
              children: [
                OutlinedButton.icon(
                  key: StudioDriverKeys.providerCancel,
                  icon: const Icon(Icons.close),
                  label: Text(context.l10n.settingsCancel),
                  onPressed: saving ? null : onCancel,
                ),
                FilledButton.icon(
                  key: StudioDriverKeys.providerSave,
                  icon: const Icon(Icons.save_outlined),
                  label: Text(context.l10n.settingsSave),
                  onPressed: saving ? null : onSave,
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

class _CustomModelEditor extends StatelessWidget {
  const _CustomModelEditor({
    required this.index,
    required this.model,
    required this.enabled,
    required this.onChanged,
    required this.onRemove,
  });
  final int index;
  final ProviderModelView model;
  final bool enabled;
  final ValueChanged<ProviderModelView> onChanged;
  final VoidCallback onRemove;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Column(
        children: [
          Row(
            children: [
              Expanded(
                child: SettingsTextEdit(
                  key: StudioDriverKeys.customModelId(index),
                  label: context.l10n.settingsModelSlug,
                  value: model.slug,
                  enabled: enabled,
                  onChanged: (value) => onChanged(
                    model.copyWith(
                      slug: value,
                      displayName:
                          model.displayName == model.slug ||
                              model.displayName == 'Custom model'
                          ? value
                          : model.displayName,
                    ),
                  ),
                ),
              ),
              IconButton(
                tooltip: context.l10n.settingsRemoveModel,
                icon: const Icon(Icons.delete_outline),
                onPressed: enabled ? onRemove : null,
              ),
            ],
          ),
          ExpansionTile(
            title: Text(context.l10n.settingsModelAdvanced),
            children: [
              SettingsTextEdit(
                label: context.l10n.settingsDisplayName,
                value: model.displayName,
                enabled: enabled,
                onChanged: (value) =>
                    onChanged(model.copyWith(displayName: value)),
              ),
              DropdownButtonFormField<String>(
                initialValue: model.wireProtocol,
                decoration: InputDecoration(
                  labelText: context.l10n.settingsProtocolType,
                ),
                items: const [
                  DropdownMenuItem(
                    value: 'chat_completions',
                    child: Text('Chat Completions (HTTP)'),
                  ),
                  DropdownMenuItem(
                    value: 'responses',
                    child: Text('Responses (HTTP)'),
                  ),
                ],
                onChanged: enabled
                    ? (value) {
                        if (value != null) {
                          onChanged(
                            model.copyWith(
                              wireProtocol: value,
                              supportedConnectionModes: const ['http'],
                              defaultConnectionMode: 'http',
                              connectionMode: 'http',
                            ),
                          );
                        }
                      }
                    : null,
              ),
              SettingsTextEdit(
                label: context.l10n.settingsContextBudget,
                value: '${model.contextWindow ?? 32000}',
                enabled: enabled,
                onChanged: (value) {
                  final count = int.tryParse(value);
                  if (count != null) {
                    onChanged(model.copyWith(contextWindow: count));
                  }
                },
              ),
              SettingsTextEdit(
                label: context.l10n.settingsOutputBudget,
                value: '${model.maxOutputTokens ?? 4096}',
                enabled: enabled,
                onChanged: (value) {
                  final count = int.tryParse(value);
                  if (count != null) {
                    onChanged(model.copyWith(maxOutputTokens: count));
                  }
                },
              ),
            ],
          ),
        ],
      ),
    );
  }
}
