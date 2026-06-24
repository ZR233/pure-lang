part of 'settings_page.dart';

class _ProviderDetails extends StatelessWidget {
  const _ProviderDetails({
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
        _SettingsHeader(
          title: provider.name,
          subtitle: provider.baseUrl,
          trailing: FilledButton.tonalIcon(
            icon: const Icon(Icons.edit_outlined),
            label: Text(context.l10n.settingsEdit),
            onPressed: () => onEdit(provider),
          ),
        ),
        const SizedBox(height: 12),
        _ProviderUsagePanel(
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
            _InfoPill(icon: Icons.key_outlined, label: provider.status),
            _InfoPill(icon: Icons.hub_outlined, label: provider.providerKind),
            _InfoPill(icon: Icons.memory_outlined, label: provider.modelCount),
          ],
        ),
        const SizedBox(height: 16),
        _SectionPanel(
          title: context.l10n.settingsProviderTitle,
          children: [
            _Readout(
              label: context.l10n.settingsProviderKey,
              value: provider.id,
            ),
            _Readout(
              label: context.l10n.settingsTemplate,
              value: provider.templateKind,
            ),
            _Readout(
              label: context.l10n.settingsDefaultModel,
              value: provider.defaultModel,
            ),
            _Readout(
              label: context.l10n.settingsApiKey,
              value: provider.hasBearerToken
                  ? context.l10n.settingsConfigured
                  : context.l10n.settingsMissing,
            ),
          ],
        ),
        const SizedBox(height: 12),
        _SectionPanel(
          title: context.l10n.settingsProviderModelsTitle,
          children: [
            for (final model in provider.allModels)
              _ModelReadout(model: model, framed: true),
          ],
        ),
      ],
    );
  }
}

