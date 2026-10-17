import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../presentation/providers/client_provider.dart';
import '../../presentation/providers/provider_usage_queries.dart';

import 'package:anywork_client/studio_client.dart';

import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_provider_details.dart';
import 'settings_provider_draft_save.dart';
import 'settings_provider_drafts.dart';
import 'settings_provider_editor.dart';
import 'settings_provider_list.dart';
import 'settings_provider_usage_demand.dart';
import 'settings_scope.dart';
import 'settings_views.dart';

class ProvidersTab extends ConsumerStatefulWidget {
  const ProvidersTab({super.key, required this.tabIndex});

  /// Tab visibility controls display observation; Client retains its topic owner.
  final int tabIndex;

  @override
  ConsumerState<ProvidersTab> createState() => _ProvidersTabState();
}

class _ProvidersTabState extends ConsumerState<ProvidersTab> {
  String _query = '';
  String? _selectedProviderId;
  ProviderDraft? _draft;
  GlobalKey<FormState> _draftFormKey = GlobalKey<FormState>();
  bool _showDetails = false;
  String? _draftError;
  final ProviderDraftSaveSession _saveSession = ProviderDraftSaveSession();
  final ProviderUsageAutoDemand _usageDemand = ProviderUsageAutoDemand();
  ProviderSubscription<bool>? _visibilitySubscription;
  ProviderSubscription<ProviderUsageQueryIdentitySnapshot>?
  _identitySubscription;
  late final AnyworkClient _client;
  final Map<(String, String), _AutoCompactIntent> _compactIntents = {};
  bool _disposing = false;

  SettingsProvidersView get _settings =>
      ref.read(settingsProvidersViewProvider);

  bool get _visible => ref.read(settingsVisibleTabProvider) == widget.tabIndex;

  @override
  void initState() {
    super.initState();
    _client = ref.read(anyworkClientProvider);
    _visibilitySubscription = ref.listenManual<bool>(
      settingsVisibleTabProvider.select((index) => index == widget.tabIndex),
      (previous, next) {
        if (!next) _flushCompactIntents();
        _scheduleUsageDemand();
      },
      fireImmediately: true,
    );
    _identitySubscription = ref
        .listenManual<ProviderUsageQueryIdentitySnapshot>(
          settingsProviderUsageQueryIdentitiesProvider,
          (previous, next) => _scheduleUsageDemand(),
          fireImmediately: true,
        );
  }

  @override
  void dispose() {
    _disposing = true;
    _flushCompactIntents();
    _visibilitySubscription?.close();
    _identitySubscription?.close();
    super.dispose();
  }

  void _changeCompact(String providerId, String slug, int? value) {
    final key = (providerId, slug);
    final current = _compactIntents[key];
    if (current != null && current.value == value && !current.failed) return;
    _compactIntents[key]?.timer?.cancel();
    final intent = _AutoCompactIntent(value);
    setState(() => _compactIntents[key] = intent);
    intent.timer = Timer(
      const Duration(milliseconds: 650),
      () => _submitCompact(key, intent),
    );
  }

  void _invalidateCompact(String providerId, String slug) {
    final key = (providerId, slug);
    final intent = _compactIntents.remove(key);
    intent?.timer?.cancel();
    if (intent != null) setState(() {});
  }

  void _commitCompact(String providerId, String slug) {
    final key = (providerId, slug);
    final intent = _compactIntents[key];
    if (intent == null) return;
    if (intent.failed) {
      final retry = _AutoCompactIntent(intent.value);
      setState(() => _compactIntents[key] = retry);
      _submitCompact(key, retry);
    } else if (!intent.submitted) {
      _submitCompact(key, intent);
    }
  }

  void _flushCompactIntents() {
    for (final entry in _compactIntents.entries.toList()) {
      if (!entry.value.submitted && !entry.value.failed) {
        _submitCompact(entry.key, entry.value);
      }
    }
  }

