part of 'studio_api.dart';

const demoProviderCatalogFixture = ProviderCatalogView(
  schemaVersion: 3,
  revision: 'demo-future-catalog-v3',
  presets: [
    ProviderPresetView(
      id: 'future-provider',
      displayName: 'Future Provider',
      description: 'Injected demo provider catalog fixture',
      wireProtocol: 'chat_completions',
      connectionModes: [
        ProviderConnectionModeView(id: 'http', displayName: 'HTTP'),
      ],
      defaultConnectionMode: 'http',
      baseUrl: 'https://future.example/v1',
      credentialLabel: 'Access Key',
      credentialEnv: 'FUTURE_PROVIDER_KEY',
      modelCatalogId: 'future-catalog',
      suggestedModel: 'future-model',
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

List<ProviderSettingsView> _providersFromSettingsPayload(
  Map<String, Object?> settings, {
  List<ProviderSettingsView> previous = const [],
  required ProviderCatalogView catalog,
}) {
  return _list(settings['providers']).map((value) {
    final provider = _map(value);
    final customModels = _list(provider['customModels'])
        .map(_providerSettingsModelFromJson)
        .where((model) => model.slug.isNotEmpty)
        .toList();
    final template =
        catalog.preset(_string(provider['templateKind'])) ??
        catalog.presets.first;
    final defaultModels = catalog.modelsFor(template.modelCatalogId);
    final models = [...defaultModels, ...customModels];
    final token = _string(provider['bearerToken']);
    final previousProvider = previous
        .where((item) => item.id == _string(provider['id']))
        .firstOrNull;
    final hasToken =
        token.trim().isNotEmpty || (previousProvider?.hasBearerToken ?? false);
    return ProviderSettingsView(
      id: _string(provider['id']),
      templateKind: template.id,
      name: _string(provider['name'], fallback: template.displayName),
      subtitle: _string(provider['name'], fallback: template.displayName),
      baseUrl: _string(provider['baseUrl'], fallback: template.baseUrl),
      bearerToken: '',
      hasBearerToken: hasToken,
      defaultModel: _string(
        provider['defaultModel'],
        fallback: template.suggestedModel,
      ),
      models: models,
      defaultModels: defaultModels,
      customModels: customModels,
      status: hasToken ? 'ready' : 'missingCredential',
      usageLabel: '${models.length} models',
      modelCount: '${models.length}',
      updatedAt: 'Preview',
      wireProtocol: template.wireProtocol,
      connectionMode: _string(
        provider['connectionMode'],
        fallback: template.defaultConnectionMode,
      ),
      catalogId: template.modelCatalogId,
      credentialLabel: template.credentialLabel,
      credentialEnv: template.credentialEnv,
      iconKey: template.iconKey,
    );
  }).toList();
}

ProviderModelView _providerSettingsModelFromJson(Object? value) {
  final model = _map(value);
  final slug = _string(model['slug']);
  return ProviderModelView(
    slug: slug,
    displayName: _string(model['displayName'], fallback: slug),
    reasoningEfforts: _stringList(model['reasoningEfforts']),
    baseInstructions: _string(model['baseInstructions']),
  );
}

List<RoleSettingsView> _rolesFromSettingsPayload(
  Map<String, Object?> settings,
) {
  return _list(settings['roles']).map((value) {
    final role = _map(value);
    return RoleSettingsView(
      key: _string(role['key']),
      providerId: _string(role['provider']),
      model: _string(role['model']),
      effort: _string(role['effort']),
    );
  }).toList();
}

InstructionsSettingsView _instructionsFromSettingsPayload(
  Map<String, Object?> settings,
) {
  return InstructionsSettingsView(
    baseOverride: _string(settings['baseOverride']),
    developer: _string(settings['developer']),
    user: _string(settings['user']),
    projectDocMaxBytes: _int(settings['projectDocMaxBytes'], fallback: 65536),
    projectDocFallbackFilenames: _stringList(
      settings['projectDocFallbackFilenames'],
    ),
  );
}

SkillsSettingsView _skillsFromSettingsPayload(Map<String, Object?> settings) {
  return SkillsSettingsView(
    enabled: _boolWithDefault(settings['enabled'], true),
    autoLearn: _boolWithDefault(settings['autoLearn'], true),
    systemEnabled: _boolWithDefault(settings['systemEnabled'], true),
    projectDir: _string(settings['projectDir'], fallback: 'skills'),
    userDir: _string(settings['userDir'], fallback: '~/.pure/skills'),
    externalDirs: _stringList(settings['externalDirs']),
    disabled: _stringList(settings['disabled']),
    autoLearnMinToolCalls: _int(settings['autoLearnMinToolCalls'], fallback: 5),
  );
}
