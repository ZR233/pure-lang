part of '../widget_test.dart';

ProviderSettingsView _providerFromSettings(Object? value) {
  final json = value as Map<String, Object?>;
  final defaultModel = json['defaultModel'] as String? ?? '';
  return ProviderSettingsView(
    id: json['id'] as String? ?? '',
    templateKind: json['templateKind'] as String? ?? 'openai',
    name: json['name'] as String? ?? '',
    baseUrl: json['baseUrl'] as String? ?? '',
    bearerToken: '',
    hasBearerToken: (json['bearerToken'] as String? ?? '').isNotEmpty,
    defaultModel: defaultModel,
    models: [
      ProviderModelView(
        slug: defaultModel,
        displayName: defaultModel,
        reasoningEfforts: const ['high'],
      ),
    ],
    status: 'ready',
    usageLabel: '1 models',
  );
}

class _RoleUpdate {
  const _RoleUpdate({
    required this.roleKey,
    required this.providerId,
    required this.model,
    required this.effort,
  });

  final String roleKey;
  final String providerId;
  final String model;
  final String? effort;
}
