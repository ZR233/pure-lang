import 'model_catalog.dart';

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
    this.autoCompactTokenLimit,
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
    this.origin = ProviderModelOrigin.bundled,
  });

  final String slug;
  final String displayName;
  final List<String> reasoningEfforts;
  final String description;
  final int? contextWindow;
  final int? maxContextWindow;

  /// 原始模型目录预设；没有预设时按解析上下文容量的 90% 推导。
  final int? autoCompactTokenLimit;
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
  final ProviderModelOrigin origin;

  ProviderModelView copyWith({
    String? slug,
    String? displayName,
    List<String>? reasoningEfforts,
    String? description,
    int? contextWindow,
    int? maxContextWindow,
    int? autoCompactTokenLimit,
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
    ProviderModelOrigin? origin,
  }) {
    return ProviderModelView(
      slug: slug ?? this.slug,
      displayName: displayName ?? this.displayName,
      reasoningEfforts: reasoningEfforts ?? this.reasoningEfforts,
      description: description ?? this.description,
      contextWindow: contextWindow ?? this.contextWindow,
      maxContextWindow: maxContextWindow ?? this.maxContextWindow,
      autoCompactTokenLimit:
          autoCompactTokenLimit ?? this.autoCompactTokenLimit,
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
      origin: origin ?? this.origin,
    );
  }
}

