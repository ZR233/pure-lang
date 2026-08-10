class ProviderModelView {
  const ProviderModelView({
    required this.slug,
    required this.displayName,
    required this.reasoningEfforts,
    this.description = '',
    this.contextWindow,
    this.maxContextWindow,
    this.maxOutputTokens,
    this.modalities = const [],
    this.capabilities = const [],
    this.reasoningLabel = '',
    this.defaultReasoningEffort = '',
    this.currency = '',
    this.inputPricePerMTok,
    this.outputPricePerMTok,
    this.cacheReadPricePerMTok,
    this.cacheWritePricePerMTok,
    this.baseInstructions = '',
    this.wireProtocol = 'chat_completions',
    this.supportedConnectionModes = const ['http'],
    this.defaultConnectionMode = 'http',
    this.connectionMode = 'http',
  });

  final String slug;
  final String displayName;
  final List<String> reasoningEfforts;
  final String description;
  final int? contextWindow;
  final int? maxContextWindow;
  final int? maxOutputTokens;
  final List<String> modalities;
  final List<String> capabilities;
  final String reasoningLabel;
  final String defaultReasoningEffort;
  final String currency;
  final double? inputPricePerMTok;
  final double? outputPricePerMTok;
  final double? cacheReadPricePerMTok;
  final double? cacheWritePricePerMTok;
  final String baseInstructions;
  final String wireProtocol;
  final List<String> supportedConnectionModes;
  final String defaultConnectionMode;
  final String connectionMode;

  ProviderModelView copyWith({
    String? slug,
    String? displayName,
    List<String>? reasoningEfforts,
    String? description,
    int? contextWindow,
    int? maxContextWindow,
    int? maxOutputTokens,
    List<String>? modalities,
    List<String>? capabilities,
    String? reasoningLabel,
    String? defaultReasoningEffort,
    String? currency,
    double? inputPricePerMTok,
    double? outputPricePerMTok,
    double? cacheReadPricePerMTok,
    double? cacheWritePricePerMTok,
    String? baseInstructions,
    String? wireProtocol,
    List<String>? supportedConnectionModes,
    String? defaultConnectionMode,
    String? connectionMode,
  }) {
    return ProviderModelView(
      slug: slug ?? this.slug,
      displayName: displayName ?? this.displayName,
      reasoningEfforts: reasoningEfforts ?? this.reasoningEfforts,
      description: description ?? this.description,
      contextWindow: contextWindow ?? this.contextWindow,
      maxContextWindow: maxContextWindow ?? this.maxContextWindow,
      maxOutputTokens: maxOutputTokens ?? this.maxOutputTokens,
      modalities: modalities ?? this.modalities,
      capabilities: capabilities ?? this.capabilities,
      reasoningLabel: reasoningLabel ?? this.reasoningLabel,
      defaultReasoningEffort:
          defaultReasoningEffort ?? this.defaultReasoningEffort,
      currency: currency ?? this.currency,
      inputPricePerMTok: inputPricePerMTok ?? this.inputPricePerMTok,
      outputPricePerMTok: outputPricePerMTok ?? this.outputPricePerMTok,
      cacheReadPricePerMTok:
          cacheReadPricePerMTok ?? this.cacheReadPricePerMTok,
      cacheWritePricePerMTok:
          cacheWritePricePerMTok ?? this.cacheWritePricePerMTok,
      baseInstructions: baseInstructions ?? this.baseInstructions,
      wireProtocol: wireProtocol ?? this.wireProtocol,
      supportedConnectionModes:
          supportedConnectionModes ?? this.supportedConnectionModes,
      defaultConnectionMode:
          defaultConnectionMode ?? this.defaultConnectionMode,
      connectionMode: connectionMode ?? this.connectionMode,
    );
  }
}

class ProviderSettingsView {
  const ProviderSettingsView({
    required this.id,
    this.templateKind = '',
    required this.name,
    this.subtitle = '',
    required this.baseUrl,
    this.bearerToken = '',
    this.hasBearerToken = false,
    required this.defaultModel,
    required this.models,
    this.defaultModels = const [],
    this.customModels = const [],
    required this.status,
    required this.usageLabel,
    this.modelCount = '',
    this.updatedAt = '',
    this.catalogId = '',
    this.credentialLabel = 'API Key',
    this.credentialEnv = '',
    this.capabilitySource = 'explicit',
    this.hostedWebSearch = false,
    this.standaloneWebSearch = '',
    this.promptCacheDialect = 'none',
    this.responsesToolSearch = false,
    this.responsesProgrammaticToolCalling = false,
    this.iconKey,
  });

