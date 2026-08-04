part of 'studio_api.dart';

StudioState _stateFromTypedSnapshot({
  required List<StudioProject> projects,
  required List<StudioThread> threads,
  required String? selectedProjectId,
  required String? selectedThreadId,
  List<StudioRecoveryIssue> recoveryIssues = const [],
  TaskRuntimeView? selectedTask,
  required frb.BridgeStudioSettingsDto settings,
}) {
  final selectedRootThreadId = threads
      .where((thread) => thread.id == selectedThreadId)
      .firstOrNull
      ?.effectiveRootThreadId;
  return StudioState(
    projects: projects,
    threads: threads,
    tasksByRootThread: selectedTask == null || selectedRootThreadId == null
        ? const {}
        : {selectedRootThreadId: selectedTask},
    providers: settings.providers.map(_providerSettingsFromFrb).toList(),
    defaultProviderId: settings.defaultProviderId,
    roles: settings.roles.map(_roleSettingsFromFrb).toList(),
    mcpServers: settings.mcpServers.map(_mcpSettingsFromFrb).toList(),
    instructions: _instructionsSettingsFromFrb(settings.instructions),
    skills: _skillsSettingsFromFrb(settings.skills),
    general: _generalSettingsFromFrb(settings.general),
    webSearch: _webSearchFromFrb(settings.webSearch),
    selectedProjectId: selectedProjectId,
    selectedThreadId: selectedThreadId,
    permissionMode: _permissionMode(settings.permissionMode),
    recoveryIssues: recoveryIssues,
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
