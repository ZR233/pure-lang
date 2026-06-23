import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';

class SettingsPage extends ConsumerWidget {
  const SettingsPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncState = ref.watch(studioControllerProvider);
    return asyncState.when(
      loading: () =>
          const Scaffold(body: Center(child: CircularProgressIndicator())),
      error: (error, stackTrace) =>
          Scaffold(body: Center(child: Text(error.toString()))),
      data: (state) => DefaultTabController(
        length: 7,
        child: Scaffold(
          appBar: AppBar(
            leading: IconButton(
              tooltip: 'Back',
              icon: const Icon(Icons.arrow_back),
              onPressed: () => context.go('/'),
            ),
            title: const Text('Settings'),
            bottom: const TabBar(
              isScrollable: true,
              tabs: [
                Tab(icon: Icon(Icons.cloud_outlined), text: 'Providers'),
                Tab(icon: Icon(Icons.notes_outlined), text: 'Instructions'),
                Tab(icon: Icon(Icons.extension_outlined), text: 'Skills'),
                Tab(icon: Icon(Icons.badge_outlined), text: 'Roles'),
                Tab(icon: Icon(Icons.hub_outlined), text: 'MCP'),
                Tab(icon: Icon(Icons.security_outlined), text: 'Security'),
                Tab(icon: Icon(Icons.tune_outlined), text: 'General'),
              ],
            ),
          ),
          body: TabBarView(
            children: [
              _ProvidersTab(providers: state.providers, roles: state.roles),
              _InstructionsTab(settings: state.instructions),
              _SkillsTab(
                skills: {
                  ...state.runtime.activeSkills,
                  ...state.skills.disabled,
                }.toList(),
                settings: state.skills,
                projectId: state.selectedProjectId,
              ),
              _RolesTab(providers: state.providers, roles: state.roles),
              _McpTab(servers: state.mcpServers),
              _SecurityTab(mode: state.permissionMode),
              _GeneralTab(settings: state.general),
            ],
          ),
        ),
      ),
    );
  }
}

class _SettingsPane extends StatelessWidget {
  const _SettingsPane({required this.children, this.maxWidth = 980});

  final List<Widget> children;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: BoxConstraints(maxWidth: maxWidth),
        child: ListView(padding: const EdgeInsets.all(20), children: children),
      ),
    );
  }
}

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

class _ProviderList extends StatelessWidget {
  const _ProviderList({
    required this.providers,
    required this.selectedProviderId,
    required this.usageByProvider,
    required this.loadingProviderIds,
    required this.usageError,
    required this.onQueryChanged,
    required this.onAdd,
    required this.onSelect,
    required this.onSetDefault,
    required this.onRefreshAll,
    required this.onRefreshProvider,
    required this.onEdit,
    required this.onDelete,
  });

  final List<ProviderSettingsView> providers;
  final String? selectedProviderId;
  final Map<String, ProviderUsageView> usageByProvider;
  final Set<String> loadingProviderIds;
  final String? usageError;
  final ValueChanged<String> onQueryChanged;
  final VoidCallback onAdd;
  final ValueChanged<ProviderSettingsView> onSelect;
  final ValueChanged<ProviderSettingsView> onSetDefault;
  final Future<void> Function({String? providerId}) onRefreshAll;
  final ValueChanged<ProviderSettingsView> onRefreshProvider;
  final ValueChanged<ProviderSettingsView> onEdit;
  final ValueChanged<ProviderSettingsView>? onDelete;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _SettingsHeader(
          title: 'Providers',
          subtitle: 'Model providers, credentials, models, and usage',
          trailing: Wrap(
            spacing: 8,
            children: [
              OutlinedButton.icon(
                icon: const Icon(Icons.refresh),
                label: const Text('Refresh usage'),
                onPressed: () => onRefreshAll(),
              ),
              FilledButton.icon(
                icon: const Icon(Icons.add),
                label: const Text('Add provider'),
                onPressed: onAdd,
              ),
            ],
          ),
        ),
        const SizedBox(height: 14),
        Row(
          children: [
            Expanded(
              child: SearchBar(
                leading: const Icon(Icons.search),
                hintText: 'Search providers',
                onChanged: onQueryChanged,
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        Expanded(
          child: Align(
            alignment: Alignment.topCenter,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 980),
              child: providers.isEmpty
                  ? const Center(child: Text('No providers found'))
                  : ListView.builder(
                      itemCount: providers.length,
                      itemBuilder: (context, index) {
                        final provider = providers[index];
                        return Padding(
                          padding: const EdgeInsets.only(bottom: 12),
                          child: _ProviderCard(
                            provider: provider,
                            selected: provider.id == selectedProviderId,
                            usage: usageByProvider[provider.id],
                            usageLoading: loadingProviderIds.contains(
                              provider.id,
                            ),
                            usageError: usageError,
                            onOpen: () => onSelect(provider),
                            onSetDefault: () => onSetDefault(provider),
                            onRefreshUsage: () => onRefreshProvider(provider),
                            onEdit: () => onEdit(provider),
                            onDelete: onDelete == null
                                ? null
                                : () => onDelete!(provider),
                          ),
                        );
                      },
                    ),
            ),
          ),
        ),
      ],
    );
  }
}

class _ProviderCard extends StatelessWidget {
  const _ProviderCard({
    required this.provider,
    required this.selected,
    required this.usage,
    required this.usageLoading,
    required this.usageError,
    required this.onOpen,
    required this.onSetDefault,
    required this.onRefreshUsage,
    required this.onEdit,
    required this.onDelete,
  });

