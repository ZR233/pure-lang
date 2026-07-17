part of 'studio_api.dart';

String? _defaultProviderIdFromConfig(Map<String, Object?> config) {
  final value = _string(
    _firstValue(config, const [
      'defaultProviderId',
      'default_provider_id',
      'defaultProvider',
      'default_provider',
    ]),
  ).trim();
  if (value.isNotEmpty) {
    return value;
  }
  final roles = _map(config['roles']);
  final planner = _map(roles['planner']);
  final plannerProvider = _string(
    _firstValue(planner, const ['provider', 'providerId', 'provider_id']),
  ).trim();
  if (plannerProvider.isNotEmpty) {
    return plannerProvider;
  }
  final providers = _map(config['providers']);
  return providers.keys.firstOrNull;
}

List<ProviderSettingsView> _providersFromConfig(Map<String, Object?> config) {
  final providers = _map(config['providers']);
  return providers.entries.map((entry) {
    final value = _map(entry.value);
    final templateKind = _providerTemplateKind(value);
    final providerModels = _providerModels(value['models']);
    final projectedCustomModels = _providerModels(
      _firstValue(value, const ['customModels', 'custom_models']),
    );
    final customSlugs = projectedCustomModels
        .map((model) => model.slug)
        .toSet();
    final defaultModels = providerModels
        .where((model) => !customSlugs.contains(model.slug))
        .toList();
    final customModels = projectedCustomModels.isEmpty
        ? providerModels
              .where((model) => customSlugs.contains(model.slug))
              .toList()
        : projectedCustomModels;
    final visibleModels = providerModels;
    final defaultModel = _string(
      _firstValue(value, const ['defaultModel', 'default_model']),
    );
    final hasBearerToken = _boolWithDefault(
      _firstValue(value, const ['hasBearerToken', 'has_bearer_token']),
      _string(
        _firstValue(value, const ['bearerToken', 'bearer_token']),
      ).trim().isNotEmpty,
    );
    final name = _string(
      _firstValue(value, const ['displayName', 'display_name', 'name']),
      fallback: entry.key,
    );
    return ProviderSettingsView(
      id: entry.key,
      templateKind: templateKind,
      name: name,
      subtitle: '$name Platform',
      baseUrl: _string(_firstValue(value, const ['baseUrl', 'base_url'])),
      bearerToken: '',
      hasBearerToken: hasBearerToken,
      defaultModel: defaultModel,
      models: visibleModels,
      defaultModels: defaultModels.isEmpty ? providerModels : defaultModels,
      customModels: customModels,
      status: hasBearerToken ? 'ready' : 'missingCredential',
      usageLabel: visibleModels.isEmpty
          ? defaultModel
          : '${visibleModels.length} models',
      modelCount: '${visibleModels.length}',
      updatedAt: 'Loaded',
      wireProtocol: _string(
        value['wireProtocol'],
        fallback: 'chat_completions',
      ),
      connectionMode: _string(
        _firstValue(value, const ['connectionMode', 'connection_mode']),
        fallback: 'http',
      ),
      catalogId: _string(
        _firstValue(_map(value['catalog']), const [
          'catalog',
          'catalogId',
          'catalog_id',
        ]),
      ),
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
          displayName: _string(
            _firstValue(model, const ['displayName', 'display_name']),
            fallback: slug,
          ),
          description: _string(model['description']),
          contextWindow: _nullableInt(
            _firstValue(model, const ['contextWindow', 'context_window']),
          ),
          maxOutputTokens: _nullableInt(
            _firstValue(model, const ['maxOutputTokens', 'max_output_tokens']),
          ),
          currency: _string(model['currency']),
          inputPricePerMTok: _nullableDouble(
            _firstValue(model, const [
              'inputPricePerMTok',
              'input_price_per_mtok',
            ]),
          ),
          outputPricePerMTok: _nullableDouble(
            _firstValue(model, const [
              'outputPricePerMTok',
              'output_price_per_mtok',
            ]),
          ),
          cacheReadPricePerMTok: _nullableDouble(
            _firstValue(model, const [
              'cacheReadPricePerMTok',
              'cache_read_price_per_mtok',
            ]),
          ),
          baseInstructions: _string(
            _firstValue(model, const ['baseInstructions', 'base_instructions']),
          ),
          reasoningEfforts: _modelReasoningEfforts(model),
        );
      })
      .where((model) => model.slug.isNotEmpty)
      .toList();
}

