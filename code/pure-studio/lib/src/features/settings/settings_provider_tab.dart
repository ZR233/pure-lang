import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import 'provider_usage_controller.dart';
import 'settings_provider_drafts.dart';
import 'settings_provider_editor.dart';
import 'settings_provider_list.dart';

class ProvidersTab extends ConsumerStatefulWidget {
  const ProvidersTab({
    super.key,
    required this.providers,
    required this.providerCatalog,
    required this.defaultProviderId,
    required this.roles,
  });

  final List<ProviderSettingsView> providers;
  final ProviderCatalogView providerCatalog;
  final String? defaultProviderId;
  final List<RoleSettingsView> roles;

  @override
  ConsumerState<ProvidersTab> createState() => ProvidersTabState();
}

class ProvidersTabState extends ConsumerState<ProvidersTab> {
  String _query = '';
  String? _selectedProviderId;
  ProviderDraft? _draft;
  bool _showDetails = false;
  bool _saving = false;
  String? _draftError;

  @override
  void didUpdateWidget(covariant ProvidersTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    final selected = _selectedProviderId;
    if (selected != null &&
        !widget.providers.any((provider) => provider.id == selected)) {
      _selectedProviderId = widget.providers.firstOrNull?.id;
      _showDetails = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    final catalog = widget.providerCatalog;
    final defaultProviderId =
        widget.defaultProviderId ?? widget.providers.firstOrNull?.id;
    final selectedId =
        _selectedProviderId ??
        defaultProviderId ??
        widget.providers.firstOrNull?.id;
    final selected = widget.providers
        .where((provider) => provider.id == selectedId)
        .firstOrNull;
    final filtered = _filteredProviders();
    final usageAsync = ref.watch(providerUsageControllerProvider);
    final usageState = usageAsync.value;
    final usageByProvider = {
      for (final usage in usageState?.usages ?? const <ProviderUsageView>[])
        usage.providerId: usage,
    };
    final loadingProviderIds = usageAsync.isLoading
        ? widget.providers.map((provider) => provider.id).toSet()
        : usageState?.loadingProviderIds ?? const <String>{};
    if (_draft != null) {
      return Padding(
        padding: const EdgeInsets.all(20),
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 980),
            child: ProviderEditor(
              key: ValueKey((
                _draft!.mode,
                _draft!.originalId,
                _draft!.provider.templateKind,
              )),
              draft: _draft!,
              presets: catalog.presets,
              saving: _saving,
              error: _draftError,
              onCancel: () => setState(() => _draft = null),
              onSave: _saveDraft,
              onChangeTemplate: _changeDraftTemplate,
              onUpdate: _updateDraft,
              onAddCustomModel: _addCustomModel,
              onUpdateCustomModel: _updateCustomModel,
              onRemoveCustomModel: _removeCustomModel,
            ),
          ),
        ),
      );
    }
    if (_showDetails) {
      return Padding(
        padding: const EdgeInsets.all(20),
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 980),
            child: ProviderDetails(
              provider: selected,
              usage: selected == null ? null : usageByProvider[selected.id],
              usageLoading: selected == null
                  ? false
                  : loadingProviderIds.contains(selected.id),
              usageError: selected == null
                  ? null
                  : usageState?.errorFor(selected.id),
              onBack: () => setState(() => _showDetails = false),
              onEdit: _startEdit,
              onRefreshUsage: selected == null
                  ? null
                  : () => _refreshUsages(providerId: selected.id),
            ),
          ),
        ),
      );
    }
    return LayoutBuilder(
      builder: (context, constraints) {
        return Padding(
          padding: const EdgeInsets.all(20),
          child: ProviderList(
            providers: filtered,
            defaultProviderId: defaultProviderId,
            filtering: _query.trim().isNotEmpty,
            usageByProvider: usageByProvider,
            loadingProviderIds: loadingProviderIds,
            usageErrorsByProviderId: usageState?.errorsByProviderId ?? const {},
            onQueryChanged: (value) => setState(() => _query = value),
            onAdd: _startAdd,
            onSelect: (provider) => setState(() {
              _selectedProviderId = provider.id;
              _showDetails = true;
            }),
            onSetDefault: _setDefaultProvider,
            onRefreshAll: _refreshUsages,
            onRefreshProvider: (provider) =>
                _refreshUsages(providerId: provider.id),
            onEdit: _startEdit,
            onDelete: widget.providers.length <= 1 ? null : _removeProvider,
          ),
        );
      },
    );
  }

  List<ProviderSettingsView> _filteredProviders() {
    final query = _query.trim().toLowerCase();
    if (query.isEmpty) {
      return widget.providers;
    }
    return widget.providers.where((provider) {
      final haystack = [
        provider.name,
        provider.id,
        provider.defaultModel,
        for (final model in provider.allModels) model.slug,
        for (final model in provider.allModels) model.displayName,
        for (final model in provider.allModels) model.wireProtocol,
        for (final model in provider.allModels) model.connectionMode,
      ].join(' ').toLowerCase();
      return haystack.contains(query);
    }).toList();
  }

  void _startAdd() {
    final catalog = ref
        .read(studioControllerProvider)
        .asData
        ?.value
        .providerCatalog;
    final template = catalog?.presets.firstOrNull;
    if (catalog == null || template == null) {
      setState(() => _draftError = 'Provider catalog is unavailable.');
      return;
    }
    final id = _suggestProviderId(template.id);
    final draft = ProviderDraftFactory.create(
      catalog: catalog,
      templateId: template.id,
      providerId: id,
    );
    if (draft == null) {
      setState(() => _draftError = 'Provider preset is unavailable.');
      return;
    }
    setState(() {
      _selectedProviderId = id;
      _showDetails = false;
      _draft = draft;
      _draftError = null;
    });
  }

  void _startEdit(ProviderSettingsView provider) {
    setState(() {
      _selectedProviderId = provider.id;
      _showDetails = false;
      _draft = ProviderDraft.edit(provider);
      _draftError = null;
    });
  }

  void _changeDraftTemplate(String templateId) {
    final catalog = ref
        .read(studioControllerProvider)
        .asData
        ?.value
        .providerCatalog;
    final current = _draft;
    if (catalog == null || current == null) {
      return;
    }
    final id = current.mode == ProviderDraftMode.create && templateId.isNotEmpty
        ? _suggestProviderId(templateId)
        : current.provider.id;
    final next = ProviderDraftFactory.changeTemplate(
      draft: current,
      catalog: catalog,
      templateId: templateId,
      providerId: id,
    );
    if (next == null) {
      return;
    }
    setState(() => _draft = next);
  }

  void _updateDraft(
    ProviderSettingsView Function(ProviderSettingsView) update,
  ) {
    final current = _draft;
    if (current == null) {
      return;
    }
    setState(
      () => _draft = current.copyWith(provider: update(current.provider)),
    );
  }

  void _addCustomModel() {
    _updateDraft((provider) {
      final existing = provider.allModels.map((model) => model.slug).toSet();
      var slug = 'custom-model';
      for (var index = 2; existing.contains(slug); index += 1) {
        slug = 'custom-model-$index';
      }
      final model = ProviderModelView(
        slug: slug,
        displayName: slug,
        contextWindow: 32000,
        maxOutputTokens: 4096,
        reasoningEfforts: const [],
        wireProtocol: 'chat_completions',
        supportedConnectionModes: const ['http'],
        defaultConnectionMode: 'http',
        connectionMode: 'http',
      );
      return provider.copyWith(
        customModels: [...provider.customModels, model],
        models: [...provider.defaultModels, ...provider.customModels, model],
        defaultModel: provider.defaultModel.isEmpty
            ? slug
            : provider.defaultModel,
      );
    });
  }

  void _updateCustomModel(int index, ProviderModelView model) {
    _updateDraft((provider) {
      final custom = [...provider.customModels];
      final previousSlug = custom[index].slug;
      custom[index] = model;
      return provider.copyWith(
        customModels: custom,
        defaultModel: provider.defaultModel == previousSlug
            ? model.slug
            : provider.defaultModel,
        models: [...provider.defaultModels, ...custom],
      );
    });
  }

  void _removeCustomModel(int index) {
    _updateDraft((provider) {
      final custom = [...provider.customModels]..removeAt(index);
      final models = [...provider.defaultModels, ...custom];
      final defaultModel =
          models.any((model) => model.slug == provider.defaultModel)
          ? provider.defaultModel
          : models.firstOrNull?.slug ?? '';
      return provider.copyWith(
        customModels: custom,
        models: models,
        defaultModel: defaultModel,
      );
    });
  }

  Future<void> _saveDraft() async {
    final current = _draft;
    if (current == null || _saving) {
      return;
    }
    setState(() {
      _saving = true;
      _draftError = null;
    });
    try {
      final provider = ProviderSettingsCommandBuilder.normalizeProvider(
        current.provider,
      );
      final providers = current.mode == ProviderDraftMode.create
          ? [...widget.providers, provider]
          : [
              for (final item in widget.providers)
                item.id == current.originalId ? provider : item,
            ];
      await _saveProviders(
        providers,
        selectedProviderId: _defaultProviderIdAfterDraftSave(current, provider),
        renamedFrom: current.originalId == provider.id
            ? null
            : current.originalId,
        renamedTo: current.originalId == provider.id ? null : provider.id,
      );
      if (mounted) {
        setState(() {
          _selectedProviderId = provider.id;
          _draft = null;
          _showDetails = false;
        });
      }
      await _refreshUsages(providerId: provider.id);
    } catch (error) {
      if (mounted) {
        setState(() => _draftError = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _saving = false);
      }
    }
  }

  Future<void> _removeProvider(ProviderSettingsView provider) async {
    if (widget.providers.length <= 1) {
      return;
    }
    final providers = widget.providers
        .where((candidate) => candidate.id != provider.id)
        .toList();
    final currentDefaultId = ref
        .read(studioControllerProvider)
        .asData
        ?.value
        .defaultProviderId;
    final selectedProviderId = currentDefaultId == provider.id
        ? providers.firstOrNull?.id
        : currentDefaultId;
    await _saveProviders(
      providers,
      selectedProviderId: selectedProviderId,
      removedProviderId: provider.id,
    );
    if (mounted) {
      setState(() {
        _selectedProviderId = selectedProviderId;
        _showDetails = false;
        if (_draft?.originalId == provider.id) {
          _draft = null;
        }
      });
    }
    await _refreshUsages();
  }

  Future<void> _setDefaultProvider(ProviderSettingsView provider) async {
    setState(() => _selectedProviderId = provider.id);
    await _saveProviders(widget.providers, selectedProviderId: provider.id);
    await _refreshUsages(providerId: provider.id);
  }

  String? _defaultProviderIdAfterDraftSave(
    ProviderDraft draft,
    ProviderSettingsView provider,
  ) {
    final currentDefaultId = ref
        .read(studioControllerProvider)
        .asData
        ?.value
        .defaultProviderId;
    if (draft.mode == ProviderDraftMode.create) {
      return provider.id;
    }
    if (currentDefaultId == draft.originalId) {
      return provider.id;
    }
    return currentDefaultId ?? widget.providers.firstOrNull?.id ?? provider.id;
  }

  Future<void> _refreshUsages({String? providerId}) async {
    await ref
        .read(providerUsageControllerProvider.notifier)
        .refresh(providerId: providerId);
  }

  Future<void> _saveProviders(
    List<ProviderSettingsView> providers, {
    required String? selectedProviderId,
    String? renamedFrom,
    String? renamedTo,
    String? removedProviderId,
  }) async {
    await ref
        .read(studioControllerProvider.notifier)
        .saveProviderSettings(
          ProviderSettingsCommandBuilder.build(
            providers: providers,
            roles: widget.roles,
            selectedProviderId: selectedProviderId,
            renamedFrom: renamedFrom,
            renamedTo: renamedTo,
            removedProviderId: removedProviderId,
          ),
        );
  }

  String _suggestProviderId(String base) {
    final existing = widget.providers.map((provider) => provider.id).toSet();
    if (!existing.contains(base)) {
      return base;
    }
    for (var index = 2; ; index += 1) {
      final candidate = '$base-$index';
      if (!existing.contains(candidate)) {
        return candidate;
      }
    }
  }
}
