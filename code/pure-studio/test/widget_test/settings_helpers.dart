part of '../widget_test.dart';

ProviderSettingsView _providerFromCommand(
  ProviderCommand command,
  ProviderSettingsView? previous,
) {
  final preset = _testProviderCatalog.preset(command.templateKind);
  final defaults = _testProviderCatalog.modelsFor(preset?.modelCatalogId ?? '');
  final custom = [
    for (final model in command.customModels)
      ProviderModelView(
        slug: model.slug,
        displayName: model.displayName,
        reasoningEfforts: const [],
        contextWindow: model.contextWindow,
        maxOutputTokens: model.maxOutputTokens,
        wireProtocol: model.wireProtocol,
      ),
  ];
  final keep = previous?.templateKind == command.templateKind ? previous : null;
  return ProviderSettingsView(
    id: command.id,
    templateKind: command.templateKind,
    name: command.name,
    baseUrl: command.baseUrl,
    pricingEnabled: command.pricingEnabled,
    hasBearerToken: switch (command.secret.action) {
      ProviderSecretAction.replace => true,
      ProviderSecretAction.preserve => previous?.hasBearerToken ?? false,
      ProviderSecretAction.clear => false,
    },
    defaultModel: command.defaultModel,
    models: [...defaults, ...custom],
    defaultModels: defaults,
    customModels: custom,
    modelConnectionModes: {
      for (final connection in command.modelConnectionModes)
        connection.slug: connection.connectionMode,
    },
    status: 'ready',
    usageLabel: '',
    catalogId: preset?.modelCatalogId ?? '',
    capabilitySource: keep?.capabilitySource ?? 'preset_defaults',
    hostedWebSearch: keep?.hostedWebSearch ?? preset?.hostedWebSearch ?? false,
    hostedWebSearchDialect:
        keep?.hostedWebSearchDialect ??
        preset?.hostedWebSearchDialect ??
        'open_ai_responses',
    standaloneWebSearch:
        keep?.standaloneWebSearch ?? preset?.standaloneWebSearch ?? '',
    promptCacheDialect:
        keep?.promptCacheDialect ?? preset?.promptCacheDialect ?? 'none',
    responsesProgrammaticToolCalling:
        keep?.responsesProgrammaticToolCalling ??
        preset?.responsesProgrammaticToolCalling ??
        false,
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
