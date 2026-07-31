part of 'studio_api.dart';

String? _defaultProviderIdFromConfig(Map<String, Object?> config) {
  return _nullableString(config['defaultProviderId']);
}

List<ProviderSettingsView> _providersFromConfig(Map<String, Object?> config) {
  final providers = _map(config['providers']);
  return providers.entries.map((entry) {
    final value = _map(entry.value);
    final templateKind = _providerTemplateKind(value);
    final providerModels = _providerModels(value['models']);
    final projectedCustomModels = _providerModels(value['customModels']);
    final customSlugs = projectedCustomModels
        .map((model) => model.slug)
        .toSet();
    final defaultModels = providerModels
        .where((model) => !customSlugs.contains(model.slug))
        .toList();
    final customModels = projectedCustomModels;
    final visibleModels = providerModels;
    final defaultModel = _string(value['defaultModel']);
    final hasBearerToken = _bool(value['hasBearerToken']);
    final name = _string(value['name']);
    final capabilities = _map(value['serviceCapabilities']);
    final webSearchCapabilities = _map(capabilities['webSearch']);
    return ProviderSettingsView(
      id: entry.key,
      templateKind: templateKind,
      name: name,
      subtitle: '$name Platform',
      baseUrl: _string(value['baseUrl']),
      bearerToken: '',
      hasBearerToken: hasBearerToken,
      defaultModel: defaultModel,
      models: visibleModels,
      defaultModels: defaultModels,
      customModels: customModels,
      status: hasBearerToken ? 'ready' : 'missingCredential',
      usageLabel: visibleModels.isEmpty
          ? defaultModel
          : '${visibleModels.length} models',
      modelCount: '${visibleModels.length}',
      updatedAt: 'Loaded',
      wireProtocol: _string(value['wireProtocol']),
      connectionMode: _string(value['connectionMode']),
      catalogId: _string(value['catalogId']),
      capabilitySource: _string(value['capabilitySource']),
      hostedWebSearch: _bool(webSearchCapabilities['hostedResponses']),
      standaloneWebSearch: _string(webSearchCapabilities['standalone']),
    );
  }).toList();
}

List<ProviderModelView> _providerModels(Object? value) {
  return _list(value)
      .map((modelValue) {
        final model = _map(modelValue);
        final slug = _string(model['slug']);
        return ProviderModelView(
          slug: slug,
          displayName: _string(model['displayName']),
          description: _string(model['description']),
          contextWindow: _nullableInt(model['contextWindow']),
          maxOutputTokens: _nullableInt(model['maxOutputTokens']),
          currency: _string(model['currency']),
          inputPricePerMTok: _nullableDouble(model['inputPricePerMTok']),
          outputPricePerMTok: _nullableDouble(model['outputPricePerMTok']),
          cacheReadPricePerMTok: _nullableDouble(
            model['cacheReadPricePerMTok'],
          ),
          baseInstructions: _string(model['baseInstructions']),
          reasoningEfforts: _stringList(model['reasoningEfforts']),
        );
      })
      .where((model) => model.slug.isNotEmpty)
      .toList();
}

String _providerTemplateKind(Map<String, Object?> provider) {
  return _string(provider['presetId']);
}

List<RoleSettingsView> _rolesFromConfig(Map<String, Object?> config) {
  final roles = _map(config['roles']);
  const roleKeys = ['explorer', 'planner', 'executor', 'reviewer'];
  return [
    for (final key in roleKeys)
      if (_map(roles[key]).isNotEmpty)
        RoleSettingsView(
          key: key,
          providerId: _string(_map(roles[key])['provider']),
          model: _string(_map(roles[key])['model']),
          effort: _string(_map(roles[key])['effort']),
        ),
  ];
}

InstructionsSettingsView _instructionsFromConfig(Map<String, Object?> config) {
  final instructions = _map(config['instructions']);
  return InstructionsSettingsView(
    baseOverride: _string(instructions['baseOverride']),
    developer: _string(instructions['developer']),
    user: _string(instructions['user']),
    projectDocMaxBytes: _int(instructions['projectDocMaxBytes']),
    projectDocFallbackFilenames: _stringList(
      instructions['projectDocFallbackFilenames'],
    ),
  );
}