  final String id;
  final String templateKind;
  final String name;
  final String subtitle;
  final String baseUrl;
  final String bearerToken;
  final bool hasBearerToken;
  final String defaultModel;
  final List<ProviderModelView> models;
  final List<ProviderModelView> defaultModels;
  final List<ProviderModelView> customModels;
  final String status;
  final String usageLabel;
  final String modelCount;
  final String updatedAt;
  final String catalogId;
  final String credentialLabel;
  final String credentialEnv;
  final String capabilitySource;
  final bool hostedWebSearch;
  final String standaloneWebSearch;
  final String promptCacheDialect;
  final bool responsesToolSearch;
  final bool responsesProgrammaticToolCalling;
  final String? iconKey;

  List<ProviderModelView> get allModels {
    if (models.isNotEmpty) {
      return models;
    }
    return [...defaultModels, ...customModels];
  }

  ProviderSettingsView withModelConnection(String slug, String mode) {
    ProviderModelView update(ProviderModelView model) =>
        model.slug == slug ? model.copyWith(connectionMode: mode) : model;
    return copyWith(
      models: models.map(update).toList(),
      defaultModels: defaultModels.map(update).toList(),
      customModels: customModels.map(update).toList(),
    );
  }

  ProviderSettingsView copyWith({
    String? id,
    String? templateKind,
    String? name,
    String? subtitle,
    String? baseUrl,
    String? bearerToken,
    bool? hasBearerToken,
    String? defaultModel,
    List<ProviderModelView>? models,
    List<ProviderModelView>? defaultModels,
    List<ProviderModelView>? customModels,
    String? status,
    String? usageLabel,
    String? modelCount,
    String? updatedAt,
    String? catalogId,
    String? credentialLabel,
    String? credentialEnv,
    String? capabilitySource,
    bool? hostedWebSearch,
    String? standaloneWebSearch,
    String? promptCacheDialect,
    bool? responsesToolSearch,
    bool? responsesProgrammaticToolCalling,
    Object? iconKey = _providerSettingsUnset,
  }) {
    return ProviderSettingsView(
      id: id ?? this.id,
      templateKind: templateKind ?? this.templateKind,
      name: name ?? this.name,
      subtitle: subtitle ?? this.subtitle,
      baseUrl: baseUrl ?? this.baseUrl,
      bearerToken: bearerToken ?? this.bearerToken,
      hasBearerToken: hasBearerToken ?? this.hasBearerToken,
      defaultModel: defaultModel ?? this.defaultModel,
      models: models ?? this.models,
      defaultModels: defaultModels ?? this.defaultModels,
      customModels: customModels ?? this.customModels,
      status: status ?? this.status,
      usageLabel: usageLabel ?? this.usageLabel,
      modelCount: modelCount ?? this.modelCount,
      updatedAt: updatedAt ?? this.updatedAt,
      catalogId: catalogId ?? this.catalogId,
      credentialLabel: credentialLabel ?? this.credentialLabel,
      credentialEnv: credentialEnv ?? this.credentialEnv,
      capabilitySource: capabilitySource ?? this.capabilitySource,
      hostedWebSearch: hostedWebSearch ?? this.hostedWebSearch,
      standaloneWebSearch: standaloneWebSearch ?? this.standaloneWebSearch,
      promptCacheDialect: promptCacheDialect ?? this.promptCacheDialect,
      responsesToolSearch: responsesToolSearch ?? this.responsesToolSearch,
      responsesProgrammaticToolCalling:
          responsesProgrammaticToolCalling ??
          this.responsesProgrammaticToolCalling,
      iconKey: identical(iconKey, _providerSettingsUnset)
          ? this.iconKey
          : iconKey as String?,
    );
  }
}

const _providerSettingsUnset = Object();

class ProviderCatalogView {
  const ProviderCatalogView({
    required this.schemaVersion,
    required this.revision,
    required this.presets,
    required this.modelCatalogs,
  });

  const ProviderCatalogView.empty()
    : schemaVersion = 0,
      revision = '',
      presets = const [],
      modelCatalogs = const {};

  final int schemaVersion;
  final String revision;
  final List<ProviderPresetView> presets;
  final Map<String, List<ProviderModelView>> modelCatalogs;

  ProviderPresetView? preset(String id) {
    for (final preset in presets) {
      if (preset.id == id) return preset;
    }
    return null;
  }

  List<ProviderModelView> modelsFor(String catalogId) {
    return modelCatalogs[catalogId] ?? const [];
  }
}

