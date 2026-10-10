part of 'studio_api.dart';

ObservedResource<T> _uninitializedResource<T>(
  frb.BridgeUninitializedResource resource,
) => UninitializedObservedResource<T>(
  revision: resource.revision.toInt(),
  updatedAt: resource.updatedAt.toInt(),
);

ObservedResource<T> _loadingResource<T>(frb.BridgeLoadingResource resource) =>
    LoadingObservedResource<T>(
      revision: resource.revision.toInt(),
      operation: resource.operation.name,
      operationId: resource.operationId,
      startedAt: resource.startedAt.toInt(),
    );

ObservedResource<T> _readyResource<T>(
  frb.BridgeReadyResource resource,
  T value,
) => ReadyObservedResource<T>(
  revision: resource.revision.toInt(),
  updatedAt: resource.updatedAt.toInt(),
  lastCheckedAt: resource.lastCheckedAt?.toInt(),
  value: value,
);

ObservedResource<T> _refreshingResource<T>(
  frb.BridgeRefreshingResource resource,
  T value,
) => RefreshingObservedResource<T>(
  revision: resource.revision.toInt(),
  operation: resource.operation.name,
  operationId: resource.operationId,
  startedAt: resource.startedAt.toInt(),
  lastCheckedAt: resource.lastCheckedAt?.toInt(),
  value: value,
);

ObservedResource<T> _staleResource<T>(
  frb.BridgeStaleResource resource,
  T value,
) => StaleObservedResource<T>(
  revision: resource.revision.toInt(),
  staleAt: resource.staleAt.toInt(),
  lastCheckedAt: resource.lastCheckedAt?.toInt(),
  value: value,
);

ObservedResourceError _resourceError(frb.BridgeStateError error) =>
    ObservedResourceError(
      code: error.code,
      message: error.message,
      retryable: error.retryable,
    );

ObservedResource<T> _degradedResource<T>(
  frb.BridgeDegradedResource resource,
  T value,
) => DegradedObservedResource<T>(
  revision: resource.revision.toInt(),
  failedAt: resource.failedAt.toInt(),
  lastCheckedAt: resource.lastCheckedAt?.toInt(),
  operation: resource.operation.name,
  error: _resourceError(resource.error),
  value: value,
);

ObservedResource<T> _failedResource<T>(frb.BridgeFailedResource resource) =>
    FailedObservedResource<T>(
      revision: resource.revision.toInt(),
      failedAt: resource.failedAt.toInt(),
      operation: resource.operation.name,
      error: _resourceError(resource.error),
    );

ObservedResource<T> _stoppedResource<T>(frb.BridgeStoppedResource resource) =>
    StoppedObservedResource<T>(
      revision: resource.revision.toInt(),
      stoppedAt: resource.stoppedAt.toInt(),
    );

SettingsStateSnapshot _settingsStateFromFrb(
  frb.BridgeSettingsStateResponse response,
) {
  final catalogByProvider = {
    for (final provider in response.catalog.providers) provider.id: provider,
  };
  final settings = response.config.settings;
  final data = _settingsDataFromFrb(
    settings,
    catalogByProvider: catalogByProvider,
  );
  return SettingsStateSnapshot(
    providers: data.providers,
    defaultProviderId: data.defaultProviderId,
    modeModelRoutes: data.modeModelRoutes,
    roles: data.roles,
    mcpServers: data.mcpServers,
    instructions: data.instructions,
    skills: data.skills,
    general: data.general,
    webSearch: data.webSearch,
    deepSeekWebSearch: data.deepSeekWebSearch,
    permissionMode: data.permissionMode,
    revision: response.config.revision.toInt(),
    modelCatalogRevision: response.catalog.revision.toInt(),
  );
}

SettingsStateData _settingsDataFromFrb(
  frb.BridgeStudioSettingsDto settings, {
  Map<String, frb.BridgeModelCatalogProviderDto> catalogByProvider = const {},
}) {
  return SettingsStateData(
    providers: settings.providers
        .map(
          (provider) => _providerSettingsFromFrb(
            provider,
            catalog: catalogByProvider[provider.id],
          ),
        )
        .toList(),
    defaultProviderId: settings.defaultProviderId,
    modeModelRoutes: settings.modeModelRoutes
        .map(_modeModelRouteFromFrb)
        .toList(),
    roles: settings.roles.map(_roleSettingsFromFrb).toList(),
    mcpServers: settings.mcpServers.map(_mcpSettingsFromFrb).toList(),
    instructions: _instructionsSettingsFromFrb(settings.instructions),
    skills: _skillsSettingsFromFrb(settings.skills),
    general: _generalSettingsFromFrb(settings.general),
    webSearch: _webSearchFromFrb(settings.webSearch),
    deepSeekWebSearch: _deepSeekWebSearchFromFrb(settings.deepseekWebSearch),
    permissionMode: _permissionMode(settings.permissionMode),
  );
}