  final ProviderSettingsView provider;
  final bool selected;
  final ProviderUsageView? usage;
  final bool usageLoading;
  final String? usageError;
  final VoidCallback onOpen;
  final VoidCallback onSetDefault;
  final VoidCallback onRefreshUsage;
  final VoidCallback onEdit;
  final VoidCallback? onDelete;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final models = provider.allModels;
    return Material(
      color: selected
          ? colors.surfaceContainerHighest.withValues(alpha: 0.62)
          : colors.surface,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(8),
        side: BorderSide(
          color: selected
              ? colors.primary.withValues(alpha: 0.55)
              : colors.outlineVariant,
        ),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(8),
        onTap: onOpen,
        child: Padding(
          padding: const EdgeInsets.all(14),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  CircleAvatar(
                    radius: 18,
                    backgroundColor: colors.surfaceContainerHigh,
                    child: Text(
                      _initials(provider.name),
                      style: Theme.of(context).textTheme.labelSmall,
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Expanded(
                              child: Text(
                                provider.name,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: Theme.of(context).textTheme.titleMedium,
                              ),
                            ),
                            if (selected)
                              const _InfoPill(
                                icon: Icons.star_outline,
                                label: 'default',
                              ),
                          ],
                        ),
                        const SizedBox(height: 3),
                        Text(
                          provider.id,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: colors.onSurfaceVariant),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 10),
                  _ProviderStatusChip(provider: provider),
                ],
              ),
              const SizedBox(height: 10),
              Wrap(
                spacing: 8,
                runSpacing: 6,
                children: [
                  _MiniMeta(
                    icon: Icons.smart_toy_outlined,
                    label: provider.defaultModel,
                  ),
                  _MiniMeta(
                    icon: Icons.memory_outlined,
                    label: '${models.length} models',
                  ),
                  _MiniMeta(
                    icon: Icons.hub_outlined,
                    label: provider.providerKind,
                  ),
                  _MiniMeta(
                    icon: Icons.account_balance_wallet_outlined,
                    label: _providerUsageSummary(provider, usage, usageLoading),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              _ProviderUsagePanel(
                provider: provider,
                usage: usage,
                loading: usageLoading,
                error: usageError,
                onRefresh: onRefreshUsage,
              ),
              if (models.isNotEmpty) ...[
                const SizedBox(height: 12),
                Wrap(
                  spacing: 6,
                  runSpacing: 6,
                  children: [
                    for (final model in models.take(5))
                      Chip(
                        visualDensity: VisualDensity.compact,
                        label: Text(model.slug),
                      ),
                    if (models.length > 5)
                      Chip(
                        visualDensity: VisualDensity.compact,
                        label: Text('+${models.length - 5}'),
                      ),
                  ],
                ),
              ],
              const SizedBox(height: 10),
              Align(
                alignment: Alignment.centerRight,
                child: Wrap(
                  spacing: 4,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    TextButton.icon(
                      icon: const Icon(Icons.open_in_new),
                      label: const Text('Details'),
                      onPressed: onOpen,
                    ),
                    TextButton.icon(
                      icon: const Icon(Icons.star_outline),
                      label: const Text('Set default'),
                      onPressed: selected ? null : onSetDefault,
                    ),
                    IconButton(
                      tooltip: 'Edit provider',
                      icon: const Icon(Icons.edit_outlined),
                      onPressed: onEdit,
                    ),
                    IconButton(
                      tooltip: 'Delete provider',
                      icon: const Icon(Icons.delete_outline),
                      onPressed: onDelete,
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ProviderUsagePanel extends StatelessWidget {
  const _ProviderUsagePanel({
    required this.provider,
    required this.usage,
    required this.loading,
    required this.error,
    required this.onRefresh,
  });

  final ProviderSettingsView provider;
  final ProviderUsageView? usage;
  final bool loading;
  final String? error;
  final VoidCallback onRefresh;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final usage = this.usage;
    return Material(
      color: colors.surfaceContainerLowest,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(8),
        side: BorderSide(color: colors.outlineVariant),
      ),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(
                  Icons.account_balance_wallet_outlined,
                  size: 17,
                  color: colors.onSurfaceVariant,
                ),
                const SizedBox(width: 7),
                Expanded(
                  child: Text(
                    'Usage',
                    style: Theme.of(context).textTheme.titleSmall,
                  ),
                ),
                Text(
                  _usageUpdatedLabel(usage?.updatedAt),
                  style: Theme.of(context).textTheme.labelSmall?.copyWith(
                    color: colors.onSurfaceVariant,
                  ),
                ),
                const SizedBox(width: 4),
                IconButton(
                  tooltip: _providerSupportsUsage(provider)
                      ? 'Refresh usage'
                      : 'Usage is not supported',
                  icon: loading
                      ? const SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.refresh, size: 18),
                  onPressed: loading || !_providerSupportsUsage(provider)
                      ? null
                      : onRefresh,
                ),
              ],
            ),
            if (error != null && error!.isNotEmpty) ...[
              const SizedBox(height: 8),
              _ProviderUsageMessage(
                icon: Icons.error_outline,
                message: error!,
                tone: _UsageTone.failed,
              ),
            ],
            const SizedBox(height: 8),
            if (usage == null)
              _ProviderUsageMessage(
                icon: loading
                    ? Icons.hourglass_empty
                    : Icons.account_balance_wallet_outlined,
                message: loading
                    ? 'Checking usage...'
                    : _providerSupportsUsage(provider)
                    ? 'Usage not loaded'
                    : 'Usage not supported',
                tone: loading ? _UsageTone.neutral : _UsageTone.muted,
              )
            else if (usage.status == 'ready' &&
                usage.usageKind == 'deepseekBalance' &&
                usage.balance != null)
              _DeepSeekUsage(usage: usage.balance!)
            else if (usage.status == 'ready' &&
                usage.usageKind == 'zhipuCodingPlan' &&
                usage.codingPlan != null)
              _ZhipuCodingPlanUsage(usage: usage.codingPlan!)
            else
              _ProviderUsageMessage(
                icon:
                    usage.status == 'failed' ||
                        usage.status == 'missingCredential'
                    ? Icons.error_outline
                    : Icons.info_outline,
                message: _providerUsageMessage(provider, usage),
                tone: usage.status == 'failed'
                    ? _UsageTone.failed
                    : usage.status == 'missingCredential'
                    ? _UsageTone.warning
                    : _UsageTone.muted,
              ),
          ],
        ),
      ),
    );
  }
}

class _DeepSeekUsage extends StatelessWidget {
  const _DeepSeekUsage({required this.usage});

  final DeepSeekBalanceUsageView usage;

