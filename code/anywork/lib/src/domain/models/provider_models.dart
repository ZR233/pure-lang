enum ModelModalityView { text, image, audio, video, file }

enum ModelInputSourceView { local, remoteUrl }

class ModelInputCapabilityView {
  const ModelInputCapabilityView({
    required this.modality,
    required this.sources,
    this.maxCount,
    this.maxBytes,
    this.maxTotalBytes,
    this.maxWidth,
    this.maxHeight,
    this.mediaTypes = const [],
  });

  final ModelModalityView modality;
  final List<ModelInputSourceView> sources;
  final int? maxCount;
  final int? maxBytes;
  final int? maxTotalBytes;
  final int? maxWidth;
  final int? maxHeight;
  final List<String> mediaTypes;

  bool supportsSource(ModelInputSourceView source) => sources.contains(source);
}

class ProviderPriceTierView {
  const ProviderPriceTierView({
    required this.label,
    required this.input,
    required this.output,
    this.cacheRead,
    this.cacheWrite,
  });
  final String label;
  final double input;
  final double output;
  final double? cacheRead;
  final double? cacheWrite;
}

class ProviderModelView {
  const ProviderModelView({
    required this.slug,
    required this.displayName,
    required this.reasoningEfforts,
    this.description = '',
    this.contextWindow,
    this.maxContextWindow,
    this.maxOutputTokens,
    this.inputCapabilities = const [],
    this.outputModalities = const [],
    this.capabilities = const [],
    this.reasoningLabel = '',
    this.defaultReasoningEffort = '',
    this.currency = '',
    this.priceTiers = const [],
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
  final List<ModelInputCapabilityView> inputCapabilities;
  final List<ModelModalityView> outputModalities;
  final List<String> capabilities;
  final String reasoningLabel;
  final String defaultReasoningEffort;
  final String currency;
  final List<ProviderPriceTierView> priceTiers;
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
    List<ModelInputCapabilityView>? inputCapabilities,
    List<ModelModalityView>? outputModalities,
    List<String>? capabilities,
    String? reasoningLabel,
    String? defaultReasoningEffort,
    String? currency,
    List<ProviderPriceTierView>? priceTiers,
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
      inputCapabilities: inputCapabilities ?? this.inputCapabilities,
      outputModalities: outputModalities ?? this.outputModalities,
      capabilities: capabilities ?? this.capabilities,
      reasoningLabel: reasoningLabel ?? this.reasoningLabel,
      defaultReasoningEffort:
          defaultReasoningEffort ?? this.defaultReasoningEffort,
      currency: currency ?? this.currency,
      priceTiers: priceTiers ?? this.priceTiers,
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

/// 模型未显式声明默认压缩阈值时使用的十进制 token 数（258k）。
///
/// 与 pl-model 的 `DEFAULT_AUTO_COMPACT_TOKEN_LIMIT` 保持一致；草稿中尚未由服务端解析的
/// 模型（新建 provider 或新增自定义模型）用它作为默认阈值展示，不写入保存请求。
const int kDefaultAutoCompactTokens = 258000;

/// provider 实例中某模型上下文压缩阈值的三层视图。
///
/// `defaultLimit` 为模型默认元信息；`overrideLimit` 为用户覆盖（`null` 表示使用默认）；
/// `effectiveLimit` 为实际生效值 `min(覆盖或默认, 上下文 90%)`，上下文未知时为 `null`；
/// `safeLimit` 为上下文 90% 安全上限，上下文未知时为 `null`。
class ProviderModelAutoCompactView {
  const ProviderModelAutoCompactView({
    required this.slug,
    required this.defaultLimit,
    this.overrideLimit,
    this.effectiveLimit,
    this.safeLimit,
  });

  /// 由模型草稿推导的默认视图：默认阈值 258k，上下文 90% 为安全上限。
  factory ProviderModelAutoCompactView.forModel(ProviderModelView model) {
    final safeLimit = safeLimitForContext(model.contextWindow);
    return ProviderModelAutoCompactView(
      slug: model.slug,
      defaultLimit: kDefaultAutoCompactTokens,
      overrideLimit: null,
      effectiveLimit: _effective(kDefaultAutoCompactTokens, safeLimit),
      safeLimit: safeLimit,
    );
  }

  /// 上下文容量 90% 的安全上限；容量未知或非正时返回 `null`（保持不自动压缩）。
  ///
  /// 用商余数计算，避免 `contextWindow * 90` 在大容量下的整数溢出。
  static int? safeLimitForContext(int? contextWindow) {
    if (contextWindow == null || contextWindow <= 0) {
      return null;
    }
    final quotient = contextWindow ~/ 100;
    final remainder = contextWindow % 100;
    return quotient * 90 + (remainder * 90) ~/ 100;
  }

  static int? _effective(int selected, int? safeLimit) {
    if (safeLimit == null) {
      return null;
    }
    return selected < safeLimit ? selected : safeLimit;
  }

  final String slug;
  final int defaultLimit;
  final int? overrideLimit;
  final int? effectiveLimit;
  final int? safeLimit;

  bool get hasOverride => overrideLimit != null;

  /// 返回覆盖变更后的副本；覆盖为 `null` 表示恢复默认。
  ///
  /// 实际生效值按 `min(覆盖或默认, 安全上限)` 重新推导，上下文未知时保持 `null`。
  ProviderModelAutoCompactView withOverride(int? overrideLimit) {
    return ProviderModelAutoCompactView(
      slug: slug,
      defaultLimit: defaultLimit,
      overrideLimit: overrideLimit,
      effectiveLimit: _effective(overrideLimit ?? defaultLimit, safeLimit),
      safeLimit: safeLimit,
    );
  }

  /// 以新的上下文安全上限重算生效值，保留模型默认与用户覆盖。
  ProviderModelAutoCompactView withSafeLimit(int? safeLimit) {
    return ProviderModelAutoCompactView(
      slug: slug,
      defaultLimit: defaultLimit,
      overrideLimit: overrideLimit,
      effectiveLimit: _effective(overrideLimit ?? defaultLimit, safeLimit),
      safeLimit: safeLimit,
    );
  }
}

class ProviderSettingsView {
  const ProviderSettingsView({
    this.pricingEnabled = false,
    required this.id,
    this.templateKind = '',
    required this.name,
    this.subtitle = '',
    required this.baseUrl,
    this.bearerToken = '',
    this.hasBearerToken = false,
    this.credentialRequired = true,
    required this.defaultModel,
    required this.models,
    this.defaultModels = const [],
    this.customModels = const [],
    this.modelConnectionModes = const {},
    this.autoCompactLimits = const {},
    required this.status,
    required this.usageLabel,
    this.modelCount = '',
    this.updatedAt = '',
    this.catalogId = '',
    this.credentialLabel = 'API Key',
    this.credentialEnv = '',
    this.capabilitySource = 'explicit',
    this.hostedWebSearch = false,
    this.hostedWebSearchDialect = 'open_ai_responses',
    this.standaloneWebSearch = '',
    this.promptCacheDialect = 'none',
    this.responsesProgrammaticToolCalling = false,
    this.iconKey,
  });

  final bool pricingEnabled;
  final String id;
  final String templateKind;
  final String name;
  final String subtitle;
  final String baseUrl;
  final String bearerToken;
  final bool hasBearerToken;
  final bool credentialRequired;
  final String defaultModel;
  final List<ProviderModelView> models;
  final List<ProviderModelView> defaultModels;
  final List<ProviderModelView> customModels;
  final Map<String, String> modelConnectionModes;
  final Map<String, ProviderModelAutoCompactView> autoCompactLimits;
  final String status;
  final String usageLabel;
  final String modelCount;
  final String updatedAt;
  final String catalogId;
  final String credentialLabel;
  final String credentialEnv;
  final String capabilitySource;
  final bool hostedWebSearch;
  final String hostedWebSearchDialect;
  final String standaloneWebSearch;
  final String promptCacheDialect;
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
      modelConnectionModes: {...modelConnectionModes, slug: mode},
    );
  }

  /// 返回某模型的三层压缩阈值视图。
  ///
  /// 优先使用 canonical snapshot 的解析结果；草稿中尚未持久化的模型（新建 provider 或
  /// 新增自定义模型）按模型默认 258,000 与上下文 90% 安全上限推导。
  ProviderModelAutoCompactView autoCompactLimitFor(ProviderModelView model) {
    return autoCompactLimits[model.slug] ??
        ProviderModelAutoCompactView.forModel(model);
  }

  /// 写入或更新某模型的三层压缩阈值视图。
  ProviderSettingsView withAutoCompactLimit(
    ProviderModelAutoCompactView limit,
  ) {
    return copyWith(
      autoCompactLimits: {...autoCompactLimits, limit.slug: limit},
    );
  }

  ProviderSettingsView copyWith({
    bool? pricingEnabled,
    String? id,
    String? templateKind,
    String? name,
    String? subtitle,
    String? baseUrl,
    String? bearerToken,
    bool? hasBearerToken,
    bool? credentialRequired,
    String? defaultModel,
    List<ProviderModelView>? models,
    List<ProviderModelView>? defaultModels,
    List<ProviderModelView>? customModels,
    Map<String, String>? modelConnectionModes,
    Map<String, ProviderModelAutoCompactView>? autoCompactLimits,
    String? status,
    String? usageLabel,
    String? modelCount,
    String? updatedAt,
    String? catalogId,
    String? credentialLabel,
    String? credentialEnv,
    String? capabilitySource,
    bool? hostedWebSearch,
    String? hostedWebSearchDialect,
    String? standaloneWebSearch,
    String? promptCacheDialect,
    bool? responsesProgrammaticToolCalling,
    Object? iconKey = _providerSettingsUnset,
  }) {
    return ProviderSettingsView(
      pricingEnabled: pricingEnabled ?? this.pricingEnabled,
      id: id ?? this.id,
      templateKind: templateKind ?? this.templateKind,
      name: name ?? this.name,
      subtitle: subtitle ?? this.subtitle,
      baseUrl: baseUrl ?? this.baseUrl,
      bearerToken: bearerToken ?? this.bearerToken,
      hasBearerToken: hasBearerToken ?? this.hasBearerToken,
      credentialRequired: credentialRequired ?? this.credentialRequired,
      defaultModel: defaultModel ?? this.defaultModel,
      models: models ?? this.models,
      defaultModels: defaultModels ?? this.defaultModels,
      customModels: customModels ?? this.customModels,
      modelConnectionModes: modelConnectionModes ?? this.modelConnectionModes,
      autoCompactLimits: autoCompactLimits ?? this.autoCompactLimits,
      status: status ?? this.status,
      usageLabel: usageLabel ?? this.usageLabel,
      modelCount: modelCount ?? this.modelCount,
      updatedAt: updatedAt ?? this.updatedAt,
      catalogId: catalogId ?? this.catalogId,
      credentialLabel: credentialLabel ?? this.credentialLabel,
      credentialEnv: credentialEnv ?? this.credentialEnv,
      capabilitySource: capabilitySource ?? this.capabilitySource,
      hostedWebSearch: hostedWebSearch ?? this.hostedWebSearch,
      hostedWebSearchDialect:
          hostedWebSearchDialect ?? this.hostedWebSearchDialect,
      standaloneWebSearch: standaloneWebSearch ?? this.standaloneWebSearch,
      promptCacheDialect: promptCacheDialect ?? this.promptCacheDialect,
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
    this.pricingEnabled = true,
    required this.id,
    required this.displayName,
    required this.description,
    required this.baseUrl,
    required this.credentialLabel,
    required this.credentialEnv,
    required this.modelCatalogId,
    required this.suggestedModel,
    this.hostedWebSearch = false,
    this.hostedWebSearchDialect = 'open_ai_responses',
    this.standaloneWebSearch = '',
    this.promptCacheDialect = 'none',
    this.responsesProgrammaticToolCalling = false,
    this.iconKey,
  });

  final bool pricingEnabled;
  final String id;
  final String displayName;
  final String description;
  final String baseUrl;
  final String credentialLabel;
  final String credentialEnv;
  final String modelCatalogId;
  final String suggestedModel;
  final bool hostedWebSearch;
  final String hostedWebSearchDialect;
  final String standaloneWebSearch;
  final String promptCacheDialect;
  final bool responsesProgrammaticToolCalling;
  final String? iconKey;

  ProviderSettingsView createProvider(
    String providerId,
    List<ProviderModelView> models,
  ) {
    return ProviderSettingsView(
      pricingEnabled: pricingEnabled,
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
      status: credentialEnv.isEmpty ? 'ready' : 'missingCredential',
      credentialRequired: credentialEnv.isNotEmpty,
      usageLabel: '${models.length} models',
      modelCount: '${models.length}',
      updatedAt: 'Draft',
      catalogId: modelCatalogId,
      credentialLabel: credentialLabel,
      credentialEnv: credentialEnv,
      capabilitySource: 'preset_defaults',
      hostedWebSearch: hostedWebSearch,
      hostedWebSearchDialect: hostedWebSearchDialect,
      standaloneWebSearch: standaloneWebSearch,
      promptCacheDialect: promptCacheDialect,
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
  final bundledModels = preset == null
      ? const <ProviderModelView>[]
      : catalog.modelsFor(preset.modelCatalogId);
  ProviderModelView applyConnectionMode(ProviderModelView model) =>
      model.copyWith(
        connectionMode:
            provider.modelConnectionModes[model.slug] ??
            model.defaultConnectionMode,
      );
  final defaultModels = bundledModels.map(applyConnectionMode).toList();
  final customModels = provider.customModels.map(applyConnectionMode).toList();
  final effectiveModels = [...defaultModels, ...customModels];
  return provider.copyWith(
    subtitle: provider.subtitle.isEmpty && preset != null
        ? preset.description
        : provider.subtitle,
    catalogId: preset?.modelCatalogId ?? provider.catalogId,
    credentialLabel: preset?.credentialLabel ?? provider.credentialLabel,
    credentialEnv: preset?.credentialEnv ?? provider.credentialEnv,
    iconKey: preset?.iconKey ?? provider.iconKey,
    defaultModels: defaultModels,
    customModels: customModels,
    models: effectiveModels,
    modelCount: '${effectiveModels.length}',
    usageLabel: effectiveModels.isEmpty
        ? provider.defaultModel
        : '${effectiveModels.length} models',
  );
}

class ProviderUsageView {
  const ProviderUsageView({
    required this.providerId,
    this.revision = 0,
    required this.updatedAt,
    required this.state,
  });

  final String providerId;
  final int revision;
  final int updatedAt;
  final ProviderUsageStateView state;
}

sealed class ProviderUsageStateView {
  const ProviderUsageStateView();
}

final class UnsupportedProviderUsageView extends ProviderUsageStateView {
  const UnsupportedProviderUsageView();
}

final class MissingCredentialProviderUsageView extends ProviderUsageStateView {
  const MissingCredentialProviderUsageView({required this.message});
  final String message;
}

final class ReadyProviderUsageView extends ProviderUsageStateView {
  const ReadyProviderUsageView({required this.data});
  final ProviderUsageDataView data;
}

final class FailedProviderUsageView extends ProviderUsageStateView {
  const FailedProviderUsageView({
    required this.code,
    required this.message,
    required this.retryable,
  });
  final String code;
  final String message;
  final bool retryable;
}

sealed class ProviderUsageDataView {
  const ProviderUsageDataView();
}

final class DeepSeekBalanceProviderUsageView extends ProviderUsageDataView {
  const DeepSeekBalanceProviderUsageView({required this.balance});
  final DeepSeekBalanceUsageView balance;
}

final class ZhipuCodingPlanProviderUsageView extends ProviderUsageDataView {
  const ZhipuCodingPlanProviderUsageView({required this.codingPlan});
  final ZhipuCodingPlanUsageView codingPlan;
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
