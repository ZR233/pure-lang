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
    this.baseInstructions = '',
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
  final String baseInstructions;

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
    String? baseInstructions,
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
      baseInstructions: baseInstructions ?? this.baseInstructions,
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
    this.wireProtocol = '',
    this.connectionMode = 'http',
    this.catalogId = '',
    this.credentialLabel = 'API Key',
    this.credentialEnv = '',
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
  final String wireProtocol;
  final String connectionMode;
  final String catalogId;
  final String credentialLabel;
  final String credentialEnv;
  final String? iconKey;

  List<ProviderModelView> get allModels {
    if (models.isNotEmpty) {
      return models;
    }
    return [...defaultModels, ...customModels];
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
    String? wireProtocol,
    String? connectionMode,
    String? catalogId,
    String? credentialLabel,
    String? credentialEnv,
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
      wireProtocol: wireProtocol ?? this.wireProtocol,
      connectionMode: connectionMode ?? this.connectionMode,
      catalogId: catalogId ?? this.catalogId,
      credentialLabel: credentialLabel ?? this.credentialLabel,
      credentialEnv: credentialEnv ?? this.credentialEnv,
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
    required this.wireProtocol,
    required this.connectionModes,
    required this.defaultConnectionMode,
    required this.baseUrl,
    required this.credentialLabel,
    required this.credentialEnv,
    required this.modelCatalogId,
    required this.suggestedModel,
    this.iconKey,
  });

  final String id;
  final String displayName;
  final String description;
  final String wireProtocol;
  final List<ProviderConnectionModeView> connectionModes;
  final String defaultConnectionMode;
  final String baseUrl;
  final String credentialLabel;
  final String credentialEnv;
  final String modelCatalogId;
  final String suggestedModel;
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
      wireProtocol: wireProtocol,
      connectionMode: defaultConnectionMode,
      catalogId: modelCatalogId,
      credentialLabel: credentialLabel,
      credentialEnv: credentialEnv,
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
    wireProtocol: preset.wireProtocol,
    connectionMode:
        preset.connectionModes.any((mode) => mode.id == provider.connectionMode)
        ? provider.connectionMode
        : preset.defaultConnectionMode,
    defaultModels: bundledModels,
    models: effectiveModels,
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
