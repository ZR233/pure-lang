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
  frb.BridgeSettingsStateSnapshot snapshot,
) {
  SettingsStateData convert(frb.BridgeSettingsStateData data) {
    final settings = data.settings;
    return SettingsStateData(
      providers: settings.providers.map(_providerSettingsFromFrb).toList(),
      defaultProviderId: settings.defaultProviderId,
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

  return SettingsStateSnapshot.fromState(
    state: switch (snapshot) {
      frb.BridgeSettingsStateSnapshot_Uninitialized(:final field0) =>
        _uninitializedResource(field0),
      frb.BridgeSettingsStateSnapshot_Loading(:final field0) =>
        _loadingResource(field0),
      frb.BridgeSettingsStateSnapshot_Ready(:final resource, :final value) =>
        _readyResource(resource, convert(value)),
      frb.BridgeSettingsStateSnapshot_Refreshing(
        :final resource,
        :final value,
      ) =>
        _refreshingResource(resource, convert(value)),
      frb.BridgeSettingsStateSnapshot_Stale(:final resource, :final value) =>
        _staleResource(resource, convert(value)),
      frb.BridgeSettingsStateSnapshot_Degraded(:final resource, :final value) =>
        _degradedResource(resource, convert(value)),
      frb.BridgeSettingsStateSnapshot_Failed(:final field0) => _failedResource(
        field0,
      ),
      frb.BridgeSettingsStateSnapshot_Stopped(:final field0) =>
        _stoppedResource(field0),
    },
  );
}

WebSearchSettingsView _webSearchFromFrb(frb.BridgeWebSearchSettingsDto value) {
  return WebSearchSettingsView(
    configuredMode: value.configuredMode,
    effectiveMode: value.effectiveMode,
    availability: value.availability,
    selected: value.selected,
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
    selected: value.selected,
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
