part of 'settings_page.dart';

class _ProvidersTab extends ConsumerStatefulWidget {
  const _ProvidersTab({required this.providers, required this.roles});

  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;

  @override
  ConsumerState<_ProvidersTab> createState() => _ProvidersTabState();
}

class _ProvidersTabState extends ConsumerState<_ProvidersTab> {
  String _query = '';
  String? _selectedProviderId;
  _ProviderDraft? _draft;
  bool _showDetails = false;
  bool _saving = false;
  String? _draftError;
  final Set<String> _usageLoadingProviderIds = {};
  String? _usageError;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        unawaited(_refreshUsages());
      }
    });
  }

  @override
  void didUpdateWidget(covariant _ProvidersTab oldWidget) {
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
    final selectedId = _selectedProviderId ?? widget.providers.firstOrNull?.id;
    final selected = widget.providers
        .where((provider) => provider.id == selectedId)
        .firstOrNull;
    final filtered = _filteredProviders();
    final state = ref.watch(studioControllerProvider).asData?.value;
    final usageByProvider = {
      for (final usage in state?.providerUsages ?? const <ProviderUsageView>[])
        usage.providerId: usage,
    };
    if (_draft != null) {
      return Padding(
        padding: const EdgeInsets.all(20),
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 980),
            child: _ProviderEditor(
              draft: _draft!,
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
            child: _ProviderDetails(
              provider: selected,
              usage: selected == null ? null : usageByProvider[selected.id],
              usageLoading: selected == null
                  ? false
                  : _usageLoadingProviderIds.contains(selected.id),
              usageError: _usageError,
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
          child: _ProviderList(
            providers: filtered,
            selectedProviderId: selectedId,
            usageByProvider: usageByProvider,
            loadingProviderIds: _usageLoadingProviderIds,
            usageError: _usageError,
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
        provider.providerKind,
        for (final model in provider.allModels) model.slug,
        for (final model in provider.allModels) model.displayName,
      ].join(' ').toLowerCase();
      return haystack.contains(query);
    }).toList();
  }

  void _startAdd() {
    final template = _providerTemplates.first;
    final id = _suggestProviderId(template.id);
    setState(() {
      _selectedProviderId = id;
      _showDetails = false;
      _draft = _ProviderDraft.create(template.createProvider(id));
      _draftError = null;
    });
  }

  void _startEdit(ProviderSettingsView provider) {
    setState(() {
      _selectedProviderId = provider.id;
      _showDetails = false;
      _draft = _ProviderDraft.edit(provider);
      _draftError = null;
    });
  }

  void _changeDraftTemplate(String templateId) {
    final template = _providerTemplates
        .where((candidate) => candidate.id == templateId)
        .firstOrNull;
    if (template == null || _draft == null) {
      return;
    }
    setState(() {
      final current = _draft!;
      final id = current.mode == _ProviderDraftMode.create
          ? _suggestProviderId(template.id)
          : current.provider.id;
      _draft = current.copyWith(provider: template.createProvider(id));
    });
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
        displayName: 'Custom model',
        reasoningEfforts: const ['high'],
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
      custom[index] = model;
      return provider.copyWith(
        customModels: custom,
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
      final provider = _normalizedProvider(current.provider);
      final providers = current.mode == _ProviderDraftMode.create
          ? [...widget.providers, provider]
          : [
              for (final item in widget.providers)
                item.id == current.originalId ? provider : item,
            ];
      await _saveProviders(
        providers,
        selectedProviderId: provider.id,
        renamedFrom: current.originalId == provider.id
            ? null
            : current.originalId,
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
    final selectedProviderId = _selectedProviderId == provider.id
        ? providers.firstOrNull?.id
        : _selectedProviderId;
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

  Future<void> _refreshUsages({String? providerId}) async {
    if (!mounted) {
      return;
    }
    setState(() {
      _usageError = null;
      if (providerId == null) {
        _usageLoadingProviderIds
          ..clear()
          ..addAll(widget.providers.map((provider) => provider.id));
      } else {
        _usageLoadingProviderIds.add(providerId);
      }
    });
    try {
      await ref.read(studioControllerProvider.notifier).refreshProviderUsages();
    } catch (error) {
      if (mounted) {
        setState(() => _usageError = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() {
          if (providerId == null) {
            _usageLoadingProviderIds.clear();
          } else {
            _usageLoadingProviderIds.remove(providerId);
          }
        });
      }
    }
  }

  Future<void> _saveProviders(
    List<ProviderSettingsView> providers, {
    required String? selectedProviderId,
    String? renamedFrom,
    String? removedProviderId,
  }) async {
    final normalized = providers.map(_normalizedProvider).toList();
    await ref.read(studioControllerProvider.notifier).saveProviderSettings({
      'defaultProviderId': selectedProviderId ?? normalized.firstOrNull?.id,
      'providers': normalized.map(_providerPayload).toList(),
      'roles': _normalizedRolePayloads(
        normalized,
        renamedFrom: renamedFrom,
        removedProviderId: removedProviderId,
      ),
    });
  }

  ProviderSettingsView _normalizedProvider(ProviderSettingsView provider) {
    final models = provider.allModels
        .where((model) => model.slug.trim().isNotEmpty)
        .toList();
    final defaultModel =
        models.any((model) => model.slug == provider.defaultModel)
        ? provider.defaultModel
        : models.firstOrNull?.slug ?? provider.defaultModel;
    return provider.copyWith(
      id: provider.id.trim(),
      name: provider.name.trim(),
      baseUrl: provider.baseUrl.trim(),
      defaultModel: defaultModel.trim(),
      models: models,
      customModels: provider.customModels
          .where((model) => model.slug.trim().isNotEmpty)
          .toList(),
    );
  }

  List<Map<String, Object?>> _normalizedRolePayloads(
    List<ProviderSettingsView> providers, {
    String? renamedFrom,
    String? removedProviderId,
  }) {
    final fallback = providers.firstOrNull;
    if (fallback == null) {
      return const [];
    }
    final providerIds = providers.map((provider) => provider.id).toSet();
    return widget.roles.map((role) {
      var providerId = role.providerId;
      if (renamedFrom != null && providerId == renamedFrom) {
        providerId = _draft?.provider.id ?? providerId;
      }
      if (removedProviderId != null && providerId == removedProviderId) {
        providerId = fallback.id;
      }
      final provider = providers
          .where((candidate) => candidate.id == providerId)
          .firstOrNull;
      final safeProvider = providerIds.contains(providerId) && provider != null
          ? provider
          : fallback;
      final model =
          safeProvider.allModels.any((item) => item.slug == role.model)
          ? role.model
          : safeProvider.defaultModel;
      return {
        'key': role.key,
        'provider': safeProvider.id,
        'model': model,
        'effort': role.effort.isEmpty ? 'high' : role.effort,
      };
    }).toList();
  }

  Map<String, Object?> _providerPayload(ProviderSettingsView provider) {
    return {
      'id': provider.id,
      'templateKind': provider.templateKind,
      'name': provider.name,
      'baseUrl': provider.baseUrl,
      'bearerToken': provider.bearerToken,
      'defaultModel': provider.defaultModel,
      'providerKind': provider.providerKind,
      'customModels': provider.customModels.map(_modelPayload).toList(),
    };
  }

  Map<String, Object?> _modelPayload(ProviderModelView model) {
    return {
      'slug': model.slug,
      'displayName': model.displayName,
      'reasoningEfforts': model.reasoningEfforts,
      'baseInstructions': model.baseInstructions,
    };
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
