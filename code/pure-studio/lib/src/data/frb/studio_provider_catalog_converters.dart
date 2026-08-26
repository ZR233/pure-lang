part of 'studio_api.dart';

ProviderCatalogView providerCatalogFromFrb(
  frb.BridgeProviderCatalogSnapshot snapshot,
) {
  return ProviderCatalogView(
    schemaVersion: snapshot.schemaVersion,
    revision: snapshot.revision,
    presets: [
      for (final preset in snapshot.presets)
        ProviderPresetView(
          id: preset.id,
          displayName: preset.displayName,
          description: preset.description ?? '',
          baseUrl: preset.baseUrl,
          credentialLabel: preset.credentialLabel,
          credentialEnv: preset.credentialEnv ?? '',
          modelCatalogId: preset.modelCatalogId,
          suggestedModel: preset.suggestedModel,
          hostedWebSearch: preset.serviceCapabilities.webSearch.hostedResponses,
          standaloneWebSearch:
              preset.serviceCapabilities.webSearch.standalone ?? '',
          promptCacheDialect: preset.serviceCapabilities.promptCacheDialect,
          responsesProgrammaticToolCalling:
              preset.serviceCapabilities.responsesProgrammaticToolCalling,
          iconKey: preset.iconKey,
        ),
    ],
    modelCatalogs: {
      for (final catalog in snapshot.modelCatalogs)
        catalog.id: [
          for (final model in catalog.models) _providerModelFromCatalog(model),
        ],
    },
  );
}

ProviderModelView _providerModelFromCatalog(frb.BridgeModelDescriptor model) {
  final capabilities = <String>[
    if (model.capabilities.streaming) 'streaming',
    if (model.capabilities.temperature) 'temperature',
    if (model.capabilities.reasoning) 'reasoning',
    if (model.capabilities.webSearch) 'web search',
    if (model.capabilities.functionCalling) 'tools',
    if (model.capabilities.parallelToolCalls) 'parallel tools',
    if (model.capabilities.customTools) 'custom tools',
    if (model.capabilities.freeformTools) 'freeform tools',
  ];
  final reasoning = model.reasoning;
  final pricing = model.pricing;
  return ProviderModelView(
    slug: model.id,
    displayName: model.displayName,
    description: model.description ?? '',
    contextWindow: model.contextWindow?.toInt(),
    maxContextWindow: model.maxContextWindow?.toInt(),
    maxOutputTokens: model.maxOutputTokens?.toInt(),
    modalities: model.modalities,
    capabilities: capabilities,
    reasoningEfforts: reasoning?.candidates ?? const [],
    reasoningLabel: reasoning?.label ?? '',
    defaultReasoningEffort: reasoning?.defaultCandidate ?? '',
    currency: pricing?.currency ?? '',
    inputPricePerMTok: pricing?.inputPerMtok,
    outputPricePerMTok: pricing?.outputPerMtok,
    cacheReadPricePerMTok: pricing?.cacheReadPerMtok,
    cacheWritePricePerMTok: pricing?.cacheWritePerMtok,
    wireProtocol: model.transport.protocol,
    supportedConnectionModes: [
      for (final mode in model.transport.connectionModes) mode.id,
    ],
    defaultConnectionMode: model.transport.defaultConnectionMode,
    connectionMode: model.transport.defaultConnectionMode,
  );
}