  void _submitCompact((String, String) key, _AutoCompactIntent intent) {
    if (intent.submitted) return;
    intent.timer?.cancel();
    intent.timer = null;
    intent.submitted = true;
    // Client owns the queued command and joins it even after this page leaves.
    unawaited(
      _client.settingsCommands
          .setModelAutoCompactLimit(key.$1, key.$2, intent.value)
          .then<void>(
            (_) {
              if (_disposing ||
                  !mounted ||
                  !identical(_compactIntents[key], intent)) {
                return;
              }
              setState(() => _compactIntents.remove(key));
            },
            onError: (Object error, StackTrace stack) {
              if (_disposing ||
                  !mounted ||
                  !identical(_compactIntents[key], intent)) {
                return;
              }
              setState(() => intent.failed = true);
            },
          ),
    );
  }

  void _scheduleUsageDemand() {
    scheduleMicrotask(_syncUsageDemand);
  }

  void _syncUsageDemand() {
    if (!mounted) return;
    final queries = ref.read(anyworkClientProvider).providerUsageQueries;
    _usageDemand.apply(
      visible: _visible,
      providers: ref
          .read(settingsProviderUsageQueryIdentitiesProvider)
          .identities,
      refresh: (providerId) =>
          ignoreProviderUsageQueryErrors(queries.refresh(providerId)),
    );
  }

