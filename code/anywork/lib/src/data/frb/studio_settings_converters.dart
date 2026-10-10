part of 'studio_api.dart';

frb.SettingsFieldInput _settingsFieldInputFromDomain(
  SettingsFieldCommand command,
) => switch (command) {
  InstructionBaseOverrideCommand(:final value) =>
    frb.SettingsFieldInput.instructionBaseOverride(value: value),
  InstructionDeveloperCommand(:final value) =>
    frb.SettingsFieldInput.instructionDeveloper(value: value),
  InstructionUserCommand(:final value) =>
    frb.SettingsFieldInput.instructionUser(value: value),
  ProjectDocMaxBytesCommand(:final value) =>
    frb.SettingsFieldInput.projectDocMaxBytes(value: BigInt.from(value)),
  ProjectDocFallbackFilenamesCommand(:final value) =>
    frb.SettingsFieldInput.projectDocFallbackFilenames(value: value),
  SkillsEnabledCommand(:final value) => frb.SettingsFieldInput.skillsEnabled(
    value: value,
  ),
  SkillsAutoLearnCommand(:final value) =>
    frb.SettingsFieldInput.skillsAutoLearn(value: value),
  SkillsSystemEnabledCommand(:final value) =>
    frb.SettingsFieldInput.skillsSystemEnabled(value: value),
  SkillsProjectDirCommand(:final value) =>
    frb.SettingsFieldInput.skillsProjectDir(value: value),
  SkillsUserDirCommand(:final value) => frb.SettingsFieldInput.skillsUserDir(
    value: value,
  ),
  SkillsExternalDirsCommand(:final value) =>
    frb.SettingsFieldInput.skillsExternalDirs(value: value),
  SkillsDisabledCommand(:final value) => frb.SettingsFieldInput.skillsDisabled(
    value: value,
  ),
  SkillsAutoLearnMinToolCallsCommand(:final value) =>
    frb.SettingsFieldInput.skillsAutoLearnMinToolCalls(value: value),
  McpServerEnabledCommand(:final id, :final value) =>
    frb.SettingsFieldInput.mcpServerEnabled(id: id, value: value),
  McpServerTransportCommand(:final id, :final value) =>
    frb.SettingsFieldInput.mcpServerTransport(id: id, transport: value),
  McpServerEndpointCommand(:final id, :final value) =>
    frb.SettingsFieldInput.mcpServerEndpoint(id: id, endpoint: value),
  GeneralFollowActiveTurnCommand(:final value) =>
    frb.SettingsFieldInput.generalFollowActiveTurn(value: value),
  GeneralCompactTimelineCommand(:final value) =>
    frb.SettingsFieldInput.generalCompactTimeline(value: value),
  GeneralSidebarWidthCommand(:final value) =>
    frb.SettingsFieldInput.generalSidebarWidth(value: value),
  GeneralPinnedThreadIdsCommand(:final value) =>
    frb.SettingsFieldInput.generalPinnedThreadIds(value: value),
  GeneralPinnedProjectIdsCommand(:final value) =>
    frb.SettingsFieldInput.generalPinnedProjectIds(value: value),
  WebSearchModeCommand(:final value) => frb.SettingsFieldInput.webSearchMode(
    value: value,
  ),
  WebSearchContextSizeCommand(:final value) =>
    frb.SettingsFieldInput.webSearchContextSize(value: value),
  WebSearchAllowedDomainsCommand(:final value) =>
    frb.SettingsFieldInput.webSearchAllowedDomains(value: value),
  WebSearchCountryCommand(:final value) =>
    frb.SettingsFieldInput.webSearchCountry(value: value),
  WebSearchRegionCommand(:final value) =>
    frb.SettingsFieldInput.webSearchRegion(value: value),
  WebSearchCityCommand(:final value) => frb.SettingsFieldInput.webSearchCity(
    value: value,
  ),
  WebSearchTimezoneCommand(:final value) =>
    frb.SettingsFieldInput.webSearchTimezone(value: value),
  DeepSeekWebSearchEnabledCommand(:final value) =>
    frb.SettingsFieldInput.deepSeekWebSearchEnabled(value: value),
  ModeModelCommand(:final modeId, :final providerId, :final model) =>
    frb.SettingsFieldInput.modeModel(
      modeId: modeId,
      providerId: providerId,
      model: model,
    ),
  ModeReasoningEffortCommand(:final modeId, :final effort) =>
    frb.SettingsFieldInput.modeReasoningEffort(modeId: modeId, effort: effort),
  RoleModelCommand(:final role, :final providerId, :final model) =>
    frb.SettingsFieldInput.roleModel(
      role: role,
      providerId: providerId,
      model: model,
    ),
  RoleReasoningEffortCommand(:final role, :final effort) =>
    frb.SettingsFieldInput.roleReasoningEffort(role: role, effort: effort),
};