  @override
  Widget build(BuildContext context) {
    final primary =
        usage.balances
            .where((item) => item.currency.toUpperCase() == 'CNY')
            .firstOrNull ??
        usage.balances.firstOrNull;
    if (primary == null) {
      return const _ProviderUsageMessage(
        icon: Icons.info_outline,
        message: 'Usage unavailable',
        tone: _UsageTone.muted,
      );
    }
    final colors = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                usage.isAvailable ? 'Available balance' : 'Balance unavailable',
                style: Theme.of(context).textTheme.labelMedium?.copyWith(
                  color: colors.onSurfaceVariant,
                ),
              ),
            ),
            Text(
              '${primary.currency} ${primary.totalBalance}',
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ],
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 6,
          children: [
            _InfoPill(
              icon: Icons.card_giftcard_outlined,
              label: 'Granted ${primary.grantedBalance}',
            ),
            _InfoPill(
              icon: Icons.payments_outlined,
              label: 'Topped up ${primary.toppedUpBalance}',
            ),
            for (final item in usage.balances.where(
              (item) => item.currency != primary.currency,
            ))
              _InfoPill(
                icon: Icons.account_balance_wallet_outlined,
                label: '${item.currency} ${item.totalBalance}',
              ),
          ],
        ),
      ],
    );
  }
}

class _ZhipuCodingPlanUsage extends StatelessWidget {
  const _ZhipuCodingPlanUsage({required this.usage});

  final ZhipuCodingPlanUsageView usage;

  @override
  Widget build(BuildContext context) {
    final ordered = [
      _findQuotaLimit(usage.limits, 'fiveHour'),
      _findQuotaLimit(usage.limits, 'weekly'),
      _findQuotaLimit(usage.limits, 'mcpMonthly'),
      ...usage.limits.where((limit) => limit.window == 'other'),
    ].whereType<ZhipuQuotaLimitView>().toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (usage.level != null && usage.level!.isNotEmpty) ...[
          _InfoPill(
            icon: Icons.workspace_premium_outlined,
            label: usage.level!,
          ),
          const SizedBox(height: 8),
        ],
        if (ordered.isEmpty)
          const _ProviderUsageMessage(
            icon: Icons.info_outline,
            message: 'Usage unavailable',
            tone: _UsageTone.muted,
          )
        else
          LayoutBuilder(
            builder: (context, constraints) {
              final twoColumns = constraints.maxWidth >= 650;
              return Wrap(
                spacing: 10,
                runSpacing: 10,
                children: [
                  for (final limit in ordered)
                    SizedBox(
                      width: twoColumns
                          ? (constraints.maxWidth - 10) / 2
                          : constraints.maxWidth,
                      child: _QuotaCard(limit: limit),
                    ),
                ],
              );
            },
          ),
      ],
    );
  }
}

class _QuotaCard extends StatelessWidget {
  const _QuotaCard({required this.limit});

  final ZhipuQuotaLimitView limit;

  @override
  Widget build(BuildContext context) {
    final percent = _quotaRemainingPercent(limit);
    final colors = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: colors.surface,
        border: Border.all(color: colors.outlineVariant),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    _quotaTitle(limit),
                    style: Theme.of(context).textTheme.labelLarge,
                  ),
                ),
                Text(
                  _resetLabel(limit.nextResetAt),
                  style: Theme.of(context).textTheme.labelSmall?.copyWith(
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 7),
            Text(
              _formatPercent(percent),
              style: Theme.of(context).textTheme.titleMedium,
            ),
            Text(
              _quotaDetail(limit),
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: colors.onSurfaceVariant),
            ),
            const SizedBox(height: 8),
            ClipRRect(
              borderRadius: BorderRadius.circular(999),
              child: LinearProgressIndicator(
                value: percent / 100,
                minHeight: 6,
                backgroundColor: colors.surfaceContainerHighest,
              ),
            ),
            if (limit.usageDetails.isNotEmpty) ...[
              const SizedBox(height: 8),
              Wrap(
                spacing: 6,
                runSpacing: 4,
                children: [
                  for (final detail in limit.usageDetails)
                    Tooltip(
                      message: detail.name,
                      child: Chip(
                        visualDensity: VisualDensity.compact,
                        label: Text(
                          '${detail.name} ${_formatToolUsage(detail)}',
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    ),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _ProviderUsageMessage extends StatelessWidget {
  const _ProviderUsageMessage({
    required this.icon,
    required this.message,
    required this.tone,
  });

  final IconData icon;
  final String message;
  final _UsageTone tone;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final color = switch (tone) {
      _UsageTone.failed => colors.error,
      _UsageTone.warning => colors.tertiary,
      _UsageTone.neutral || _UsageTone.muted => colors.onSurfaceVariant,
    };
    return Row(
      children: [
        Icon(icon, size: 16, color: color),
        const SizedBox(width: 7),
        Expanded(
          child: Text(
            message,
            style: Theme.of(
              context,
            ).textTheme.bodySmall?.copyWith(color: color),
          ),
        ),
      ],
    );
  }
}

enum _UsageTone { failed, warning, neutral, muted }

class _InlineError extends StatelessWidget {
  const _InlineError({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: colors.errorContainer.withValues(alpha: 0.5),
        border: Border.all(color: colors.error.withValues(alpha: 0.35)),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Row(
          children: [
            Icon(Icons.error_outline, color: colors.error),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                message,
                style: Theme.of(
                  context,
                ).textTheme.bodySmall?.copyWith(color: colors.onErrorContainer),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _EmptySettingsMessage extends StatelessWidget {
  const _EmptySettingsMessage({
    required this.icon,
    required this.title,
    required this.body,
  });

  final IconData icon;
  final String title;
  final String body;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Material(
      color: colors.surfaceContainerLowest,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(8),
        side: BorderSide(color: colors.outlineVariant),
      ),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Row(
          children: [
            Icon(icon, color: colors.onSurfaceVariant),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(title, style: Theme.of(context).textTheme.titleSmall),
                  const SizedBox(height: 3),
                  Text(
                    body,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: colors.onSurfaceVariant,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

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
    final colors = Theme.of(context).colorScheme;
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
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Material(
                  color: colors.surfaceContainerLowest,
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(8),
                    side: BorderSide(color: colors.outlineVariant),
                  ),
                  child: ListTile(
                    dense: true,
                    leading: const Icon(Icons.smart_toy_outlined),
                    title: Text(model.displayName),
                    subtitle: Text(model.slug),
                    trailing: Text(_modelPriceLabel(model)),
                  ),
                ),
              ),
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
            LayoutBuilder(
              builder: (context, constraints) {
                final twoColumns = constraints.maxWidth >= 620;
                final fields = [
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
                ];
                if (!twoColumns) {
                  return Column(
                    children: [
                      for (final field in fields)
                        Padding(
                          padding: const EdgeInsets.only(bottom: 10),
                          child: field,
                        ),
                    ],
                  );
                }
                return Wrap(
                  spacing: 12,
                  runSpacing: 10,
                  children: [
                    for (final field in fields)
                      SizedBox(
                        width: (constraints.maxWidth - 12) / 2,
                        child: field,
                      ),
                  ],
                );
              },
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
  const _ModelReadout({required this.model});

  final ProviderModelView model;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      dense: true,
      leading: const Icon(Icons.smart_toy_outlined),
      title: Text(model.displayName),
      subtitle: Text(model.slug),
      trailing: Text(_modelPriceLabel(model)),
    );
  }
}

class _SettingsHeader extends StatelessWidget {
  const _SettingsHeader({
    required this.title,
    required this.subtitle,
    this.trailing,
  });

  final String title;
  final String subtitle;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(title, style: Theme.of(context).textTheme.titleLarge),
              if (subtitle.isNotEmpty) ...[
                const SizedBox(height: 4),
                Text(
                  subtitle,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                ),
              ],
            ],
          ),
        ),
        ?trailing,
      ],
    );
  }
}

class _SectionPanel extends StatelessWidget {
  const _SectionPanel({
    required this.title,
    required this.children,
    this.trailing,
  });

  final String title;
  final List<Widget> children;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Material(
      color: colors.surface,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(8),
        side: BorderSide(color: colors.outlineVariant),
      ),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    title,
                    style: Theme.of(context).textTheme.titleSmall,
                  ),
                ),
                ?trailing,
              ],
            ),
            const SizedBox(height: 12),
            ...children,
          ],
        ),
      ),
    );
  }
}

