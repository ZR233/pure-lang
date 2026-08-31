part of 'studio_api.dart';

ProviderSettingsView _providerSettingsFromFrb(
  frb.BridgeProviderSettingsDto value,
) {
  final customModels = value.customModels
      .map(_customModelSettingsFromFrb)
      .toList();
  final connectionModes = {
    for (final mode in value.modelConnectionModes)
      mode.slug: mode.connectionMode,
  };

  return ProviderSettingsView(
    id: value.id,
    templateKind: value.templateKind,
    name: value.name,
    subtitle: '${value.name} Platform',
    baseUrl: value.baseUrl,
    bearerToken: '',
    hasBearerToken: value.hasBearerToken,
    defaultModel: value.defaultModel,
    models: const [],
    customModels: customModels,
    modelConnectionModes: connectionModes,
    status: value.hasBearerToken ? 'ready' : 'missingCredential',
    usageLabel: value.defaultModel,
    modelCount: '${customModels.length}',
    updatedAt: 'Loaded',
    catalogId: value.catalogId ?? '',
    capabilitySource: value.capabilitySource,
    hostedWebSearch: value.hostedWebSearch,
    hostedWebSearchDialect: value.hostedWebSearchDialect,
    standaloneWebSearch: value.standaloneWebSearch ?? '',
    promptCacheDialect: value.promptCacheDialect,
    responsesProgrammaticToolCalling: value.responsesProgrammaticToolCalling,
  );
}

ProviderModelView _customModelSettingsFromFrb(
  frb.BridgeCustomModelSettingsDto value,
) {
  return ProviderModelView(
    slug: value.slug,
    displayName: value.displayName,
    baseInstructions: value.baseInstructions,
    reasoningEfforts: value.reasoningEfforts,
    wireProtocol: value.wireProtocol,
    supportedConnectionModes: value.supportedConnectionModes,
    defaultConnectionMode: value.defaultConnectionMode,
    connectionMode: value.defaultConnectionMode,
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
    state: switch (value.configuration) {
      frb.BridgeMcpServerConfiguration.enabled => const McpCheckingState(
        message: '',
      ),
      frb.BridgeMcpServerConfiguration.disabled => const McpDisabledState(
        message: '',
      ),
      frb.BridgeMcpServerConfiguration.missingCredential =>
        const McpMissingCredentialState(message: ''),
    },
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