ModelCatalogStatusView _modelCatalogStatusFromFrb(
  frb.BridgeModelCatalogStatusDto status,
) => ModelCatalogStatusView(
  supported: status.supported,
  source: switch (status.source) {
    frb.BridgeModelCatalogSource.default_ =>
      ModelCatalogSource.defaultDefinition,
    frb.BridgeModelCatalogSource.cached => ModelCatalogSource.cached,
    frb.BridgeModelCatalogSource.online => ModelCatalogSource.online,
  },
  probing: status.probing,
  lastSuccessAt: status.lastSuccessAt?.toInt(),
  checkedAt: status.checkedAt?.toInt(),
  error: switch (status.error) {
    null => null,
    frb.BridgeModelCatalogError_Unsupported() => const ModelCatalogErrorView(
      ModelCatalogFailure.unsupported,
    ),
    frb.BridgeModelCatalogError_Configuration() => const ModelCatalogErrorView(
      ModelCatalogFailure.configuration,
    ),
    frb.BridgeModelCatalogError_Timeout() => const ModelCatalogErrorView(
      ModelCatalogFailure.timeout,
    ),
    frb.BridgeModelCatalogError_Transport(:final httpStatus) =>
      ModelCatalogErrorView(
        ModelCatalogFailure.transport,
        httpStatus: httpStatus,
      ),
    frb.BridgeModelCatalogError_Http(:final status) => ModelCatalogErrorView(
      ModelCatalogFailure.http,
      httpStatus: status,
    ),
    frb.BridgeModelCatalogError_TooLarge() => const ModelCatalogErrorView(
      ModelCatalogFailure.tooLarge,
    ),
    frb.BridgeModelCatalogError_Protocol() => const ModelCatalogErrorView(
      ModelCatalogFailure.protocol,
    ),
    frb.BridgeModelCatalogError_CacheIdentity() => const ModelCatalogErrorView(
      ModelCatalogFailure.cacheIdentity,
    ),
    frb.BridgeModelCatalogError_UnexpectedNotModified() =>
      const ModelCatalogErrorView(ModelCatalogFailure.unexpectedNotModified),
    frb.BridgeModelCatalogError_CacheWrite() => const ModelCatalogErrorView(
      ModelCatalogFailure.cacheWrite,
    ),
    frb.BridgeModelCatalogError_Closing() => const ModelCatalogErrorView(
      ModelCatalogFailure.closing,
    ),
    frb.BridgeModelCatalogError_Stale() => const ModelCatalogErrorView(
      ModelCatalogFailure.stale,
    ),
  },
  cacheWarning: switch (status.cacheWarning) {
    null => null,
    frb.BridgeModelCatalogCacheWarning.read => ModelCatalogCacheWarning.read,
    frb.BridgeModelCatalogCacheWarning.schema =>
      ModelCatalogCacheWarning.schema,
    frb.BridgeModelCatalogCacheWarning.identity =>
      ModelCatalogCacheWarning.identity,
    frb.BridgeModelCatalogCacheWarning.declaration =>
      ModelCatalogCacheWarning.declaration,
  },
);

ProviderSettingsView _providerSettingsFromFrb(
  frb.BridgeProviderSettingsDto value, {
  frb.BridgeModelCatalogProviderDto? catalog,
}) {
  final customModels = value.customModels
      .map(_customModelSettingsFromFrb)
      .toList();
  final connectionModes = {
    for (final mode in value.modelConnectionModes)
      mode.slug: mode.connectionMode,
  };
  final autoCompactLimits = {
    for (final limit in value.modelAutoCompactLimits)
      limit.slug: ProviderModelAutoCompactView(
        slug: limit.slug,
        defaultLimit: limit.defaultLimit.toInt(),
        overrideLimit: limit.overrideLimit?.toInt(),
        effectiveLimit: limit.effectiveLimit?.toInt(),
        safeLimit: limit.safeLimit?.toInt(),
      ),
  };

  final catalogModels =
      catalog?.effectiveModels.map(_providerModelFromCatalog).toList() ??
      const <ProviderModelView>[];
  final models = catalog == null ? const <ProviderModelView>[] : catalogModels;
  return ProviderSettingsView(
    modelCatalog: catalog == null
        ? const ModelCatalogStatusView()
        : _modelCatalogStatusFromFrb(catalog.modelCatalog),
    pricingEnabled: value.pricingEnabled,
    id: value.id,
    templateKind: value.templateKind,
    name: value.name,
    subtitle: '${value.name} Platform',
    baseUrl: value.baseUrl,
    bearerToken: '',
    hasBearerToken: value.hasBearerToken,
    credentialRequired: value.credentialRequired,
    defaultModel: value.defaultModel,
    models: models,
    customModels: customModels,
    modelConnectionModes: connectionModes,
    autoCompactLimits: autoCompactLimits,
    status: value.hasBearerToken || !value.credentialRequired
        ? 'ready'
        : 'missingCredential',
    usageLabel: value.defaultModel,
    modelCount: '${models.length}',
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
    inputCapabilities: value.inputCapabilities
        .map(_modelInputCapabilityFromFrb)
        .toList(),
    contextWindow: value.contextWindow.toInt(),
    maxOutputTokens: value.maxOutputTokens.toInt(),
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

ModeModelRouteView _modeModelRouteFromFrb(
  frb.BridgeModeModelSettingsDto value,
) {
  return ModeModelRouteView(
    modeId: ThreadModeId.fromId(value.modeId),
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
        message: 'MCP health check is pending',
      ),
      frb.BridgeMcpServerConfiguration.disabled => const McpDisabledState(
        message: 'MCP server is disabled in configuration',
      ),
      frb.BridgeMcpServerConfiguration.missingCredential =>
        const McpMissingCredentialState(
          message: 'MCP server credential is not configured',
        ),
    },
    sourceKind: value.sourceKind,
    mutationPolicy: value.mutationPolicy,
  );
}

GeneralSettingsView _generalSettingsFromFrb(
  frb.BridgeGeneralSettingsDto value,
) {
  return GeneralSettingsView(
    followActiveTurn: value.followActiveTurn,
    compactTimeline: value.compactTimeline,
    sidebarWidth: value.sidebarWidth ?? 336,
    pinnedThreadIds: value.pinnedThreadIds,
    pinnedProjectIds: value.pinnedProjectIds,
  );
}
