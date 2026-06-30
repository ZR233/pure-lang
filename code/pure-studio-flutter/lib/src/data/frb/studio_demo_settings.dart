part of 'studio_api.dart';

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
  if (provider.templateKind == 'deepseek') {
    return ProviderUsageView(
      providerId: provider.id,
      updatedAt: now,
      status: 'ready',
      usageKind: 'deepseekBalance',
      balance: const DeepSeekBalanceUsageView(
        isAvailable: true,
        balances: [
          DeepSeekBalanceInfoView(
            currency: 'CNY',
            totalBalance: '126.40',
            grantedBalance: '26.40',
            toppedUpBalance: '100.00',
          ),
        ],
      ),
    );
  }
  if (provider.templateKind == 'zhipu-coding-plan') {
    return ProviderUsageView(
      providerId: provider.id,
      updatedAt: now,
      status: 'ready',
      usageKind: 'zhipuCodingPlan',
      codingPlan: const ZhipuCodingPlanUsageView(
        level: 'Pro',
        limits: [
          ZhipuQuotaLimitView(
            window: 'fiveHour',
            label: '5h',
            percentage: 32,
            remaining: 68000,
            total: 100000,
            usageDetails: [],
          ),
          ZhipuQuotaLimitView(
            window: 'weekly',
            label: '7d',
            percentage: 54,
            remaining: 460000,
            total: 1000000,
            usageDetails: [],
          ),
          ZhipuQuotaLimitView(
            window: 'mcpMonthly',
            label: 'MCP',
            percentage: 18,
            remaining: 82,
            total: 100,
            usageDetails: [
              ZhipuToolUsageDetailView(
                name: 'search',
                currentValue: 12,
                total: 100,
                percentage: 12,
              ),
            ],
          ),
        ],
      ),
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
}) {
  return _list(settings['providers']).map((value) {
    final provider = _map(value);
    final customModels = _list(provider['customModels'])
        .map(_providerSettingsModelFromJson)
        .where((model) => model.slug.isNotEmpty)
        .toList();
    final template = _templateFor(_string(provider['templateKind']));
    final defaultModels = template.defaultModels;
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
      name: _string(provider['name'], fallback: template.name),
      subtitle:
          '${_string(provider['name'], fallback: template.name)} Platform',
      baseUrl: _string(provider['baseUrl'], fallback: template.baseUrl),
      bearerToken: '',
      hasBearerToken: hasToken,
      defaultModel: _string(
        provider['defaultModel'],
        fallback: template.defaultModel,
      ),
      models: models,
      defaultModels: defaultModels,
      customModels: customModels,
      status: hasToken ? 'ready' : 'missingCredential',
      usageLabel: '${models.length} models',
      modelCount: '${models.length}',
      updatedAt: 'Preview',
      providerKind: template.providerKind,
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

_ProviderTemplateDefaults _templateFor(String id) {
  return _providerTemplates.firstWhere(
    (template) => template.id == id,
    orElse: () => _providerTemplates.first,
  );
}

class _ProviderTemplateDefaults {
  const _ProviderTemplateDefaults({
    required this.id,
    required this.name,
    required this.baseUrl,
    required this.defaultModel,
    required this.providerKind,
    required this.defaultModels,
  });

  final String id;
  final String name;
  final String baseUrl;
  final String defaultModel;
  final String providerKind;
  final List<ProviderModelView> defaultModels;
}

const _providerTemplates = [
  _ProviderTemplateDefaults(
    id: 'deepseek',
    name: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com',
    defaultModel: 'deepseek-v4-flash',
    providerKind: 'deep_seek',
    defaultModels: [
      ProviderModelView(
        slug: 'deepseek-v4-flash',
        displayName: 'DeepSeek V4 Flash',
        reasoningEfforts: ['high', 'max'],
      ),
      ProviderModelView(
        slug: 'deepseek-v4-pro',
        displayName: 'DeepSeek V4 Pro',
        reasoningEfforts: ['high', 'max'],
      ),
    ],
  ),
  _ProviderTemplateDefaults(
    id: 'openai',
    name: 'OpenAI',
    baseUrl: 'https://api.openai.com/v1',
    defaultModel: 'gpt-5.5',
    providerKind: 'open_ai',
    defaultModels: [
      ProviderModelView(
        slug: 'gpt-5.5',
        displayName: 'GPT-5.5',
        reasoningEfforts: ['medium', 'low', 'high', 'xhigh'],
      ),
      ProviderModelView(
        slug: 'gpt-5.4',
        displayName: 'GPT-5.4',
        reasoningEfforts: ['medium', 'low', 'high', 'xhigh'],
      ),
      ProviderModelView(
        slug: 'gpt-5.4-mini',
        displayName: 'GPT-5.4-Mini',
        reasoningEfforts: ['medium', 'low', 'high', 'xhigh'],
      ),
    ],
  ),
  _ProviderTemplateDefaults(
    id: 'zhipu',
    name: 'Zhipu',
    baseUrl: 'https://open.bigmodel.cn/api/paas/v4',
    defaultModel: 'glm-5.2',
    providerKind: 'zhipu',
    defaultModels: [
      ProviderModelView(
        slug: 'glm-5.2',
        displayName: 'GLM-5.2',
        reasoningEfforts: ['enabled', 'none'],
      ),
      ProviderModelView(
        slug: 'glm-5',
        displayName: 'GLM-5',
        reasoningEfforts: ['enabled', 'none'],
      ),
    ],
  ),
  _ProviderTemplateDefaults(
    id: 'zhipu-coding-plan',
    name: 'Zhipu Coding Plan',
    baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
    defaultModel: 'glm-5.2',
    providerKind: 'zhipu',
    defaultModels: [
      ProviderModelView(
        slug: 'glm-5.2',
        displayName: 'GLM-5.2',
        reasoningEfforts: ['enabled', 'none'],
      ),
      ProviderModelView(
        slug: 'glm-5',
        displayName: 'GLM-5',
        reasoningEfforts: ['enabled', 'none'],
      ),
    ],
  ),
];
