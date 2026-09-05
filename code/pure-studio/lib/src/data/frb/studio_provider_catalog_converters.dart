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
          pricingEnabled: preset.pricingEnabled,
          id: preset.id,
          displayName: preset.displayName,
          description: preset.description ?? '',
          baseUrl: preset.baseUrl,
          credentialLabel: preset.credentialLabel,
          credentialEnv: preset.credentialEnv ?? '',
          modelCatalogId: preset.modelCatalogId,
          suggestedModel: preset.suggestedModel,
          hostedWebSearch: preset.serviceCapabilities.webSearch.hostedResponses,
          hostedWebSearchDialect:
              preset.serviceCapabilities.webSearch.hostedDialect,
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
    inputCapabilities: [
      for (final capability in model.capabilities.input)
        ModelInputCapabilityView(
          modality: _modelModalityFromFrb(capability.modality),
          sources: [
            for (final source in capability.sources)
              switch (source) {
                frb.BridgeModelInputSource.local => ModelInputSourceView.local,
                frb.BridgeModelInputSource.remoteUrl =>
                  ModelInputSourceView.remoteUrl,
              },
          ],
          maxCount: capability.maxCount,
          maxBytes: capability.maxBytes?.toInt(),
          maxTotalBytes: capability.maxTotalBytes?.toInt(),
          maxWidth: capability.maxWidth,
          maxHeight: capability.maxHeight,
          mediaTypes: capability.mediaTypes,
        ),
    ],
    outputModalities: [
      for (final modality in model.capabilities.output)
        _modelModalityFromFrb(modality),
    ],
    capabilities: capabilities,
    reasoningEfforts: reasoning?.candidates ?? const [],
    reasoningLabel: reasoning?.label ?? '',
    defaultReasoningEffort: reasoning?.defaultCandidate ?? '',
    currency: pricing?.currency ?? '',
    priceTiers: [
      for (final tier in pricing?.tiers ?? <frb.BridgeModelPriceTier>[])
        ProviderPriceTierView(
          label: tier.label,
          input: tier.inputPerMtok,
          output: tier.outputPerMtok,
          cacheRead: tier.cacheReadPerMtok,
          cacheWrite: tier.cacheWritePerMtok,
        ),
    ],
    wireProtocol: model.transport.protocol,
    supportedConnectionModes: [
      for (final mode in model.transport.connectionModes) mode.id,
    ],
    defaultConnectionMode: model.transport.defaultConnectionMode,
    connectionMode: model.transport.defaultConnectionMode,
  );
}

ModelModalityView _modelModalityFromFrb(frb.BridgeModelModality modality) {
  return switch (modality) {
    frb.BridgeModelModality.text => ModelModalityView.text,
    frb.BridgeModelModality.image => ModelModalityView.image,
    frb.BridgeModelModality.audio => ModelModalityView.audio,
    frb.BridgeModelModality.video => ModelModalityView.video,
    frb.BridgeModelModality.file => ModelModalityView.file,
  };
}