class _TextEdit extends StatelessWidget {
  const _TextEdit({
    required this.label,
    required this.value,
    required this.onChanged,
    this.enabled = true,
    this.obscureText = false,
  });

  final String label;
  final String value;
  final ValueChanged<String> onChanged;
  final bool enabled;
  final bool obscureText;

  @override
  Widget build(BuildContext context) {
    return TextFormField(
      initialValue: value,
      enabled: enabled,
      obscureText: obscureText,
      decoration: InputDecoration(labelText: label),
      onChanged: onChanged,
    );
  }
}

class _Readout extends StatelessWidget {
  const _Readout({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
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

class _ProviderStatusChip extends StatelessWidget {
  const _ProviderStatusChip({required this.provider});

  final ProviderSettingsView provider;

  @override
  Widget build(BuildContext context) {
    final ready = provider.status == 'ready';
    return _InfoPill(
      icon: ready ? Icons.check_circle_outline : Icons.error_outline,
      label: ready ? 'ready' : 'setup',
    );
  }
}

class _MiniMeta extends StatelessWidget {
  const _MiniMeta({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(
          icon,
          size: 14,
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
        const SizedBox(width: 4),
        Text(
          label,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(context).textTheme.bodySmall,
        ),
      ],
    );
  }
}

class _InfoPill extends StatelessWidget {
  const _InfoPill({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: colors.surfaceContainerLow,
        border: Border.all(color: colors.outlineVariant),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 5),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 14),
            const SizedBox(width: 5),
            Text(label, style: Theme.of(context).textTheme.labelSmall),
          ],
        ),
      ),
    );
  }
}

enum _ProviderDraftMode { create, edit }

class _ProviderDraft {
  const _ProviderDraft({
    required this.mode,
    required this.originalId,
    required this.provider,
  });

  factory _ProviderDraft.create(ProviderSettingsView provider) {
    return _ProviderDraft(
      mode: _ProviderDraftMode.create,
      originalId: provider.id,
      provider: provider,
    );
  }

  factory _ProviderDraft.edit(ProviderSettingsView provider) {
    return _ProviderDraft(
      mode: _ProviderDraftMode.edit,
      originalId: provider.id,
      provider: provider,
    );
  }

  final _ProviderDraftMode mode;
  final String originalId;
  final ProviderSettingsView provider;

  _ProviderDraft copyWith({ProviderSettingsView? provider}) {
    return _ProviderDraft(
      mode: mode,
      originalId: originalId,
      provider: provider ?? this.provider,
    );
  }
}

class _ProviderTemplate {
  const _ProviderTemplate({
    required this.id,
    required this.name,
    required this.baseUrl,
    required this.defaultModel,
    required this.providerKind,
    required this.defaultModels,
  });

  final String id;
  final String name;
  final String baseUrl;
  final String defaultModel;
  final String providerKind;
  final List<ProviderModelView> defaultModels;

  ProviderSettingsView createProvider(String providerId) {
    return ProviderSettingsView(
      id: providerId,
      templateKind: id,
      name: name,
      subtitle: '$name Platform',
      baseUrl: baseUrl,
      bearerToken: '',
      hasBearerToken: false,
      defaultModel: defaultModel,
      models: defaultModels,
      defaultModels: defaultModels,
      customModels: const [],
      status: 'missingCredential',
      usageLabel: '${defaultModels.length} models',
      modelCount: '${defaultModels.length}',
      updatedAt: 'Draft',
      providerKind: providerKind,
    );
  }
}