/// provider 实例中某模型上下文压缩阈值的四层视图。
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

  /// 尚无 canonical 解析结果时，由同一份目录元数据推导草稿读数。
  factory ProviderModelAutoCompactView.forModel(ProviderModelView model) {
    final safeLimit = safeLimitForContext(
      model.contextWindow ?? model.maxContextWindow,
    );
    final defaultLimit = model.autoCompactTokenLimit ?? safeLimit;
    return ProviderModelAutoCompactView(
      slug: model.slug,
      defaultLimit: defaultLimit,
      overrideLimit: null,
      effectiveLimit: _effective(defaultLimit, safeLimit),
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
    final limit = quotient * 90 + (remainder * 90) ~/ 100;
    return limit > 0 ? limit : null;
  }

  static int? _effective(int? selected, int? safeLimit) {
    if (selected == null || safeLimit == null) {
      return null;
    }
    return selected < safeLimit ? selected : safeLimit;
  }

  final String slug;
  final int? defaultLimit;
  final int? overrideLimit;
  final int? effectiveLimit;
  final int? safeLimit;

  bool get hasOverride => overrideLimit != null;

  /// 返回覆盖变更后的副本；覆盖为 `null` 表示恢复默认。
  ///
  /// 实际生效值按 `min(覆盖或默认, 安全上限)` 重新推导，上下文未知时保持 `null`。
  ProviderModelAutoCompactView withOverride(int? overrideLimit) {
    final normalized = overrideLimit == defaultLimit ? null : overrideLimit;
    return ProviderModelAutoCompactView(
      slug: slug,
      defaultLimit: defaultLimit,
      overrideLimit: normalized,
      effectiveLimit: _effective(normalized ?? defaultLimit, safeLimit),
      safeLimit: safeLimit,
    );
  }

  /// Reprojects a stored configuration value without normalizing it.
  ///
  /// Read-only catalog merges must retain an explicit override even when a
  /// new catalog default happens to equal it. Normalization belongs to an
  /// explicit write intent, which uses [withOverride].
  ProviderModelAutoCompactView withStoredOverride(int? overrideLimit) {
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
    this.modelCatalog = const ModelCatalogStatusView(),
    required this.id,
    this.templateKind = '',
    required this.name,
    this.subtitle = '',
    required this.baseUrl,
    this.bearerToken = '',
    this.hasBearerToken = false,
    this.credentialRequired = true,
    this.usageQueryRevision = 0,
    required this.defaultModel,
    required this.models,
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
  final ModelCatalogStatusView modelCatalog;
  final String id;
  final String templateKind;
  final String name;
  final String subtitle;
  final String baseUrl;
  final String bearerToken;
  final bool hasBearerToken;
  final bool credentialRequired;
  final int usageQueryRevision;
  final String defaultModel;
  final List<ProviderModelView> models;
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

  List<ProviderModelView> modelsWithOrigin(ProviderModelOrigin origin) => [
    for (final model in models)
      if (model.origin == origin) model,
  ];

  List<ProviderModelView> get bundledModels =>
      modelsWithOrigin(ProviderModelOrigin.bundled);

  List<ProviderModelView> get onlineModels =>
      modelsWithOrigin(ProviderModelOrigin.online);

  List<ProviderModelView> get customModels =>
      modelsWithOrigin(ProviderModelOrigin.manual);

  ProviderSettingsView withModelConnection(String slug, String mode) {
    ProviderModelView update(ProviderModelView model) =>
        model.slug == slug ? model.copyWith(connectionMode: mode) : model;
    return copyWith(
      models: models.map(update).toList(),
      modelConnectionModes: {...modelConnectionModes, slug: mode},
    );
  }

  /// 返回某模型的四层压缩阈值视图。
  ///
  /// 优先使用 canonical snapshot 的解析结果；草稿中尚未持久化的模型（新建 provider 或
  /// 新增自定义模型）按目录预设或上下文 90% 推导，不写入用户配置。
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
    ModelCatalogStatusView? modelCatalog,
    bool? pricingEnabled,
    String? id,
    String? templateKind,
    String? name,
    String? subtitle,
    String? baseUrl,
    String? bearerToken,
    bool? hasBearerToken,
    bool? credentialRequired,
    int? usageQueryRevision,
    String? defaultModel,
    List<ProviderModelView>? models,
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
      modelCatalog: modelCatalog ?? this.modelCatalog,
      pricingEnabled: pricingEnabled ?? this.pricingEnabled,
      id: id ?? this.id,
      templateKind: templateKind ?? this.templateKind,
      name: name ?? this.name,
      subtitle: subtitle ?? this.subtitle,
      baseUrl: baseUrl ?? this.baseUrl,
      bearerToken: bearerToken ?? this.bearerToken,
      hasBearerToken: hasBearerToken ?? this.hasBearerToken,
      credentialRequired: credentialRequired ?? this.credentialRequired,
      usageQueryRevision: usageQueryRevision ?? this.usageQueryRevision,
      defaultModel: defaultModel ?? this.defaultModel,
      models: models ?? this.models,
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

  /// 空目录只表示尚未加载，不能覆盖已经加载的静态事实。
  bool get isUsable => revision.isNotEmpty && presets.isNotEmpty;

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
      usageQueryRevision: 0,
      defaultModel: suggestedModel,
      models: [
        for (final model in models)
          model.origin == ProviderModelOrigin.bundled
              ? model
              : model.copyWith(origin: ProviderModelOrigin.bundled),
      ],
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
  ProviderModelView applyConnectionMode(ProviderModelView model) =>
      model.copyWith(
        connectionMode:
            provider.modelConnectionModes[model.slug] ??
            model.defaultConnectionMode,
      );
  final effectiveModels = provider.models.map(applyConnectionMode).toList();
  return provider.copyWith(
    subtitle: provider.subtitle.isEmpty && preset != null
        ? preset.description
        : provider.subtitle,
    catalogId: preset?.modelCatalogId ?? provider.catalogId,
    credentialLabel: preset?.credentialLabel ?? provider.credentialLabel,
    credentialEnv: preset?.credentialEnv ?? provider.credentialEnv,
    iconKey: preset?.iconKey ?? provider.iconKey,
    models: effectiveModels,
    modelCount: '${effectiveModels.length}',
    usageLabel: effectiveModels.isEmpty
        ? provider.defaultModel
        : '${effectiveModels.length} models',
  );
}

/// 为整组供应商附加静态目录元数据。
///
/// 所有 UI 投影都通过这里完成目录 join，避免设置、状态栏和 Driver 各自
/// 维护一份字段合并逻辑。
List<ProviderSettingsView> providersWithCatalogMetadata(
  Iterable<ProviderSettingsView> providers,
  ProviderCatalogView catalog,
) => [
  for (final provider in providers)
    providerWithCatalogMetadata(provider, catalog),
];

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
