part of 'studio_api.dart';

const demoProviderCatalogFixture = ProviderCatalogView(
  schemaVersion: 6,
  revision: 'demo-future-catalog-v6',
  presets: [
    ProviderPresetView(
      id: 'future-provider',
      displayName: 'Future Provider',
      description: 'Injected demo provider catalog fixture',
      baseUrl: 'https://future.example/v1',
      credentialLabel: 'Access Key',
      credentialEnv: 'FUTURE_PROVIDER_KEY',
      modelCatalogId: 'future-catalog',
      suggestedModel: 'future-model',
      hostedWebSearch: true,
      standaloneWebSearch: 'future_search_dialect',
      promptCacheDialect: 'implicit_prefix',
      responsesToolSearch: false,
      responsesProgrammaticToolCalling: false,
    ),
  ],
  modelCatalogs: {
    'future-catalog': [
      ProviderModelView(
        slug: 'future-model',
        displayName: 'Future Model',
        reasoningEfforts: ['eco', 'balanced', 'max'],
        defaultReasoningEffort: 'balanced',
        contextWindow: 500000,
        maxOutputTokens: 64000,
        wireProtocol: 'chat_completions',
        supportedConnectionModes: ['http'],
        defaultConnectionMode: 'http',
        connectionMode: 'http',
      ),
    ],
  },
);

ProviderUsageView _demoProviderUsage(ProviderSettingsView provider) {
  final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
  if (!provider.hasBearerToken) {
    return ProviderUsageView(
      providerId: provider.id,
      updatedAt: now,
      status: 'missingCredential',
      usageKind: 'unknown',
      message: 'provider API key is not configured',
    );
  }
  return ProviderUsageView(
    providerId: provider.id,
    updatedAt: now,
    status: 'unsupported',
    usageKind: 'unsupported',
  );
}

List<ProviderSettingsView> _providersFromSettingsCommand(
  ProviderSettingsCommand command, {
  List<ProviderSettingsView> previous = const [],
  required ProviderCatalogView catalog,
}) {
  return command.providers.map((provider) {
    final customModels = provider.customModels
        .map(
          (model) => ProviderModelView(
            slug: model.slug,
            displayName: model.displayName,
            reasoningEfforts: model.reasoningEfforts,
            baseInstructions: model.baseInstructions ?? '',
            wireProtocol: model.wireProtocol,
            supportedConnectionModes: model.supportedConnectionModes,
            defaultConnectionMode: model.defaultConnectionMode,
            connectionMode: model.defaultConnectionMode,
          ),
        )
        .where((model) => model.slug.isNotEmpty)
        .toList();
    final template =
        catalog.preset(provider.templateKind) ?? catalog.presets.first;
    final connectionModes = {
      for (final model in provider.modelConnectionModes)
        model.slug: model.connectionMode,
    };
    ProviderModelView withCurrentConnection(ProviderModelView model) =>
        model.copyWith(
          connectionMode:
              connectionModes[model.slug] ?? model.defaultConnectionMode,
        );
    final defaultModels = catalog
        .modelsFor(template.modelCatalogId)
        .map(withCurrentConnection)
        .toList();
    final models = [
      ...defaultModels,
      ...customModels.map(withCurrentConnection),
    ];
    final previousProvider = previous
        .where((item) => item.id == (provider.originalId ?? provider.id))
        .firstOrNull;
    final hasToken = switch (provider.secret.action) {
      ProviderSecretAction.preserve =>
        previousProvider?.hasBearerToken ?? false,
      ProviderSecretAction.replace => true,
      ProviderSecretAction.clear => false,
    };
    return ProviderSettingsView(
      id: provider.id,
      templateKind: template.id,
      name: provider.name.isEmpty ? template.displayName : provider.name,
      subtitle: provider.name.isEmpty ? template.displayName : provider.name,
      baseUrl: provider.baseUrl.isEmpty ? template.baseUrl : provider.baseUrl,
      bearerToken: '',
      hasBearerToken: hasToken,
      defaultModel: provider.defaultModel.isEmpty
          ? template.suggestedModel
          : provider.defaultModel,
      models: models,
      defaultModels: defaultModels,
      customModels: customModels.map(withCurrentConnection).toList(),
      status: hasToken ? 'ready' : 'missingCredential',
      usageLabel: '${models.length} models',
      modelCount: '${models.length}',
      updatedAt: 'Preview',
      catalogId: template.modelCatalogId,
      credentialLabel: template.credentialLabel,
      credentialEnv: template.credentialEnv,
      capabilitySource: provider.capabilitySource.isEmpty
          ? 'preset_defaults'
          : provider.capabilitySource,
      hostedWebSearch: provider.hostedWebSearch,
      standaloneWebSearch:
          provider.standaloneWebSearch ?? template.standaloneWebSearch,
      promptCacheDialect: provider.promptCacheDialect,
      responsesToolSearch: provider.responsesToolSearch,
      responsesProgrammaticToolCalling:
          provider.responsesProgrammaticToolCalling,
      iconKey: template.iconKey,
    );
  }).toList();
}

List<RoleSettingsView> _rolesFromSettingsCommand(
  ProviderSettingsCommand command,
) {
  return command.roles.map((role) {
    return RoleSettingsView(
      key: role.key,
      providerId: role.providerId,
      model: role.model,
      effort: role.effort,
    );
  }).toList();
}

InstructionsSettingsView _instructionsFromSettingsCommand(
  InstructionsSettingsCommand command,
) {
  return InstructionsSettingsView(
    baseOverride: command.baseOverride,
    developer: command.developer,
    user: command.user,
    projectDocMaxBytes: command.projectDocMaxBytes,
    projectDocFallbackFilenames: command.projectDocFallbackFilenames,
  );
}

SkillsSettingsView _skillsFromSettingsCommand(SkillsSettingsCommand command) {
  return SkillsSettingsView(
    enabled: command.enabled,
    autoLearn: command.autoLearn,
    systemEnabled: command.systemEnabled,
    projectDir: command.projectDir,
    userDir: command.userDir,
    externalDirs: command.externalDirs,
    disabled: command.disabled,
    autoLearnMinToolCalls: command.autoLearnMinToolCalls,
  );
}