const _providerTemplates = [
  _ProviderTemplate(
    id: 'deepseek',
    name: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com',
    defaultModel: 'deepseek-v4-flash',
    providerKind: 'deep_seek',
    defaultModels: [
      ProviderModelView(
        slug: 'deepseek-v4-flash',
        displayName: 'DeepSeek V4 Flash',
        reasoningEfforts: ['high', 'max'],
        contextWindow: 1000000,
        maxOutputTokens: 384000,
        currency: 'CNY',
        inputPricePerMTok: 1,
        outputPricePerMTok: 2,
      ),
      ProviderModelView(
        slug: 'deepseek-v4-pro',
        displayName: 'DeepSeek V4 Pro',
        reasoningEfforts: ['high', 'max'],
        contextWindow: 1000000,
        maxOutputTokens: 384000,
        currency: 'CNY',
        inputPricePerMTok: 3,
        outputPricePerMTok: 6,
      ),
    ],
  ),
  _ProviderTemplate(
    id: 'openai',
    name: 'OpenAI',
    baseUrl: 'https://api.openai.com/v1',
    defaultModel: 'gpt-5.5',
    providerKind: 'open_ai',
    defaultModels: [
      ProviderModelView(
        slug: 'gpt-5.5',
        displayName: 'GPT-5.5',
        reasoningEfforts: ['medium', 'low', 'high', 'xhigh'],
      ),
      ProviderModelView(
        slug: 'gpt-5.4',
        displayName: 'GPT-5.4',
        reasoningEfforts: ['medium', 'low', 'high', 'xhigh'],
      ),
      ProviderModelView(
        slug: 'gpt-5.4-mini',
        displayName: 'GPT-5.4-Mini',
        reasoningEfforts: ['medium', 'low', 'high', 'xhigh'],
      ),
    ],
  ),
  _ProviderTemplate(
    id: 'zhipu',
    name: 'Zhipu',
    baseUrl: 'https://open.bigmodel.cn/api/paas/v4',
    defaultModel: 'glm-5.2',
    providerKind: 'zhipu',
    defaultModels: [
      ProviderModelView(
        slug: 'glm-5.2',
        displayName: 'GLM-5.2',
        reasoningEfforts: ['enabled', 'none'],
      ),
      ProviderModelView(
        slug: 'glm-5',
        displayName: 'GLM-5',
        reasoningEfforts: ['enabled', 'none'],
      ),
    ],
  ),
  _ProviderTemplate(
    id: 'zhipu-coding-plan',
    name: 'Zhipu Coding Plan',
    baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
    defaultModel: 'glm-5.2',
    providerKind: 'zhipu',
    defaultModels: [
      ProviderModelView(
        slug: 'glm-5.2',
        displayName: 'GLM-5.2',
        reasoningEfforts: ['enabled', 'none'],
      ),
      ProviderModelView(
        slug: 'glm-5',
        displayName: 'GLM-5',
        reasoningEfforts: ['enabled', 'none'],
      ),
    ],
  ),
];

String _initials(String value) {
  final words = value.trim().split(RegExp(r'\s+'));
  if (words.isEmpty || words.first.isEmpty) {
    return '?';
  }
  if (words.length == 1) {
    final word = words.first;
    return word.substring(0, word.length < 2 ? word.length : 2).toUpperCase();
  }
  return words
      .take(2)
      .map((word) => word.isEmpty ? '' : word.substring(0, 1))
      .join()
      .toUpperCase();
}

String _modelPriceLabel(ProviderModelView model) {
  if (model.currency.isEmpty ||
      model.inputPricePerMTok == null ||
      model.outputPricePerMTok == null) {
    return '';
  }
  return '${model.currency} ${_trimNumber(model.inputPricePerMTok!)}/${_trimNumber(model.outputPricePerMTok!)}';
}

String _trimNumber(double value) {
  return value
      .toStringAsFixed(value.truncateToDouble() == value ? 0 : 3)
      .replaceFirst(RegExp(r'\.?0+$'), '');
}

bool _providerSupportsUsage(ProviderSettingsView provider) {
  return provider.templateKind == 'deepseek' ||
      provider.templateKind == 'zhipu-coding-plan';
}

String _providerUsageSummary(
  ProviderSettingsView provider,
  ProviderUsageView? usage,
  bool loading,
) {
  if (loading && usage == null) {
    return 'Checking usage';
  }
  if (usage == null) {
    return _providerSupportsUsage(provider)
        ? 'Usage not loaded'
        : 'Unsupported';
  }
  return switch (usage.status) {
    'unsupported' => 'Unsupported',
    'missingCredential' => 'Missing key',
    'failed' => 'Usage failed',
    'ready' => _readyProviderUsageSummary(provider, usage),
    _ => 'Usage unavailable',
  };
}

String _readyProviderUsageSummary(
  ProviderSettingsView provider,
  ProviderUsageView usage,
) {
  if (usage.usageKind == 'deepseekBalance' && usage.balance != null) {
    final primary =
        usage.balance!.balances
            .where((item) => item.currency.toUpperCase() == 'CNY')
            .firstOrNull ??
        usage.balance!.balances.firstOrNull;
    return primary == null
        ? 'Usage unavailable'
        : '${primary.currency} ${primary.totalBalance}';
  }
  if (provider.templateKind == 'zhipu-coding-plan' &&
      usage.codingPlan != null) {
    final fiveHour = _findQuotaLimit(usage.codingPlan!.limits, 'fiveHour');
    final weekly = _findQuotaLimit(usage.codingPlan!.limits, 'weekly');
    if (fiveHour != null && weekly != null) {
      return '5h ${_formatPercent(_quotaRemainingPercent(fiveHour))} · 7d ${_formatPercent(_quotaRemainingPercent(weekly))}';
    }
  }
  return 'Usage unavailable';
}

String _providerUsageMessage(
  ProviderSettingsView provider,
  ProviderUsageView usage,
) {
  return switch (usage.status) {
    'missingCredential' =>
      usage.message ?? 'Provider API key is not configured',
    'failed' => usage.message ?? 'Usage query failed',
    'unsupported' => 'Usage is not supported for ${provider.name}',
    _ => 'Usage unavailable',
  };
}

String _usageUpdatedLabel(int? seconds) {
  if (seconds == null || seconds <= 0) {
    return 'Not checked';
  }
  return 'Updated ${_formatUnixShort(seconds)}';
}