SettingsStateSnapshot _settingsConfigStateFromFrb(
  frb.BridgeSettingsConfigStateSnapshot snapshot,
) {
  SettingsStateData convert(frb.BridgeSettingsConfigStateData data) {
    return _settingsDataFromFrb(data.settings);
  }

  return SettingsStateSnapshot.fromState(
    state: switch (snapshot) {
      frb.BridgeSettingsConfigStateSnapshot_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeSettingsConfigStateSnapshot_Loading(:final field0) =>
        _loadingResource(field0),
      frb.BridgeSettingsConfigStateSnapshot_Ready(
        :final resource,
        :final value,
      ) =>
        _readyResource(resource, convert(value)),
      frb.BridgeSettingsConfigStateSnapshot_Refreshing(
        :final resource,
        :final value,
      ) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeSettingsConfigStateSnapshot_Stale(
        :final resource,
        :final value,
      ) =>
        _staleResource(resource, convert(value)),
      frb.BridgeSettingsConfigStateSnapshot_Degraded(
        :final resource,
        :final value,
      ) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeSettingsConfigStateSnapshot_Failed(:final field0) =>
        _failedResource(field0),
      frb.BridgeSettingsConfigStateSnapshot_Stopped(:final field0) =>
        _stoppedResource(field0),
    },
  );
}

SettingsStateSnapshot _modelCatalogStateFromFrb(
  frb.BridgeModelCatalogStateSnapshot snapshot,
) {
  SettingsStateData convert(frb.BridgeModelCatalogStateData data) {
    return SettingsStateData(
      providers: data.providers.map(_providerCatalogSettingsFromFrb).toList(),
    );
  }

  return SettingsStateSnapshot(
    providers: switch (snapshot) {
      frb.BridgeModelCatalogStateSnapshot_Ready(:final value) => convert(
        value,
      ).providers,
      frb.BridgeModelCatalogStateSnapshot_Refreshing(:final value) => convert(
        value,
      ).providers,
      frb.BridgeModelCatalogStateSnapshot_Stale(:final value) => convert(
        value,
      ).providers,
      frb.BridgeModelCatalogStateSnapshot_Degraded(:final value) => convert(
        value,
      ).providers,
      _ => const [],
    },
    modelCatalogRevision: switch (snapshot) {
      frb.BridgeModelCatalogStateSnapshot_Uninitialized(:final field0) =>
        field0.revision.toInt(),
      frb.BridgeModelCatalogStateSnapshot_Loading(:final field0) =>
        field0.revision.toInt(),
      frb.BridgeModelCatalogStateSnapshot_Ready(:final resource) =>
        resource.revision.toInt(),
      frb.BridgeModelCatalogStateSnapshot_Refreshing(:final resource) =>
        resource.revision.toInt(),
      frb.BridgeModelCatalogStateSnapshot_Stale(:final resource) =>
        resource.revision.toInt(),
      frb.BridgeModelCatalogStateSnapshot_Degraded(:final resource) =>
        resource.revision.toInt(),
      frb.BridgeModelCatalogStateSnapshot_Failed(:final field0) =>
        field0.revision.toInt(),
      frb.BridgeModelCatalogStateSnapshot_Stopped(:final field0) =>
        field0.revision.toInt(),
    },
  );
}

SettingsStateSnapshot _modelCatalogSnapshotFromFrb(
  frb.BridgeModelCatalogSnapshotDto snapshot,
) {
  return SettingsStateSnapshot(
    providers: snapshot.providers.map(_providerCatalogSettingsFromFrb).toList(),
    modelCatalogRevision: snapshot.revision.toInt(),
  );
}

ProviderSettingsView _providerCatalogSettingsFromFrb(
  frb.BridgeModelCatalogProviderDto value,
) {
  final models = value.effectiveModels.map(_providerModelFromCatalog).toList();
  return ProviderSettingsView(
    id: value.id,
    name: value.id,
    baseUrl: '',
    defaultModel: models.isEmpty ? '' : models.first.slug,
    models: models,
    modelCatalog: _modelCatalogStatusFromFrb(value.modelCatalog),
    status: 'catalog',
    usageLabel: '${models.length} models',
    modelCount: '${models.length}',
  );
}

WebSearchSettingsView _webSearchFromFrb(frb.BridgeWebSearchSettingsDto value) {
  return WebSearchSettingsView(
    configuredMode: value.configuredMode,
    effectiveMode: value.effectiveMode,
    availability: value.availability,
    contextSize: value.contextSize,
    allowedDomains: value.allowedDomains,
    country: value.country,
    region: value.region,
    city: value.city,
    timezone: value.timezone,
    providerId: value.providerId,
    model: value.model,
  );
}

DeepSeekWebSearchSettingsView _deepSeekWebSearchFromFrb(
  frb.BridgeDeepSeekWebSearchSettingsDto value,
) {
  return DeepSeekWebSearchSettingsView(
    configuredEnabled: value.configuredEnabled,
    effectiveEnabled: value.effectiveEnabled,
    availability: value.availability,
    providerId: value.providerId,
    model: value.model,
  );
}

String _interactionTitle(InteractionKind kind, InteractionPayload payload) {
  return switch (payload) {
    ToolApprovalInteractionPayload(:final toolName) =>
      toolName.isEmpty ? 'Tool approval' : toolName,
    UserInputInteractionPayload() => 'User input requested',
    UnknownInteractionPayload() => switch (kind) {
      InteractionKind.toolApproval => 'Tool approval',
      InteractionKind.userInput => 'User input requested',
    },
  };
}

String _interactionBody(InteractionKind kind, InteractionPayload payload) {
  return switch (payload) {
    ToolApprovalInteractionPayload(:final arguments) => _jsonText(arguments),
    UserInputInteractionPayload(:final questions) =>
      questions
          .map((question) => question.question)
          .where((question) => question.isNotEmpty)
          .join('\n'),
    UnknownInteractionPayload() => '',
  };
}
