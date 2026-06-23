class ProviderModelView {
  const ProviderModelView({
    required this.slug,
    required this.displayName,
    required this.reasoningEfforts,
    this.description = '',
    this.contextWindow,
    this.maxOutputTokens,
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
  final int? maxOutputTokens;
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
    int? maxOutputTokens,
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
      maxOutputTokens: maxOutputTokens ?? this.maxOutputTokens,
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
    this.templateKind = 'openai',
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
    this.providerKind = '',
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
  final String providerKind;

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
    String? providerKind,
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
      providerKind: providerKind ?? this.providerKind,
    );
  }
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