ZhipuQuotaLimitView? _findQuotaLimit(
  List<ZhipuQuotaLimitView> limits,
  String window,
) {
  return limits.where((limit) => limit.window == window).firstOrNull;
}

double _quotaRemainingPercent(ZhipuQuotaLimitView limit) {
  final remaining = limit.remaining;
  final total = limit.total;
  if (remaining != null && total != null && total > 0) {
    return _clampPercent((remaining / total) * 100);
  }
  return _clampPercent(100 - limit.percentage);
}

String _quotaTitle(ZhipuQuotaLimitView limit) {
  return switch (limit.window) {
    'fiveHour' => '5 hour quota',
    'weekly' => 'Weekly quota',
    'mcpMonthly' => 'MCP quota',
    _ => limit.label.isEmpty ? 'Quota' : limit.label,
  };
}

String _quotaDetail(ZhipuQuotaLimitView limit) {
  final remaining = limit.remaining;
  final currentValue = limit.currentValue;
  final total = limit.total;
  if (remaining != null && total != null) {
    return '${_formatCompactNumber(remaining)} of ${_formatCompactNumber(total)} remaining';
  }
  if (currentValue != null && total != null) {
    return '${_formatCompactNumber(currentValue)} of ${_formatCompactNumber(total)} used';
  }
  return '${_formatPercent(_quotaRemainingPercent(limit))} remaining';
}

String _resetLabel(int? seconds) {
  if (seconds == null || seconds <= 0) {
    return '';
  }
  return 'Reset ${_formatUnixShort(seconds)}';
}

String _formatToolUsage(ZhipuToolUsageDetailView detail) {
  final currentValue = detail.currentValue;
  final total = detail.total;
  if (currentValue != null && total != null) {
    return '${_formatCompactNumber((total - currentValue).clamp(0, total))}/${_formatCompactNumber(total)}';
  }
  if (currentValue != null) {
    return _formatCompactNumber(currentValue);
  }
  if (detail.percentage != null) {
    return _formatPercent(_clampPercent(100 - detail.percentage!));
  }
  return '';
}

String _formatUnixShort(int seconds) {
  final date = DateTime.fromMillisecondsSinceEpoch(seconds * 1000);
  final hour = date.hour.toString().padLeft(2, '0');
  final minute = date.minute.toString().padLeft(2, '0');
  return '${date.month}/${date.day} $hour:$minute';
}

String _formatCompactNumber(num value) {
  final number = value.toDouble();
  final abs = number.abs();
  if (abs >= 1000000) {
    return '${_trimNumber(number / 1000000)}M';
  }
  if (abs >= 1000) {
    return '${_trimNumber(number / 1000)}K';
  }
  return _trimNumber(number);
}

String _formatPercent(double value) => '${_trimNumber(value)}%';

double _clampPercent(double value) {
  if (!value.isFinite) {
    return 0;
  }
  if (value < 0) {
    return 0;
  }
  if (value > 100) {
    return 100;
  }
  return value;
}

class _InstructionsTab extends ConsumerStatefulWidget {
  const _InstructionsTab({required this.settings});

  final InstructionsSettingsView settings;

  @override
  ConsumerState<_InstructionsTab> createState() => _InstructionsTabState();
}

class _InstructionsTabState extends ConsumerState<_InstructionsTab> {
  late final TextEditingController _baseController;
  late final TextEditingController _developerController;
  late final TextEditingController _userController;
  Timer? _saveTimer;
  bool _saving = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _baseController = TextEditingController(text: widget.settings.baseOverride);
    _developerController = TextEditingController(
      text: widget.settings.developer,
    );
    _userController = TextEditingController(text: widget.settings.user);
  }

  @override
  void didUpdateWidget(covariant _InstructionsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_saving) {
      return;
    }
    if (oldWidget.settings.baseOverride != widget.settings.baseOverride &&
        _baseController.text != widget.settings.baseOverride) {
      _baseController.text = widget.settings.baseOverride;
    }
    if (oldWidget.settings.developer != widget.settings.developer &&
        _developerController.text != widget.settings.developer) {
      _developerController.text = widget.settings.developer;
    }
    if (oldWidget.settings.user != widget.settings.user &&
        _userController.text != widget.settings.user) {
      _userController.text = widget.settings.user;
    }
  }

  @override
  void dispose() {
    _saveTimer?.cancel();
    _baseController.dispose();
    _developerController.dispose();
    _userController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return _SettingsPane(
      children: [
        TextField(
          controller: _baseController,
          maxLines: 8,
          decoration: const InputDecoration(
            labelText: 'Base instructions',
            prefixIcon: Icon(Icons.notes_outlined),
          ),
          onChanged: (_) => _scheduleSave(),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _developerController,
          maxLines: 8,
          decoration: const InputDecoration(
            labelText: 'Developer instructions',
            prefixIcon: Icon(Icons.code),
          ),
          onChanged: (_) => _scheduleSave(),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _userController,
          maxLines: 8,
          decoration: const InputDecoration(
            labelText: 'User context',
            prefixIcon: Icon(Icons.person_outline),
          ),
          onChanged: (_) => _scheduleSave(),
        ),
        if (_saving || _error != null) ...[
          const SizedBox(height: 12),
          if (_saving) const LinearProgressIndicator(),
          if (_error != null) _InlineError(message: _error!),
        ],
      ],
    );
  }

  void _scheduleSave() {
    _saveTimer?.cancel();
    _saveTimer = Timer(const Duration(milliseconds: 650), () {
      unawaited(_save());
    });
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .saveInstructionsSettings({
            'baseOverride': _baseController.text,
            'developer': _developerController.text,
            'user': _userController.text,
            'projectDocMaxBytes': widget.settings.projectDocMaxBytes,
            'projectDocFallbackFilenames':
                widget.settings.projectDocFallbackFilenames,
          });
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _saving = false);
      }
    }
  }
}

class _SkillsTab extends ConsumerStatefulWidget {
  const _SkillsTab({
    required this.skills,
    required this.settings,
    required this.projectId,
  });

  final List<String> skills;
  final SkillsSettingsView settings;
  final String? projectId;

  @override
  ConsumerState<_SkillsTab> createState() => _SkillsTabState();
}

