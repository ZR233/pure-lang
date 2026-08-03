part of 'studio_api.dart';

ProviderSettingsView _providerSettingsFromFrb(
  frb.BridgeProviderSettingsDto value,
) {
  final models = value.models.map(_providerModelSettingsFromFrb).toList();
  final customModels = value.customModels
      .map(_providerModelSettingsFromFrb)
      .toList();
  final customSlugs = customModels.map((model) => model.slug).toSet();
  final defaultModels = models
      .where((model) => !customSlugs.contains(model.slug))
      .toList();
  final modelCount = models.length;

  return ProviderSettingsView(
    id: value.id,
    templateKind: value.templateKind,
    name: value.name,
    subtitle: '${value.name} Platform',
    baseUrl: value.baseUrl,
    bearerToken: '',
    hasBearerToken: value.hasBearerToken,
    defaultModel: value.defaultModel,
    models: models,
    defaultModels: defaultModels,
    customModels: customModels,
    status: value.hasBearerToken ? 'ready' : 'missingCredential',
    usageLabel: models.isEmpty ? value.defaultModel : '$modelCount models',
    modelCount: '$modelCount',
    updatedAt: 'Loaded',
    wireProtocol: value.wireProtocol,
    connectionMode: value.connectionMode,
    catalogId: value.catalogId ?? '',
    capabilitySource: value.capabilitySource,
    hostedWebSearch: value.hostedWebSearch,
    standaloneWebSearch: value.standaloneWebSearch ?? '',
  );
}

ProviderModelView _providerModelSettingsFromFrb(
  frb.BridgeProviderModelSettingsDto value,
) {
  return ProviderModelView(
    slug: value.slug,
    displayName: value.displayName,
    description: value.description,
    contextWindow: value.contextWindow?.toInt(),
    maxOutputTokens: value.maxOutputTokens?.toInt(),
    currency: value.currency,
    inputPricePerMTok: value.inputPricePerMTok,
    outputPricePerMTok: value.outputPricePerMTok,
    cacheReadPricePerMTok: value.cacheReadPricePerMTok,
    baseInstructions: value.baseInstructions,
    reasoningEfforts: value.reasoningEfforts,
  );
}

RoleSettingsView _roleSettingsFromFrb(frb.BridgeRoleSettingsDto value) {
  return RoleSettingsView(
    key: value.key,
    providerId: value.providerId,
    model: value.model,
    effort: value.effort,
  );
}

InstructionsSettingsView _instructionsSettingsFromFrb(
  frb.BridgeInstructionsSettingsDto value,
) {
  return InstructionsSettingsView(
    baseOverride: value.baseOverride,
    developer: value.developer,
    user: value.user,
    projectDocMaxBytes: value.projectDocMaxBytes.toInt(),
    projectDocFallbackFilenames: value.projectDocFallbackFilenames,
  );
}

SkillsSettingsView _skillsSettingsFromFrb(frb.BridgeSkillsSettingsDto value) {
  return SkillsSettingsView(
    enabled: value.enabled,
    autoLearn: value.autoLearn,
    systemEnabled: value.systemEnabled,
    projectDir: value.projectDir,
    userDir: value.userDir,
    externalDirs: value.externalDirs,
    disabled: value.disabled,
    autoLearnMinToolCalls: value.autoLearnMinToolCalls,
  );
}

McpServerSettingsView _mcpSettingsFromFrb(
  frb.BridgeMcpServerSettingsDto value,
) {
  return McpServerSettingsView(
    id: value.id,
    transport: value.transport,
    endpoint: value.endpoint,
    enabled: value.enabled,
    status: value.status,
    sourceKind: value.sourceKind,
    mutationPolicy: value.mutationPolicy,
  );
}

GeneralSettingsView _generalSettingsFromFrb(
  frb.BridgeGeneralSettingsDto value,
) {
  return GeneralSettingsView(
    followSystemTheme: value.followSystemTheme,
    followActiveTurn: value.followActiveTurn,
    compactTimeline: value.compactTimeline,
  );
}