class ProviderPresetView {
  const ProviderPresetView({
    required this.id,
    required this.displayName,
    required this.description,
    required this.baseUrl,
    required this.credentialLabel,
    required this.credentialEnv,
    required this.modelCatalogId,
    required this.suggestedModel,
    this.hostedWebSearch = false,
    this.standaloneWebSearch = '',
    this.promptCacheDialect = 'none',
    this.responsesToolSearch = false,
    this.responsesProgrammaticToolCalling = false,
    this.iconKey,
  });

  final String id;
  final String displayName;
  final String description;
  final String baseUrl;
  final String credentialLabel;
  final String credentialEnv;
  final String modelCatalogId;
  final String suggestedModel;
  final bool hostedWebSearch;
  final String standaloneWebSearch;
  final String promptCacheDialect;
  final bool responsesToolSearch;
  final bool responsesProgrammaticToolCalling;
  final String? iconKey;

  ProviderSettingsView createProvider(
    String providerId,
    List<ProviderModelView> models,
  ) {
    return ProviderSettingsView(
      id: providerId,
      templateKind: id,
      name: displayName,
      subtitle: description,
      baseUrl: baseUrl,
      bearerToken: '',
      hasBearerToken: false,
      defaultModel: suggestedModel,
      models: models,
      defaultModels: models,
      customModels: const [],
      status: 'missingCredential',
      usageLabel: '${models.length} models',
      modelCount: '${models.length}',
      updatedAt: 'Draft',
      catalogId: modelCatalogId,
      credentialLabel: credentialLabel,
      credentialEnv: credentialEnv,
      capabilitySource: 'preset_defaults',
      hostedWebSearch: hostedWebSearch,
      standaloneWebSearch: standaloneWebSearch,
      promptCacheDialect: promptCacheDialect,
      responsesToolSearch: responsesToolSearch,
      responsesProgrammaticToolCalling: responsesProgrammaticToolCalling,
      iconKey: iconKey,
    );
  }
}

class ProviderConnectionModeView {
  const ProviderConnectionModeView({
    required this.id,
    required this.displayName,
  });

  final String id;
  final String displayName;
}

ProviderSettingsView providerWithCatalogMetadata(
  ProviderSettingsView provider,
  ProviderCatalogView catalog,
) {
  final preset = catalog.preset(provider.templateKind);
  if (preset == null) return provider;
  final bundledModels = catalog.modelsFor(preset.modelCatalogId);
  final effectiveModels = [...bundledModels, ...provider.customModels];
  return provider.copyWith(
    subtitle: provider.subtitle.isEmpty
        ? preset.description
        : provider.subtitle,
    catalogId: preset.modelCatalogId,
    credentialLabel: preset.credentialLabel,
    credentialEnv: preset.credentialEnv,
    iconKey: preset.iconKey,
    defaultModels: provider.defaultModels.isEmpty
        ? bundledModels
        : provider.defaultModels,
    models: provider.models.isEmpty ? effectiveModels : provider.models,
  );
}

class ProviderUsageView {
  const ProviderUsageView({
    required this.providerId,
    required this.updatedAt,
    required this.status,
    required this.usageKind,
    this.message,
    this.balance,
    this.codingPlan,
  });

  final String providerId;
  final int updatedAt;
  final String status;
  final String usageKind;
  final String? message;
  final DeepSeekBalanceUsageView? balance;
  final ZhipuCodingPlanUsageView? codingPlan;
}

class DeepSeekBalanceUsageView {
  const DeepSeekBalanceUsageView({
    required this.isAvailable,
    required this.balances,
  });

  final bool isAvailable;
  final List<DeepSeekBalanceInfoView> balances;
}

class DeepSeekBalanceInfoView {
  const DeepSeekBalanceInfoView({
    required this.currency,
    required this.totalBalance,
    required this.grantedBalance,
    required this.toppedUpBalance,
  });

  final String currency;
  final String totalBalance;
  final String grantedBalance;
  final String toppedUpBalance;
}

class ZhipuCodingPlanUsageView {
  const ZhipuCodingPlanUsageView({this.level, required this.limits});

  final String? level;
  final List<ZhipuQuotaLimitView> limits;
}

class ZhipuQuotaLimitView {
  const ZhipuQuotaLimitView({
    required this.window,
    required this.label,
    required this.percentage,
    this.currentValue,
    this.total,
    this.remaining,
    this.nextResetAt,
    required this.usageDetails,
  });

  final String window;
  final String label;
  final double percentage;
  final double? currentValue;
  final double? total;
  final double? remaining;
  final int? nextResetAt;
  final List<ZhipuToolUsageDetailView> usageDetails;
}

class ZhipuToolUsageDetailView {
  const ZhipuToolUsageDetailView({
    required this.name,
    this.currentValue,
    this.total,
    this.percentage,
  });

  final String name;
  final double? currentValue;
  final double? total;
  final double? percentage;
}