class _SkillsTabState extends ConsumerState<_SkillsTab> {
  String _query = '';
  final Set<String> _discoveredSkills = {};
  bool _discovering = false;
  String? _discoverError;
  String? _saveError;

  @override
  void initState() {
    super.initState();
    _discoveredSkills.addAll(widget.skills);
  }

  @override
  void didUpdateWidget(covariant _SkillsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    _discoveredSkills.addAll(widget.skills);
  }

  @override
  Widget build(BuildContext context) {
    final skills = {...widget.skills, ..._discoveredSkills}.toList()..sort();
    final disabledSkills = widget.settings.disabled.toSet();
    final filteredSkills = skills
        .where((skill) => skill.toLowerCase().contains(_query.toLowerCase()))
        .toList();
    return _SettingsPane(
      children: [
        SearchBar(
          leading: const Icon(Icons.search),
          hintText: 'Filter skills',
          onChanged: (value) => setState(() => _query = value),
        ),
        const SizedBox(height: 14),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            for (final skill in filteredSkills)
              FilterChip(
                avatar: const Icon(Icons.extension_outlined, size: 18),
                label: Text(skill, overflow: TextOverflow.ellipsis),
                selected: !disabledSkills.contains(skill),
                onSelected: (selected) {
                  final disabled = {...disabledSkills};
                  if (selected) {
                    disabled.remove(skill);
                  } else {
                    disabled.add(skill);
                  }
                  unawaited(_saveDisabled(disabled));
                },
              ),
          ],
        ),
        if (filteredSkills.isEmpty) ...[
          const SizedBox(height: 12),
          _EmptySettingsMessage(
            icon: Icons.extension_outlined,
            title: widget.projectId == null
                ? 'Open a project to discover skills'
                : 'No skills match this filter',
            body: widget.projectId == null
                ? 'Skills are discovered from the selected workspace and configured user/system sources.'
                : 'Clear the search or run discovery again.',
          ),
        ],
        if (_discoverError != null) ...[
          const SizedBox(height: 12),
          _InlineError(message: _discoverError!),
        ],
        if (_saveError != null) ...[
          const SizedBox(height: 12),
          _InlineError(message: _saveError!),
        ],
        const SizedBox(height: 16),
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton.icon(
            icon: _discovering
                ? const SizedBox.square(
                    dimension: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.travel_explore),
            label: Text(_discovering ? 'Discovering' : 'Discover'),
            onPressed: widget.projectId == null || _discovering
                ? null
                : _discoverSkills,
          ),
        ),
      ],
    );
  }

  Future<void> _saveDisabled(Set<String> disabled) async {
    try {
      setState(() => _saveError = null);
      await ref.read(studioControllerProvider.notifier).saveSkillsSettings({
        'enabled': widget.settings.enabled,
        'autoLearn': widget.settings.autoLearn,
        'systemEnabled': widget.settings.systemEnabled,
        'projectDir': widget.settings.projectDir,
        'userDir': widget.settings.userDir,
        'externalDirs': widget.settings.externalDirs,
        'disabled': disabled.toList()..sort(),
        'autoLearnMinToolCalls': widget.settings.autoLearnMinToolCalls,
      });
    } catch (error) {
      if (mounted) {
        setState(() => _saveError = error.toString());
      }
    }
  }

  Future<void> _discoverSkills() async {
    setState(() {
      _discovering = true;
      _discoverError = null;
    });
    try {
      final skills = await ref
          .read(studioControllerProvider.notifier)
          .listDiscoveredSkills();
      if (!mounted) {
        return;
      }
      setState(() => _discoveredSkills.addAll(skills));
    } catch (error) {
      if (mounted) {
        setState(() => _discoverError = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _discovering = false);
      }
    }
  }
}

class _RolesTab extends ConsumerStatefulWidget {
  const _RolesTab({required this.providers, required this.roles});

  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;

  @override
  ConsumerState<_RolesTab> createState() => _RolesTabState();
}

class _RolesTabState extends ConsumerState<_RolesTab> {
  final Map<String, String> _selectionByRole = {};

