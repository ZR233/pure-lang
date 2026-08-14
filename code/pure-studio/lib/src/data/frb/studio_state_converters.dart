part of 'studio_api.dart';

ObservedStateMeta _observedMetaFromFrb(frb.BridgeObservedStateMeta meta) {
  return meta.phase.when(
    uninitialized: () => _observedMeta(meta, ObservedStatePhase.uninitialized),
    ready: () => _observedMeta(meta, ObservedStatePhase.ready),
    running: (operation, operationId) => _observedMeta(
      meta,
      ObservedStatePhase.running,
      operation: operation.name,
      operationId: operationId,
    ),
    failed: (operation, error) => _observedMeta(
      meta,
      ObservedStatePhase.failed,
      operation: operation.name,
      errorCode: error.code,
      errorMessage: error.message,
      retryable: error.retryable,
    ),
    stopped: () => _observedMeta(meta, ObservedStatePhase.stopped),
  );
}

ObservedStateMeta _observedMeta(
  frb.BridgeObservedStateMeta meta,
  ObservedStatePhase phase, {
  String? operation,
  String? operationId,
  String? errorCode,
  String? errorMessage,
  bool retryable = false,
}) {
  return ObservedStateMeta(
    revision: meta.revision.toInt(),
    phase: phase,
    updatedAt: _dateFromUnix(meta.updatedAt),
    lastCheckedAt: meta.lastCheckedAt == null
        ? null
        : _dateFromUnix(meta.lastCheckedAt!),
    stale: meta.stale,
    operation: operation,
    operationId: operationId,
    errorCode: errorCode,
    errorMessage: errorMessage,
    retryable: retryable,
  );
}

SettingsStateSnapshot _settingsStateFromFrb(
  frb.BridgeSettingsStateSnapshot snapshot,
) {
  final settings = snapshot.settings;
  return SettingsStateSnapshot(
    meta: _observedMetaFromFrb(snapshot.meta),
    providers: settings.providers.map(_providerSettingsFromFrb).toList(),
    defaultProviderId: settings.defaultProviderId,
    roles: settings.roles.map(_roleSettingsFromFrb).toList(),
    mcpServers: settings.mcpServers.map(_mcpSettingsFromFrb).toList(),
    instructions: _instructionsSettingsFromFrb(settings.instructions),
    skills: _skillsSettingsFromFrb(settings.skills),
    general: _generalSettingsFromFrb(settings.general),
    webSearch: _webSearchFromFrb(settings.webSearch),
    permissionMode: _permissionMode(settings.permissionMode),
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

String _interactionTitle(InteractionKind kind, InteractionPayload payload) {
  return switch (payload) {
    ToolApprovalInteractionPayload(:final toolName) =>
      toolName.isEmpty ? 'Tool approval' : toolName,
    UserInputInteractionPayload() => 'User input requested',
    PlanConfirmationInteractionPayload() => 'Plan confirmation',
    UnknownInteractionPayload() => switch (kind) {
      InteractionKind.toolApproval => 'Tool approval',
      InteractionKind.userInput => 'User input requested',
      InteractionKind.planConfirmation => 'Plan confirmation',
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
    PlanConfirmationInteractionPayload(:final content) => content,
    UnknownInteractionPayload() => '',
  };
}
