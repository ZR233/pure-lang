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
            label: const Text('Providers'),
            onPressed: onBack,
          ),
          const Expanded(child: Center(child: Text('No provider selected'))),
        ],
      );
    }
    return ListView(
      children: [
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton.icon(
            icon: const Icon(Icons.arrow_back),
            label: const Text('Providers'),
            onPressed: onBack,
          ),
        ),
        const SizedBox(height: 4),
        _SettingsHeader(
          title: provider.name,
          subtitle: provider.baseUrl,
          trailing: FilledButton.tonalIcon(
            icon: const Icon(Icons.edit_outlined),
            label: const Text('Edit'),
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
          title: 'Provider',
          children: [
            _Readout(label: 'Provider key', value: provider.id),
            _Readout(label: 'Template', value: provider.templateKind),
            _Readout(label: 'Default model', value: provider.defaultModel),
            _Readout(
              label: 'API key',
              value: provider.hasBearerToken ? 'configured' : 'missing',
            ),
          ],
        ),
        const SizedBox(height: 12),
        _SectionPanel(
          title: 'Models',
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
              ? 'New provider'
              : provider.name,
          subtitle: provider.baseUrl,
          trailing: Wrap(
            spacing: 8,
            children: [
              OutlinedButton.icon(
                icon: const Icon(Icons.close),
                label: const Text('Cancel'),
                onPressed: saving ? null : onCancel,
              ),
              FilledButton.icon(
                icon: const Icon(Icons.save_outlined),
                label: const Text('Save'),
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
          title: 'Connection',
          children: [
            _ResponsiveFieldGrid(
              children: [
                _TextEdit(
                  label: 'Provider key',
                  value: provider.id,
                  enabled: !saving,
                  onChanged: (value) =>
                      onUpdate((item) => item.copyWith(id: value)),
                ),
                DropdownButtonFormField<String>(
                  initialValue: provider.templateKind,
                  decoration: const InputDecoration(labelText: 'Template'),
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
                  label: 'Display name',
                  value: provider.name,
                  enabled: !saving,
                  onChanged: (value) =>
                      onUpdate((item) => item.copyWith(name: value)),
                ),
                _ReadonlyField(
                  label: 'Protocol type',
                  value: provider.providerKind,
                ),
              ],
            ),
            const SizedBox(height: 10),
            _TextEdit(
              label: 'Base URL',
              value: provider.baseUrl,
              enabled: !saving,
              onChanged: (value) =>
                  onUpdate((item) => item.copyWith(baseUrl: value)),
            ),
            const SizedBox(height: 10),
            _TextEdit(
              label: provider.hasBearerToken
                  ? 'API key (leave blank to keep current)'
                  : 'API key',
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
              decoration: const InputDecoration(labelText: 'Default model'),
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
          title: 'Default models',
          trailing: Text('${provider.defaultModels.length} bundled'),
          children: [
            for (final model in provider.defaultModels)
              _ModelReadout(model: model),
          ],
        ),
        const SizedBox(height: 12),
        _SectionPanel(
          title: 'Custom models',
          trailing: OutlinedButton.icon(
            icon: const Icon(Icons.add),
            label: const Text('Add model'),
            onPressed: saving ? null : onAddCustomModel,
          ),
          children: [
            if (provider.customModels.isEmpty)
              const Padding(
                padding: EdgeInsets.symmetric(vertical: 8),
                child: Text('No custom models'),
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
                      label: 'Model slug',
                      value: model.slug,
                      enabled: enabled,
                      onChanged: (value) =>
                          onChanged(model.copyWith(slug: value)),
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: _TextEdit(
                      label: 'Display name',
                      value: model.displayName,
                      enabled: enabled,
                      onChanged: (value) =>
                          onChanged(model.copyWith(displayName: value)),
                    ),
                  ),
                  const SizedBox(width: 6),
                  IconButton(
                    tooltip: 'Remove model',
                    icon: const Icon(Icons.delete_outline),
                    onPressed: enabled ? onRemove : null,
                  ),
                ],
              ),
              const SizedBox(height: 10),
              _TextEdit(
                label: 'Reasoning efforts',
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
    final tile = ListTile(
      dense: true,
      leading: const Icon(Icons.smart_toy_outlined),
      title: Text(model.displayName),
      subtitle: Text(model.slug),
      trailing: Text(_modelPriceLabel(model)),
    );
    if (!framed) {
      return tile;
    }
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: StudioPanel(
        backgroundColor: Theme.of(context).colorScheme.surfaceContainerLowest,
        radius: StudioRadii.sm,
        child: tile,
      ),
    );
  }
}