String _providerTemplateKind(Map<String, Object?> provider) {
  return _string(
    _firstValue(provider, const [
      'presetId',
      'preset_id',
      'templateKind',
      'template_kind',
    ]),
  );
}

List<String> _modelReasoningEfforts(Map<String, Object?> model) {
  final direct = _stringList(
    _firstValue(model, const ['reasoningEfforts', 'reasoning_efforts']),
  );
  if (direct.isNotEmpty) {
    return direct;
  }
  final efforts = <String>{};
  for (final parameterValue in _list(model['parameters'])) {
    final parameter = _map(parameterValue);
    if (_string(parameter['name']) != 'effort') {
      continue;
    }
    efforts.addAll(_stringList(parameter['candidates']));
  }
  return efforts.toList();
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
    baseOverride: _string(
      _firstValue(instructions, const ['baseOverride', 'base_override']),
    ),
    developer: _string(instructions['developer']),
    user: _string(instructions['user']),
    projectDocMaxBytes: _int(
      _firstValue(instructions, const [
        'projectDocMaxBytes',
        'project_doc_max_bytes',
      ]),
      fallback: 65536,
    ),
    projectDocFallbackFilenames: _stringList(
      _firstValue(instructions, const [
        'projectDocFallbackFilenames',
        'project_doc_fallback_filenames',
      ]),
    ),
  );
}

SkillsSettingsView _skillsFromConfig(Map<String, Object?> config) {
  final skills = _map(config['skills']);
  final system = _map(skills['system']);
  return SkillsSettingsView(
    enabled: _boolWithDefault(skills['enabled'], true),
    autoLearn: _boolWithDefault(
      _firstValue(skills, const ['autoLearn', 'auto_learn']),
      true,
    ),
    systemEnabled: _boolWithDefault(system['enabled'], true),
    projectDir: _string(
      _firstValue(skills, const ['projectDir', 'project_dir']),
      fallback: 'skills',
    ),
    userDir: _string(
      _firstValue(skills, const ['userDir', 'user_dir']),
      fallback: '~/.pure/skills',
    ),
    externalDirs: _stringList(
      _firstValue(skills, const ['externalDirs', 'external_dirs']),
    ),
    disabled: _stringList(skills['disabled']),
    autoLearnMinToolCalls: _int(
      _firstValue(skills, const [
        'autoLearnMinToolCalls',
        'auto_learn_min_tool_calls',
      ]),
      fallback: 5,
    ),
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
      final transport = _string(
        server['transport'],
        fallback: _string(server['type']),
      );
      final command = _string(server['command']);
      final url = _string(server['url'], fallback: _string(server['endpoint']));
      final enabled = builtin
          ? _boolWithDefault(server['enabled'], true)
          : !_bool(server['disabled']) &&
                _boolWithDefault(server['enabled'], true) &&
                _string(server['status'], fallback: 'enabled') != 'disabled';
      servers.add(
        McpServerSettingsView(
          id: entry.key,
          transport: transport.isEmpty
              ? (builtin ? _builtinMcpTransport(entry.key) : 'stdio')
              : transport,
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

  addServers(
    _firstValue(config, const ['mcpServers', 'mcp_servers']),
    builtin: false,
  );
  addServers(
    _firstValue(config, const ['builtinMcpServers', 'builtin_mcp_servers']),
    builtin: true,
  );
  return servers;
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