  @override
  void didUpdateWidget(covariant ProvidersTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    final selected = _selectedProviderId;
    if (selected != null &&
        !_settings.providers.any((provider) => provider.id == selected)) {
      _selectedProviderId = _settings.providers.firstOrNull?.id;
      _showDetails = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    // Visibility only drives display observation; Client retains the topic.
    final visible = ref.watch(settingsVisibleTabProvider) == widget.tabIndex;
    watchSettingsScope(
      ref,
      visible: visible,
      kind: BridgeSettingsScopeKind.usage,
    );
    final settings = ref.watch(settingsProvidersViewProvider);
    final catalog = settings.providerCatalog;
    final defaultProviderId = settings.defaultProviderId;
    final selectedId =
        _selectedProviderId ??
        defaultProviderId ??
        _settings.providers.firstOrNull?.id;
    final selected = _settings.providers
        .where((provider) => provider.id == selectedId)
        .firstOrNull;
    final filtered = _filteredProviders();
    final usageView = ref.watch(settingsUsageViewProvider);
    final canonicalUsages = usageView.usages;
    final usageByProvider = {
      for (final usage in canonicalUsages) usage.providerId: usage,
    };
    final queryState = ref.watch(providerUsageQueriesProvider);
    final usageErrorsByProviderId = queryState.errorsByProviderId;
    final loadingProviderIds = queryState.loadingProviderIds;
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
              formKey: _draftFormKey,
              draft: _draft!,
              presets: catalog.presets,
              saving: _saveSession.isSaving,
              error: _draftError,
              onCancel: () => setState(() {
                _saveSession.abandon();
                _draft = null;
                _draftError = null;
              }),
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
    final details = ProviderDetails(
      provider: selected,
      usage: selected == null ? null : usageByProvider[selected.id],
      usageLoading:
          selected != null && loadingProviderIds.contains(selected.id),
      usageError: selected == null
          ? null
          : usageErrorsByProviderId[selected.id],
      onBack: () {
        _flushCompactIntents();
        setState(() => _showDetails = false);
      },
      onEdit: _startEdit,
      autoCompactEdits: {
        for (final entry in _compactIntents.entries)
          if (entry.key.$1 == selectedId)
            entry.key.$2: (
              overrideLimit: entry.value.value,
              pending: !entry.value.failed,
              failed: entry.value.failed,
            ),
      },
      onAutoCompactChanged: (slug, value) =>
          _changeCompact(selectedId!, slug, value),
      onAutoCompactCommit: (slug) => _commitCompact(selectedId!, slug),
      onAutoCompactInvalid: (slug) => _invalidateCompact(selectedId!, slug),
      onRemoveManualModel: selected == null
          ? (_) {}
          : (model) => unawaited(_removeManualModel(selected, model)),
      onRefreshUsage: selected == null
          ? null
          : () => unawaited(_refreshUsages(providerId: selected.id)),
    );
    final list = ProviderList(
      providers: filtered,
      selectedProviderId: _showDetails ? selectedId : null,
      defaultProviderId: defaultProviderId,
      filtering: _query.trim().isNotEmpty,
      usageByProvider: usageByProvider,
      loadingProviderIds: loadingProviderIds,
      usageErrorsByProviderId: usageErrorsByProviderId,
      onQueryChanged: (value) => setState(() => _query = value),
      onAdd: _startAdd,
      onSelect: (provider) {
        _flushCompactIntents();
        setState(() {
          _selectedProviderId = provider.id;
          _showDetails = true;
        });
      },
      onSetDefault: _setDefaultProvider,
      onRefreshAll: _refreshUsages,
      onRefreshProvider: (provider) =>
          unawaited(_refreshUsages(providerId: provider.id)),
      onEdit: _startEdit,
      onDelete: _settings.providers.length <= 1 ? null : _removeProvider,
    );
    return LayoutBuilder(
      builder: (context, constraints) {
        final split = constraints.maxWidth >= 1100 && _showDetails;
        return Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (visible)
                SettingsTopicStatus(topic: const ProviderUsageTopic()),
              Expanded(
                child: split
                    ? Row(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          SizedBox(width: 360, child: list),
                          const SizedBox(width: 24),
                          const VerticalDivider(width: 1),
                          const SizedBox(width: 24),
                          Expanded(child: details),
                        ],
                      )
                    : _showDetails
                    ? details
                    : list,
              ),
            ],
          ),
        );
      },
    );
  }

  List<ProviderSettingsView> _filteredProviders() {
    final query = _query.trim().toLowerCase();
    if (query.isEmpty) {
      return _settings.providers;
    }
    return _settings.providers.where((provider) {
      final haystack = [
        provider.name,
        provider.id,
        provider.defaultModel,
        for (final model in provider.models) model.slug,
        for (final model in provider.models) model.displayName,
        for (final model in provider.models) model.wireProtocol,
        for (final model in provider.models) model.connectionMode,
      ].join(' ').toLowerCase();
      return haystack.contains(query);
    }).toList();
  }

  void _startAdd() {
    _flushCompactIntents();
    final catalog = ref.read(settingsProvidersViewProvider).providerCatalog;
    final template = catalog.presets.firstOrNull;
    if (template == null) {
      setState(
        () => _draftError = context.l10n.settingsProviderCatalogUnavailable,
      );
      return;
    }
    final id = _suggestProviderId(template.id);
    final draft = ProviderDraftFactory.create(
      catalog: catalog,
      templateId: template.id,
      providerId: id,
    );
    if (draft == null) {
      setState(
        () => _draftError = context.l10n.settingsProviderPresetUnavailable,
      );
      return;
    }
    setState(() {
      _saveSession.abandon();
      _selectedProviderId = id;
      _showDetails = false;
      _draft = draft;
      _draftFormKey = GlobalKey<FormState>();
      _draftError = null;
    });
  }

  Future<void> _startEdit(ProviderSettingsView provider) async {
    _flushCompactIntents();
    await _client.settingsCommands.enqueueSettingsWrite(() async {});
    if (!mounted) return;
    final latest = _settings.providers
        .where((item) => item.id == provider.id)
        .firstOrNull;
    if (latest == null) return;
    setState(() {
      _saveSession.abandon();
      _selectedProviderId = provider.id;
      _showDetails = false;
      _draft = ProviderDraft.edit(latest);
      _draftFormKey = GlobalKey<FormState>();
      _draftError = null;
    });
  }

  void _changeDraftTemplate(String templateId) {
    final catalog = ref.read(settingsProvidersViewProvider).providerCatalog;
    final current = _draft;
    if (current == null || current.provider.templateKind == templateId) {
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
    setState(() {
      _draft = next;
      // A retained GlobalKey would reparent the old Form under the new
      // ProviderEditor and preserve its text, validation and secret fields.
      _draftFormKey = GlobalKey<FormState>();
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
      final existing = provider.models.map((model) => model.slug).toSet();
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
        origin: ProviderModelOrigin.manual,
      );
      return provider.copyWith(
        models: [...provider.models, model],
        defaultModel: ModelRouteQueries.defaultModelAfterAddition(
          provider.defaultModel,
          slug,
        ),
      );
    });
  }

  void _updateCustomModel(int index, ProviderModelView model) {
    _updateDraft((provider) {
      final previousModel = provider.customModels[index];
      final previousSlug = previousModel.slug;
      final connections = {...provider.modelConnectionModes};
      if (previousSlug != model.slug ||
          previousModel.wireProtocol != model.wireProtocol) {
        connections.remove(previousSlug);
      }
      // 保留既有用户覆盖：同名编辑不得丢覆盖，仅更名时迁移；上下文窗口变化时按新容量
      // 重算派生默认、安全上限与生效值，保留用户覆盖。
      final limits = {...provider.autoCompactLimits};
      final recalculated = ProviderModelAutoCompactView.forModel(model)
          .withOverride(limits[previousSlug]?.overrideLimit);
      if (previousSlug != model.slug) {
        limits.remove(previousSlug);
      }
      limits[model.slug] = recalculated;
      final models = [
        for (final candidate in provider.models)
          if (candidate.slug == previousSlug)
            model.copyWith(origin: ProviderModelOrigin.manual)
          else
            candidate,
      ];
      return provider.copyWith(
        modelConnectionModes: connections,
        autoCompactLimits: limits,
        defaultModel: provider.defaultModel == previousSlug
            ? model.slug
            : provider.defaultModel,
        models: models,
      );
    });
  }

  void _removeCustomModel(int index) {
    _updateDraft((provider) {
      final custom = [...provider.customModels];
      final removedSlug = custom[index].slug;
      custom.removeAt(index);
      final models = [
        for (final candidate in provider.models)
          if (candidate.slug != removedSlug) candidate,
      ];
      final limits = {...provider.autoCompactLimits}..remove(removedSlug);
      final connections = {...provider.modelConnectionModes}
        ..remove(removedSlug);
      // 仅当被移除的模型正是当前默认模型时才改写，避免覆盖无法解析的
      // canonical defaultModel。
      final defaultModel = ModelRouteQueries.defaultModelAfterRemoval(
        current: provider.defaultModel,
        removed: removedSlug,
        remaining: models,
      );
      return provider.copyWith(
        modelConnectionModes: connections,
        autoCompactLimits: limits,
        models: models,
        defaultModel: defaultModel,
      );
    });
  }

  Future<void> _removeManualModel(
    ProviderSettingsView provider,
    ProviderModelView model,
  ) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.l10n.settingsRemoveManualModelConfirmTitle),
        content: Text(context.l10n.settingsRemoveManualModelConfirmBody),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(context.l10n.settingsCancel),
          ),
          FilledButton(
            key: StudioDriverKeys.providerManualModelDeleteConfirm,
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(context.l10n.settingsRemoveManualModelConfirmAction),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    try {
      await _client.settingsCommands.removeProviderModel(
        provider.id,
        model.slug,
      );
    } catch (_) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(context.l10n.settingsRemoveManualModelFailed)),
      );
    }
  }

  Future<void> _saveDraft() async {
    final current = _draft;
    if (current == null || _saveSession.isSaving) {
      return;
    }
    if (_draftFormKey.currentState?.validate() == false) {
      return;
    }
    final epoch = _saveSession.begin();
    setState(() => _draftError = null);
    try {
      final provider = ProviderCommandBuilder.normalizeProvider(
        current.provider,
      );
      final makeDefault =
          current.mode == ProviderDraftMode.create ||
          ref.read(settingsProvidersViewProvider).defaultProviderId ==
              current.originalId;
      final saved = await ref
          .read(anyworkClientProvider)
          .settingsCommands
          .saveProvider(
            ProviderCommandBuilder.buildProvider(
              provider,
              originalId: current.mode == ProviderDraftMode.edit
                  ? current.originalId
                  : null,
            ),
          );
      if (!saved.providers.any((candidate) => candidate.id == provider.id)) {
        throw StateError(
          'Provider save returned a canonical snapshot without ${provider.id}.',
        );
      }
      if (makeDefault) {
        await ref
            .read(anyworkClientProvider)
            .settingsCommands
            .setDefaultProvider(provider.id);
      }
      if (!_saveSession.owns(epoch)) {
        return;
      }
      _saveSession.succeed(epoch);
      if (mounted) {
        setState(() {
          _selectedProviderId = provider.id;
          _draft = null;
          _showDetails = false;
        });
      }
    } catch (error) {
      if (_saveSession.owns(epoch)) {
        _saveSession.fail(epoch);
        if (mounted) {
          setState(() => _draftError = error.toString());
        }
      }
    }
  }

  Future<void> _removeProvider(ProviderSettingsView provider) async {
    if (_settings.providers.length <= 1) {
      return;
    }
    final providers = _canonicalProviders
        .where((candidate) => candidate.id != provider.id)
        .toList();
    final currentDefaultId = ref
        .read(settingsProvidersViewProvider)
        .defaultProviderId;
    final selectedProviderId = ModelRouteQueries.providerAfterRemoval(
      providers: providers,
      currentDefault: currentDefaultId,
      removed: provider.id,
    );
    final saved = await ref
        .read(anyworkClientProvider)
        .settingsCommands
        .removeProvider(provider.id, replacementProviderId: selectedProviderId);
    if (saved.providers.any((candidate) => candidate.id == provider.id)) {
      throw StateError(
        'Provider removal returned a canonical snapshot that still contains '
        '${provider.id}.',
      );
    }
    if (mounted) {
      setState(() {
        _selectedProviderId = selectedProviderId;
        _showDetails = false;
        if (_draft?.originalId == provider.id) {
          _draft = null;
        }
      });
    }
    ref.read(anyworkClientProvider).providerUsageQueries.forget(provider.id);
  }

  Future<void> _setDefaultProvider(ProviderSettingsView provider) async {
    setState(() => _selectedProviderId = provider.id);
    await ref
        .read(anyworkClientProvider)
        .settingsCommands
        .setDefaultProvider(provider.id);
    final saved = ref.read(settingsProvidersViewProvider);
    if (saved.canonical == null || saved.defaultProviderId != provider.id) {
      throw StateError(
        'Provider default save returned ${saved.defaultProviderId}, '
        'expected ${provider.id}.',
      );
    }
  }

  Future<void> _refreshUsages({String? providerId}) async {
    final queries = ref.read(anyworkClientProvider).providerUsageQueries;
    if (providerId == null) {
      await queries.refreshAll();
      return;
    }
    ignoreProviderUsageQueryErrors(queries.refresh(providerId));
  }

  List<ProviderSettingsView> get _canonicalProviders => _settings.providers;

  String _suggestProviderId(String base) {
    final existing = _canonicalProviders.map((provider) => provider.id).toSet();
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

/// 对象身份隔离旧完成通知；不把原始文本、焦点或 canonical 数据放进提交状态。
class _AutoCompactIntent {
  _AutoCompactIntent(this.value);

  final int? value;
  Timer? timer;
  bool submitted = false;
  bool failed = false;
}