class _ProviderEditor extends StatelessWidget {
  const _ProviderEditor({
    required this.draft,
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

  final _ProviderDraft draft;
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
    return ListView(
      children: [
        _SettingsHeader(
          title: draft.mode == _ProviderDraftMode.create
              ? context.l10n.settingsNewProvider
              : provider.name,
          subtitle: provider.baseUrl,
          trailing: Wrap(
            spacing: 8,
            children: [
              OutlinedButton.icon(
                icon: const Icon(Icons.close),
                label: Text(context.l10n.settingsCancel),
                onPressed: saving ? null : onCancel,
              ),
              FilledButton.icon(
                icon: const Icon(Icons.save_outlined),
                label: Text(context.l10n.settingsSave),
                onPressed: saving ? null : onSave,
              ),
            ],
          ),
        ),
        if (error != null) ...[
          const SizedBox(height: 12),
          _InlineError(message: error!),
        ],
        const SizedBox(height: 12),
        _SectionPanel(
          title: context.l10n.settingsProviderConnectionTitle,
          children: [
            _ResponsiveFieldGrid(
              children: [
                _TextEdit(
                  label: context.l10n.settingsProviderKey,
                  value: provider.id,
                  enabled: !saving,
                  onChanged: (value) =>
                      onUpdate((item) => item.copyWith(id: value)),
                ),
                DropdownButtonFormField<String>(
                  initialValue: provider.templateKind,
                  decoration: InputDecoration(
                    labelText: context.l10n.settingsTemplate,
                  ),
                  items: [
                    for (final template in _providerTemplates)
                      DropdownMenuItem(
                        value: template.id,
                        child: Text(template.name),
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
                _TextEdit(
                  label: context.l10n.settingsDisplayName,
                  value: provider.name,
                  enabled: !saving,
                  onChanged: (value) =>
                      onUpdate((item) => item.copyWith(name: value)),
                ),
                _ReadonlyField(
                  label: context.l10n.settingsProtocolType,
                  value: provider.providerKind,
                ),
              ],
            ),
            const SizedBox(height: 10),
            _TextEdit(
              label: context.l10n.settingsBaseUrl,
              value: provider.baseUrl,
              enabled: !saving,
              onChanged: (value) =>
                  onUpdate((item) => item.copyWith(baseUrl: value)),
            ),
            const SizedBox(height: 10),
            _TextEdit(
              label: provider.hasBearerToken
                  ? context.l10n.settingsApiKeyKeepCurrent
                  : context.l10n.settingsApiKey,
              value: provider.bearerToken,
              enabled: !saving,
              obscureText: true,
              onChanged: (value) =>
                  onUpdate((item) => item.copyWith(bearerToken: value)),
            ),
            const SizedBox(height: 10),
            DropdownButtonFormField<String>(
              initialValue:
                  models.any((model) => model.slug == provider.defaultModel)
                  ? provider.defaultModel
                  : models.firstOrNull?.slug,
              decoration: InputDecoration(
                labelText: context.l10n.settingsDefaultModel,
              ),
              items: [
                for (final model in models)
                  DropdownMenuItem(
                    value: model.slug,
                    child: Text('${model.displayName} (${model.slug})'),
                  ),
              ],
              onChanged: saving
                  ? null
                  : (value) {
                      if (value != null) {
                        onUpdate((item) => item.copyWith(defaultModel: value));
                      }
                    },
            ),
          ],
        ),
        const SizedBox(height: 12),
        _SectionPanel(
          title: context.l10n.settingsProviderDefaultModelsTitle,
          trailing: Text(
            context.l10n.settingsBundledModels(provider.defaultModels.length),
          ),
          children: [
            for (final model in provider.defaultModels)
              _ModelReadout(model: model),
          ],
        ),
        const SizedBox(height: 12),
        _SectionPanel(
          title: context.l10n.settingsProviderCustomModelsTitle,
          trailing: OutlinedButton.icon(
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
              for (var index = 0; index < provider.customModels.length; index++)
                _CustomModelEditor(
                  model: provider.customModels[index],
                  enabled: !saving,
                  onChanged: (model) => onUpdateCustomModel(index, model),
                  onRemove: () => onRemoveCustomModel(index),
                ),
          ],
        ),
      ],
    );
  }
}

class _CustomModelEditor extends StatelessWidget {
  const _CustomModelEditor({
    required this.model,
    required this.enabled,
    required this.onChanged,
    required this.onRemove,
  });

  final ProviderModelView model;
  final bool enabled;
  final ValueChanged<ProviderModelView> onChanged;
  final VoidCallback onRemove;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: Border.all(
            color: Theme.of(context).colorScheme.outlineVariant,
          ),
          borderRadius: BorderRadius.circular(8),
        ),
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            children: [
              Row(
                children: [
                  Expanded(
                    child: _TextEdit(
                      label: context.l10n.settingsModelSlug,
                      value: model.slug,
                      enabled: enabled,
                      onChanged: (value) =>
                          onChanged(model.copyWith(slug: value)),
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: _TextEdit(
                      label: context.l10n.settingsDisplayName,
                      value: model.displayName,
                      enabled: enabled,
                      onChanged: (value) =>
                          onChanged(model.copyWith(displayName: value)),
                    ),
                  ),
                  const SizedBox(width: 6),
                  IconButton(
                    tooltip: context.l10n.settingsRemoveModel,
                    icon: const Icon(Icons.delete_outline),
                    onPressed: enabled ? onRemove : null,
                  ),
                ],
              ),
              const SizedBox(height: 10),
              _TextEdit(
                label: context.l10n.settingsReasoningEfforts,
                value: model.reasoningEfforts.join(', '),
                enabled: enabled,
                onChanged: (value) => onChanged(
                  model.copyWith(
                    reasoningEfforts: value
                        .split(',')
                        .map((part) => part.trim())
                        .where((part) => part.isNotEmpty)
                        .toList(),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ModelReadout extends StatelessWidget {
  const _ModelReadout({required this.model, this.framed = false});

  final ProviderModelView model;
  final bool framed;

  @override
  Widget build(BuildContext context) {
    final price = _modelPriceLabel(model);
    final row = Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      child: Row(
        children: [
          StudioIconBadge(
            icon: Icons.smart_toy_outlined,
            size: 30,
            backgroundColor: context.studioPaper2,
            foregroundColor: context.studioInkSoft,
          ),
          const SizedBox(width: 11),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  model.displayName,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.labelLarge?.copyWith(
                    color: context.studioInk,
                  ),
                ),
                Text(
                  model.slug,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.bodySmall?.copyWith(
                    color: context.studioInkSoft,
                    fontFamily: 'Consolas',
                  ),
                ),
              ],
            ),
          ),
          if (price.isNotEmpty) ...[
            const SizedBox(width: 10),
            Text(
              price,
              style: context.text.labelSmall?.copyWith(
                color: context.studioInkSoft,
              ),
            ),
          ],
        ],
      ),
    );
    if (!framed) {
      return row;
    }
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: StudioPanel(
        backgroundColor: Theme.of(context).colorScheme.surfaceContainerLowest,
        radius: StudioRadii.sm,
        child: row,
      ),
    );
  }
}