SkillsSettingsView _skillsFromConfig(Map<String, Object?> config) {
  final skills = _map(config['skills']);
  final system = _map(skills['system']);
  return SkillsSettingsView(
    enabled: _bool(skills['enabled']),
    autoLearn: _bool(skills['autoLearn']),
    systemEnabled: _bool(system['enabled']),
    projectDir: _string(skills['projectDir']),
    userDir: _string(skills['userDir']),
    externalDirs: _stringList(skills['externalDirs']),
    disabled: _stringList(skills['disabled']),
    autoLearnMinToolCalls: _int(skills['autoLearnMinToolCalls']),
  );
}

GeneralSettingsView _generalFromJson(Object? value) {
  final json = _map(value);
  return GeneralSettingsView(
    followSystemTheme: _boolWithDefault(json['followSystemTheme'], true),
    followActiveTurn: _boolWithDefault(json['followActiveTurn'], true),
    compactTimeline: _bool(json['compactTimeline']),
  );
}

List<McpServerSettingsView> _mcpServersFromConfig(Map<String, Object?> config) {
  final servers = <McpServerSettingsView>[];
  final hasZhipuMcpCredential = _hasZhipuMcpCredential(config);
  void addServers(Object? value, {required bool builtin}) {
    for (final entry in _map(value).entries) {
      final server = _map(entry.value);
      final builtinEndpoint = builtin ? _builtinMcpEndpoint(entry.key) : '';
      final transport = builtin
          ? _builtinMcpTransport(entry.key)
          : _mcpTransport(server['transport']);
      final command = _string(server['command']);
      final url = _string(server['url']);
      final enabled = _bool(server['enabled']);
      servers.add(
        McpServerSettingsView(
          id: entry.key,
          transport: transport,
          endpoint: builtinEndpoint.isNotEmpty
              ? builtinEndpoint
              : (url.isEmpty ? command : url),
          enabled: enabled,
          status: builtin
              ? _builtinMcpStatus(enabled, hasZhipuMcpCredential)
              : (enabled ? 'enabled' : 'disabled'),
          sourceKind: builtin ? 'builtIn' : 'user',
          mutationPolicy: builtin ? 'lockedIdentity' : 'userEditable',
        ),
      );
    }
  }

  addServers(config['mcpServers'], builtin: false);
  addServers(config['builtinMcpServers'], builtin: true);
  return servers;
}

String _mcpTransport(Object? value) {
  return switch (_string(value)) {
    'stdio' => 'stdio',
    'streamableHttp' => 'streamableHttp',
    final label => throw FormatException('Unknown MCP transport: $label'),
  };
}

String _builtinMcpEndpoint(String serverId) {
  return switch (serverId) {
    'zhipu_search' => 'https://open.bigmodel.cn/api/mcp/web_search_prime/mcp',
    'zhipu_reader' => 'https://open.bigmodel.cn/api/mcp/web_reader/mcp',
    'zhipu_zread' => 'https://open.bigmodel.cn/api/mcp/zread/mcp',
    'zhipu_vision' => 'npx',
    _ => '',
  };
}

String _builtinMcpTransport(String serverId) {
  return serverId == 'zhipu_vision' ? 'stdio' : 'streamableHttp';
}

String _builtinMcpStatus(bool enabled, bool hasCredential) {
  if (!enabled) {
    return 'disabled';
  }
  return hasCredential ? 'enabled' : 'missingCredential';
}

bool _hasZhipuMcpCredential(Map<String, Object?> config) {
  final providers = _map(config['providers']);
  bool hasToken(MapEntry<String, Object?> entry) {
    final provider = _map(entry.value);
    return _bool(provider['hasBearerToken']);
  }

  if (providers.entries.any(
    (entry) =>
        hasToken(entry) &&
        _providerTemplateKind(_map(entry.value)) == 'zhipu-coding-plan',
  )) {
    return true;
  }
  return false;
}