  @override
  Widget build(BuildContext context) {
    const roles = ['explorer', 'planner', 'executor', 'reviewer'];
    final options = _roleModelOptions(widget.providers);
    return _SettingsPane(
      children: [
        for (final role in roles)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Row(
                children: [
                  const Icon(Icons.badge_outlined),
                  const SizedBox(width: 12),
                  Expanded(
                    flex: 2,
                    child: Text(
                      role,
                      style: Theme.of(context).textTheme.titleSmall,
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    flex: 3,
                    child: DropdownButtonFormField<String>(
                      initialValue: _selectedRoleModelKey(role, options),
                      decoration: const InputDecoration(labelText: 'Model'),
                      items: [
                        if (options.isEmpty)
                          const DropdownMenuItem(
                            value: 'default::default',
                            child: Text('default'),
                          )
                        else
                          for (final option in options)
                            DropdownMenuItem(
                              value: option.key,
                              child: Text(
                                option.label,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                      ],
                      onChanged: (value) {
                        if (value == null) {
                          return;
                        }
                        setState(() => _selectionByRole[role] = value);
                        final option = options.firstWhere(
                          (option) => option.key == value,
                        );
                        ref
                            .read(studioControllerProvider.notifier)
                            .setModelRole(
                              roleKey: role,
                              providerId: option.providerId,
                              model: option.model,
                              effort: option.effort,
                            );
                      },
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }

  String? _roleSelectionKey(String roleKey) {
    final role = widget.roles.where((role) => role.key == roleKey).firstOrNull;
    if (role == null || role.providerId.isEmpty || role.model.isEmpty) {
      return null;
    }
    return '${role.providerId}::${role.model}';
  }

  String _selectedRoleModelKey(String role, List<_RoleModelOption> options) {
    final selected = _selectionByRole[role];
    if (selected != null && options.any((option) => option.key == selected)) {
      return selected;
    }
    final configured = _roleSelectionKey(role);
    if (configured != null &&
        options.any((option) => option.key == configured)) {
      return configured;
    }
    return options.isEmpty ? 'default::default' : options.first.key;
  }

  List<_RoleModelOption> _roleModelOptions(
    List<ProviderSettingsView> providers,
  ) {
    final options = <_RoleModelOption>[];
    for (final provider in providers) {
      final models = provider.models.isEmpty
          ? [
              ProviderModelView(
                slug: provider.defaultModel,
                displayName: provider.defaultModel,
                reasoningEfforts: const [],
              ),
            ]
          : provider.models;
      for (final model in models) {
        if (model.slug.isEmpty) {
          continue;
        }
        options.add(
          _RoleModelOption(
            providerId: provider.id,
            model: model.slug,
            label:
                '${provider.name} / ${model.displayName.isEmpty ? model.slug : model.displayName}',
            effort: model.reasoningEfforts.firstOrNull,
          ),
        );
      }
    }
    return options;
  }
}

class _RoleModelOption {
  const _RoleModelOption({
    required this.providerId,
    required this.model,
    required this.label,
    required this.effort,
  });

  final String providerId;
  final String model;
  final String label;
  final String? effort;

  String get key => '$providerId::$model';
}

class _McpTab extends ConsumerStatefulWidget {
  const _McpTab({required this.servers});

  final List<McpServerSettingsView> servers;

  @override
  ConsumerState<_McpTab> createState() => _McpTabState();
}

class _McpTabState extends ConsumerState<_McpTab> {
  final Map<String, bool> _enabledByServer = {};
  final Map<String, String> _endpointByServer = {};
  Timer? _saveTimer;
  String? _error;

  @override
  void dispose() {
    _saveTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return _SettingsPane(
      children: [
        for (final server in widget.servers)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Column(
                children: [
                  SwitchListTile(
                    value: _enabledByServer[server.id] ?? server.enabled,
                    onChanged: (value) {
                      setState(() {
                        _enabledByServer[server.id] = value;
                      });
                      unawaited(_save());
                    },
                    secondary: const Icon(Icons.hub_outlined),
                    title: Text(server.id),
                    subtitle: Text('${server.transport}  ${server.status}'),
                  ),
                  TextFormField(
                    initialValue: server.endpoint,
                    decoration: const InputDecoration(labelText: 'Endpoint'),
                    onChanged: (value) => setState(() {
                      _endpointByServer[server.id] = value;
                      _scheduleSave();
                    }),
                  ),
                ],
              ),
            ),
          ),
        if (_error != null) _InlineError(message: _error!),
      ],
    );
  }

  void _scheduleSave() {
    _saveTimer?.cancel();
    _saveTimer = Timer(const Duration(milliseconds: 650), () {
      unawaited(_save());
    });
  }

  Future<void> _save() async {
    try {
      setState(() => _error = null);
      await ref.read(studioControllerProvider.notifier).saveMcpSettings({
        'servers': [
          for (final server in widget.servers)
            {
              'id': server.id,
              'enabled': _enabledByServer[server.id] ?? server.enabled,
              'transport': server.transport,
              'endpoint': _endpointByServer[server.id] ?? server.endpoint,
            },
        ],
      });
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }
}

class _SecurityTab extends ConsumerWidget {
  const _SecurityTab({required this.mode});

  final PermissionMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return _SettingsPane(
      maxWidth: 620,
      children: [
        SegmentedButton<PermissionMode>(
          showSelectedIcon: false,
          segments: const [
            ButtonSegment(
              value: PermissionMode.requestApproval,
              icon: Icon(Icons.verified_user_outlined),
              label: Text('Request'),
            ),
            ButtonSegment(
              value: PermissionMode.autoReview,
              icon: Icon(Icons.rule_folder_outlined),
              label: Text('Review'),
            ),
            ButtonSegment(
              value: PermissionMode.fullAccess,
              icon: Icon(Icons.lock_open_outlined),
              label: Text('Full'),
            ),
          ],
          selected: {mode},
          onSelectionChanged: (selection) {
            ref
                .read(studioControllerProvider.notifier)
                .setPermissionMode(selection.first);
          },
        ),
        const SizedBox(height: 12),
        Card(
          child: ListTile(
            leading: const Icon(Icons.security_outlined),
            title: Text(mode.name),
            subtitle: const Text('Workspace boundary policy'),
          ),
        ),
      ],
    );
  }
}

class _GeneralTab extends ConsumerStatefulWidget {
  const _GeneralTab({required this.settings});

  final GeneralSettingsView settings;

  @override
  ConsumerState<_GeneralTab> createState() => _GeneralTabState();
}

class _GeneralTabState extends ConsumerState<_GeneralTab> {
  String? _error;

  @override
  Widget build(BuildContext context) {
    return _SettingsPane(
      children: [
        SwitchListTile(
          value: widget.settings.followSystemTheme,
          onChanged: (value) =>
              _save(widget.settings.copyWith(followSystemTheme: value)),
          secondary: const Icon(Icons.dark_mode_outlined),
          title: const Text('Follow system theme'),
        ),
        SwitchListTile(
          value: widget.settings.followActiveTurn,
          onChanged: (value) =>
              _save(widget.settings.copyWith(followActiveTurn: value)),
          secondary: const Icon(Icons.vertical_align_bottom),
          title: const Text('Follow active turn'),
        ),
        SwitchListTile(
          value: widget.settings.compactTimeline,
          onChanged: (value) =>
              _save(widget.settings.copyWith(compactTimeline: value)),
          secondary: const Icon(Icons.view_agenda_outlined),
          title: const Text('Compact timeline'),
        ),
        if (_error != null) _InlineError(message: _error!),
      ],
    );
  }

  Future<void> _save(GeneralSettingsView settings) async {
    try {
      setState(() => _error = null);
      await ref.read(studioControllerProvider.notifier).saveGeneralSettings({
        'followSystemTheme': settings.followSystemTheme,
        'followActiveTurn': settings.followActiveTurn,
        'compactTimeline': settings.compactTimeline,
      });
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }
}

class _ReadonlyField extends StatelessWidget {
  const _ReadonlyField({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: TextFormField(
        initialValue: value,
        readOnly: true,
        decoration: InputDecoration(labelText: label),
      ),
    );
  }
}
