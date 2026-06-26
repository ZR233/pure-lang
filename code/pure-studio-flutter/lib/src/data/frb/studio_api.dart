import 'dart:async';
import 'dart:convert';

import '../../domain/models/studio_models.dart';
import '../../rust/api/studio.dart' as frb;
import '../../rust/frb_generated.dart';

abstract class StudioApi {
  Future<StudioState> bootstrap();
  Future<StudioState> openProject(String path);
  Future<StudioState> selectProject(String projectId);
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  });
  Future<StudioState> createSession(String projectId, {String? title});
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  });
  Future<StudioState> setSessionMode(String sessionId, CompileMode mode);
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedSessionId,
  });
  Future<StudioState> loadSessionState(String sessionId);
  Future<List<StudioBridgeEvent>> loadStudioEvents(
    String sessionId, {
    int? afterSequence,
    int limit = 500,
  });
  Stream<Object> subscribeGlobalEvents();
  Stream<Object> subscribeSessionEvents(String sessionId);
  Future<void> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  );
  Future<void> stopPrompt(String sessionId);
  Future<void> resolveInteraction(
    String interactionId,
    Map<String, Object?> resolution,
  );
  Future<void> saveRuntimePermissionMode(PermissionMode mode);
  Future<StudioState> saveProviderSettings(Map<String, Object?> settings);
  Future<StudioState> saveInstructionsSettings(Map<String, Object?> settings);
  Future<StudioState> saveSkillsSettings(Map<String, Object?> settings);
  Future<StudioState> saveMcpSettings(Map<String, Object?> settings);
  Future<StudioState> saveGeneralSettings(Map<String, Object?> settings);
  Future<List<ProviderUsageView>> loadProviderUsages();
  Future<List<String>> listDiscoveredSkills(String projectId);
  Future<void> saveStudioSettingsDraft(
    String section,
    Map<String, Object?> draft,
  );
}

class FrbStudioApi implements StudioApi {
  static Future<void>? _initFuture;

  static Future<void> _ensureReady() {
    return _initFuture ??= () async {
      await RustLib.init();
      await frb.initializeRuntime();
      await frb.startRuntime();
    }();
  }

  @override
  Future<StudioState> bootstrap() async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(await frb.bootstrapStudio());
  }

  @override
  Future<StudioState> openProject(String path) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(await frb.openProject(path: path));
  }

  @override
  Future<StudioState> selectProject(String projectId) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.selectProject(projectId: projectId),
    );
  }

  @override
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.archiveProject(
        projectId: projectId,
        selectedProjectId: selectedProjectId,
      ),
    );
  }

  @override
  Future<StudioState> createSession(String projectId, {String? title}) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.createSession(projectId: projectId, title: title),
    );
  }

  @override
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.archiveSession(
        sessionId: sessionId,
        selectedSessionId: selectedSessionId,
      ),
    );
  }

  @override
  Future<StudioState> setSessionMode(String sessionId, CompileMode mode) async {
    await _ensureReady();
    return studioStateFromFrbSession(
      await frb.setSessionMode(
        sessionId: sessionId,
        mode: _compileModeLabel(mode),
      ),
    );
  }

  @override
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedSessionId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.setModelRole(
        roleKey: roleKey,
        providerId: providerId,
        model: model,
        effort: effort,
        selectedSessionId: selectedSessionId,
      ),
    );
  }

  @override
  Future<StudioState> loadSessionState(String sessionId) async {
    await _ensureReady();
    return studioStateFromFrbSession(
      await frb.loadSessionState(sessionId: sessionId),
    );
  }

  @override
  Future<List<StudioBridgeEvent>> loadStudioEvents(
    String sessionId, {
    int? afterSequence,
    int limit = 500,
  }) async {
    await _ensureReady();
    final response = await frb.loadStudioEvents(
      sessionId: sessionId,
      afterSequence: afterSequence,
      limit: limit,
    );
    return response.events.map(StudioBridgeEvent.fromFrb).toList();
  }

  @override
  Future<void> resolveInteraction(
    String interactionId,
    Map<String, Object?> resolution,
  ) async {
    await _ensureReady();
    await frb.resolveInteraction(
      interactionId: interactionId,
      resolutionJson: jsonEncode(resolution),
    );
  }

  @override
  Future<void> stopPrompt(String sessionId) async {
    await _ensureReady();
    await frb.stopPrompt(sessionId: sessionId);
  }

  @override
  Stream<Object> subscribeGlobalEvents() async* {
    await _ensureReady();
    yield* frb.subscribeGlobalEvents().map(StudioBridgeEvent.fromFrb);
  }

  @override
  Stream<Object> subscribeSessionEvents(String sessionId) async* {
    await _ensureReady();
    yield* frb
        .subscribeSessionEvents(sessionId: sessionId)
        .map(StudioBridgeEvent.fromFrb);
  }

  @override
  Future<void> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  ) async {
    await _ensureReady();
    await frb.submitPrompt(
      sessionId: sessionId,
      prompt: prompt,
      attachmentIds: attachmentIds,
    );
  }

  @override
  Future<void> saveRuntimePermissionMode(PermissionMode mode) async {
    await _ensureReady();
    await frb.saveRuntimePermissionMode(mode: _permissionModeLabel(mode));
  }

  @override
  Future<StudioState> saveProviderSettings(
    Map<String, Object?> settings,
  ) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveProviderSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<StudioState> saveInstructionsSettings(
    Map<String, Object?> settings,
  ) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveInstructionsSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<StudioState> saveSkillsSettings(Map<String, Object?> settings) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveSkillsSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<StudioState> saveMcpSettings(Map<String, Object?> settings) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveMcpSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<StudioState> saveGeneralSettings(Map<String, Object?> settings) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveGeneralSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async {
    await _ensureReady();
    final response = await frb.loadProviderUsages();
    return response.usages.map(_providerUsageFromFrb).toList();
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    await _ensureReady();
    final response = await frb.listDiscoveredSkills(projectId: projectId);
    return response.skills
        .map((skill) => skill.name)
        .where((name) => name.isNotEmpty)
        .toList()
      ..sort();
  }

  @override
  Future<void> saveStudioSettingsDraft(
    String section,
    Map<String, Object?> draft,
  ) async {
    await _ensureReady();
    await frb.saveStudioSettingsDraft(
      section: section,
      draftJson: jsonEncode(draft),
    );
  }
}

class StudioBridgeEvent {
  const StudioBridgeEvent({
    required this.payload,
    this.eventId,
    this.sessionId,
    this.turnId,
    this.sequence,
    this.createdAt,
  });

  factory StudioBridgeEvent.fromFrb(frb.BridgeEventEnvelope event) {
    return StudioBridgeEvent(
      eventId: event.eventId,
      sessionId: event.sessionId,
      turnId: event.turnId,
      sequence: event.sequence,
      createdAt: _dateFromUnix(event.createdAt),
      payload: _bridgePayloadFromFrb(event.payload, sequence: event.sequence),
    );
  }

  final String? eventId;
  final String? sessionId;
  final String? turnId;
  final BigInt? sequence;
  final DateTime? createdAt;
  final StudioBridgeEventPayload payload;
}

sealed class StudioBridgeEventPayload {
  const StudioBridgeEventPayload();

  String? get sessionId => null;
}

final class TurnChangedPayload extends StudioBridgeEventPayload {
  const TurnChangedPayload({required this.turn});

  final StudioTurnView turn;

  @override
  String get sessionId => turn.sessionId;
}

final class MessageUpdatedPayload extends StudioBridgeEventPayload {
  const MessageUpdatedPayload({required this.message});

  final TimelineMessage message;

  @override
  String get sessionId => message.sessionId;
}

final class MessageRemovedPayload extends StudioBridgeEventPayload {
  const MessageRemovedPayload({required this.messageId});

  final String messageId;
}

final class MessagePartUpdatedPayload extends StudioBridgeEventPayload {
  const MessagePartUpdatedPayload({required this.part});

  final TimelinePartSnapshot part;

  @override
  String get sessionId => part.sessionId;
}

final class MessagePartRemovedPayload extends StudioBridgeEventPayload {
  const MessagePartRemovedPayload({
    required this.messageId,
    required this.partId,
  });

  final String messageId;
  final String partId;
}

final class MessagePartDeltaPayload extends StudioBridgeEventPayload {
  const MessagePartDeltaPayload({required this.delta});

  final TimelinePartDelta delta;

  @override
  String get sessionId => delta.sessionId;
}

final class InteractionChangedPayload extends StudioBridgeEventPayload {
  const InteractionChangedPayload({
    required this.interaction,
    required this.status,
  });

  final PendingInteraction interaction;
  final String status;

  @override
  String get sessionId => interaction.sessionId;
}

final class AgentChangedPayload extends StudioBridgeEventPayload {
  const AgentChangedPayload({required this.agent});

  final StudioAgentView agent;

  @override
  String get sessionId => agent.sessionId;
}

final class AgentTimelineChangedPayload extends StudioBridgeEventPayload {
  const AgentTimelineChangedPayload({required this.event});

  final TimelineAgentEvent event;

  @override
  String get sessionId => event.sessionId;
}

final class SessionRuntimeChangedPayload extends StudioBridgeEventPayload {
  const SessionRuntimeChangedPayload({
    required this.runtime,
    required this.sessionId,
  });

  final SessionRuntimeView runtime;

  @override
  final String sessionId;
}

final class SkillActivatedPayload extends StudioBridgeEventPayload {
  const SkillActivatedPayload({required this.name});

  final String name;
}

final class PlanLifecycleChangedPayload extends StudioBridgeEventPayload {
  const PlanLifecycleChangedPayload({required this.state});

  final String state;
}

final class SessionListChangedPayload extends StudioBridgeEventPayload {
  const SessionListChangedPayload({
    required this.projectId,
    required this.sessions,
  });

  final String? projectId;
  final List<StudioSession> sessions;
}

final class McpHealthChangedPayload extends StudioBridgeEventPayload {
  const McpHealthChangedPayload({
    required this.activeMcpServers,
    required this.servers,
  });

  final List<String> activeMcpServers;
  final List<McpServerSettingsView> servers;
}

final class LspHealthChangedPayload extends StudioBridgeEventPayload {
  const LspHealthChangedPayload({required this.activeLspServers});

  final List<String> activeLspServers;
}

final class StalePayload extends StudioBridgeEventPayload {
  const StalePayload({required this.laggedEvents});

  final int laggedEvents;
}

final class IgnoredBridgeEventPayload extends StudioBridgeEventPayload {
  const IgnoredBridgeEventPayload();
}

final class SettingsDraftSavedPayload extends StudioBridgeEventPayload {
  const SettingsDraftSavedPayload({required this.section, required this.saved});

  final String section;
  final bool saved;
}

class StudioTurnView {
  const StudioTurnView({required this.sessionId, required this.status});

  final String sessionId;
  final String status;
}

class StudioAgentView {
  const StudioAgentView({required this.sessionId});

  final String sessionId;
}

StudioBridgeEventPayload _bridgePayloadFromFrb(
  frb.BridgeEventPayload payload, {
  required BigInt sequence,
}) {
  final itemSequence = sequence.toInt();
  return switch (payload) {
    frb.BridgeEventPayload_TurnChanged(:final turn) => TurnChangedPayload(
      turn: StudioTurnView(sessionId: turn.sessionId, status: turn.status),
    ),
    frb.BridgeEventPayload_MessageUpdated(:final message) =>
      MessageUpdatedPayload(
        message: _timelineMessageFromFrb(message, sequence: itemSequence),
      ),
    frb.BridgeEventPayload_MessageRemoved(:final messageId) =>
      MessageRemovedPayload(messageId: messageId),
    frb.BridgeEventPayload_MessagePartUpdated(:final part_) =>
      _isIgnoredTimelinePartType(part_.partType)
          ? const IgnoredBridgeEventPayload()
          : MessagePartUpdatedPayload(
              part: _timelinePartSnapshotFromFrb(part_, sequence: itemSequence),
            ),
    frb.BridgeEventPayload_MessagePartRemoved(
      :final messageId,
      :final partId,
    ) =>
      MessagePartRemovedPayload(messageId: messageId, partId: partId),
    frb.BridgeEventPayload_MessagePartDelta(:final delta) =>
      MessagePartDeltaPayload(delta: _timelinePartDeltaFromFrb(delta)),
    frb.BridgeEventPayload_InteractionChanged(:final event) =>
      _interactionChangedPayloadFromFrb(event),
    frb.BridgeEventPayload_AgentChanged(:final agent) => AgentChangedPayload(
      agent: StudioAgentView(sessionId: agent.sessionId),
    ),
    frb.BridgeEventPayload_AgentTimelineChanged(:final event) =>
      AgentTimelineChangedPayload(event: _timelineAgentEventFromFrb(event)),
    frb.BridgeEventPayload_SessionRuntimeChanged(:final runtime) =>
      SessionRuntimeChangedPayload(
        runtime: _sessionRuntimeFromFrb(runtime),
        sessionId: runtime.sessionId,
      ),
    frb.BridgeEventPayload_SkillActivated(:final activation) =>
      SkillActivatedPayload(name: activation.name),
    frb.BridgeEventPayload_PlanLifecycleChanged(:final event) =>
      PlanLifecycleChangedPayload(state: event.state),
    frb.BridgeEventPayload_SessionHandoffChanged() => throw FormatException(
      'sessionHandoffChanged is not a Flutter bridge event',
    ),
    frb.BridgeEventPayload_SessionListChanged(
      :final projectId,
      :final sessions,
    ) =>
      SessionListChangedPayload(
        projectId: _emptyToNull(projectId),
        sessions: sessions.map(_sessionFromFrb).toList(),
      ),
    frb.BridgeEventPayload_McpHealthChanged(:final health) =>
      McpHealthChangedPayload(
        activeMcpServers: health.activeMcpServers,
        servers: health.mcpServers.map(_mcpServerFromFrb).toList(),
      ),
    frb.BridgeEventPayload_LspHealthChanged(:final health) =>
      LspHealthChangedPayload(activeLspServers: health.activeLspServers),
    frb.BridgeEventPayload_Stale(:final laggedEvents) => StalePayload(
      laggedEvents: laggedEvents.toInt(),
    ),
  };
}

TimelineMessage _timelineMessageFromFrb(
  frb.BridgeStudioMessageDto message, {
  required int sequence,
}) {
  return TimelineMessage(
    id: message.messageId,
    sessionId: message.sessionId,
    role: message.role.isEmpty ? 'assistant' : message.role,
    createdAt: _dateFromUnix(message.createdAt),
    sequence: sequence,
  );
}

TimelinePartSnapshot _timelinePartSnapshotFromFrb(
  frb.BridgeStudioPartDto part, {
  required int sequence,
}) {
  return TimelinePartSnapshot(
    id: part.partId,
    messageId: part.messageId,
    sessionId: part.sessionId,
    turnId: part.turnId,
    type: _partType(part.partType),
    order: part.order.toInt(),
    revision: part.revision.toInt(),
    sequence: sequence,
    text: _frbPartText(part),
    status: part.status.isEmpty ? 'completed' : part.status,
    createdAt: _dateFromUnix(part.createdAt),
    updatedAt: _dateFromUnix(part.updatedAt),
    completedAt: part.completedAt == null
        ? null
        : _dateFromUnix(part.completedAt!),
    error: part.error,
    textChannel: _textChannel(part.textChannel),
    tool: _toolPartFromFrb(part.tool),
    agent: _agentPartFromFrb(part.agent),
    planContent: part.plan?.content,
    synthetic: part.synthetic,
    ignored: part.ignored,
  );
}

String _frbPartText(frb.BridgeStudioPartDto part) {
  if (part.text.isNotEmpty) {
    return part.text;
  }
  return switch (_partType(part.partType)) {
    TimelinePartType.tool => [
      part.tool?.arguments,
      part.tool?.result,
    ].whereType<String>().where((value) => value.isNotEmpty).join('\n'),
    TimelinePartType.plan => part.plan?.content ?? '',
    TimelinePartType.agent => part.agent?.summary ?? part.agent?.task ?? '',
    TimelinePartType.reasoning ||
    TimelinePartType.text ||
    TimelinePartType.turn ||
    TimelinePartType.inference => '',
  };
}

TimelineToolPart? _toolPartFromFrb(frb.BridgeStudioToolPartDto? tool) {
  if (tool == null) {
    return null;
  }
  return TimelineToolPart(
    toolCallId: tool.toolCallId,
    callId: tool.callId,
    providerItemId: tool.providerItemId,
    name: tool.name.isEmpty ? 'tool' : tool.name,
    arguments: tool.arguments,
    result: tool.result,
    exitCode: tool.exitCode,
    timedOut: tool.timedOut,
    workingDirectory: tool.workingDirectory,
    denialReason: tool.denialReason,
  );
}

TimelineAgentPart? _agentPartFromFrb(frb.BridgeStudioAgentPartDto? agent) {
  if (agent == null) {
    return null;
  }
  return TimelineAgentPart(
    id: agent.id,
    path: agent.path,
    parentPath: agent.parentPath,
    role: agent.role.isEmpty ? 'agent' : agent.role,
    task: agent.task,
    status: agent.status,
    summary: agent.summary,
    depth: agent.depth,
    error: agent.error,
    reason: agent.reason,
  );
}

TimelinePartDelta _timelinePartDeltaFromFrb(
  frb.BridgeStudioPartDeltaDto delta,
) {
  return TimelinePartDelta(
    sessionId: delta.sessionId,
    messageId: delta.messageId,
    partId: delta.partId,
    revision: delta.revision.toInt(),
    field: _timelineDeltaField(delta.field),
    delta: delta.delta,
    chunkIndex: delta.chunkIndex,
  );
}

InteractionChangedPayload _interactionChangedPayloadFromFrb(
  frb.BridgeInteractionChangedDto event,
) {
  final payload = _interactionPayloadFromFrb(event.payload);
  final kind = _interactionKind(event.kind);
  final interaction = PendingInteraction(
    id: event.interactionId,
    sessionId: event.sessionId,
    kind: kind,
    title: _interactionTitle(kind, payload),
    body: _interactionBody(kind, payload),
    payload: payload,
  );
  return InteractionChangedPayload(
    interaction: interaction,
    status: event.status,
  );
}

TimelineAgentEvent _timelineAgentEventFromFrb(
  frb.BridgeAgentTimelineEventDto event,
) {
  final payload = _agentTimelinePayloadFromFrb(event.payload);
  return timelineAgentEventFromPayload(
    payload,
    eventId: event.eventId,
    sessionId: event.sessionId,
    sequence: event.sequence.toInt(),
    createdAt: _dateFromUnix(event.createdAt),
    kindType: _string(payload['type']),
  );
}

Map<String, Object?> _agentTimelinePayloadFromFrb(
  frb.BridgeAgentTimelinePayloadDto payload,
) {
  return switch (payload) {
    frb.BridgeAgentTimelinePayloadDto_SpawnBegin(
      :final callId,
      :final senderPath,
      :final taskName,
      :final prompt,
      :final role,
      :final model,
      :final reasoningEffort,
    ) =>
      {
        'type': 'spawnBegin',
        'callId': callId,
        'senderPath': senderPath,
        'taskName': taskName,
        'prompt': prompt,
        'role': role,
        'model': ?model,
        'reasoningEffort': ?reasoningEffort,
      },
    frb.BridgeAgentTimelinePayloadDto_SpawnEnd(
      :final callId,
      :final senderPath,
      :final agentId,
      :final path,
      :final role,
      :final status,
      :final prompt,
      :final error,
    ) =>
      {
        'type': 'spawnEnd',
        'callId': callId,
        'senderPath': senderPath,
        'agentId': ?agentId,
        'path': ?path,
        'role': ?role,
        'status': status,
        'prompt': prompt,
        'error': ?error,
      },
    frb.BridgeAgentTimelinePayloadDto_InteractionBegin(
      :final callId,
      :final senderPath,
      :final receiverPath,
      :final prompt,
    ) =>
      {
        'type': 'interactionBegin',
        'callId': callId,
        'senderPath': senderPath,
        'receiverPath': receiverPath,
        'prompt': prompt,
      },
    frb.BridgeAgentTimelinePayloadDto_InteractionEnd(
      :final callId,
      :final senderPath,
      :final receiverPath,
      :final status,
      :final prompt,
      :final error,
    ) =>
      {
        'type': 'interactionEnd',
        'callId': callId,
        'senderPath': senderPath,
        'receiverPath': receiverPath,
        'status': status,
        'prompt': prompt,
        'error': ?error,
      },
    frb.BridgeAgentTimelinePayloadDto_WaitingBegin(
      :final callId,
      :final senderPath,
    ) =>
      {'type': 'waitingBegin', 'callId': callId, 'senderPath': senderPath},
    frb.BridgeAgentTimelinePayloadDto_WaitingEnd(
      :final callId,
      :final senderPath,
      :final timedOut,
    ) =>
      {
        'type': 'waitingEnd',
        'callId': callId,
        'senderPath': senderPath,
        'timedOut': timedOut,
      },
    frb.BridgeAgentTimelinePayloadDto_CloseBegin(
      :final callId,
      :final senderPath,
      :final receiverPath,
    ) =>
      {
        'type': 'closeBegin',
        'callId': callId,
        'senderPath': senderPath,
        'receiverPath': receiverPath,
      },
    frb.BridgeAgentTimelinePayloadDto_CloseEnd(
      :final callId,
      :final senderPath,
      :final receiverPath,
      :final status,
      :final error,
    ) =>
      {
        'type': 'closeEnd',
        'callId': callId,
        'senderPath': senderPath,
        'receiverPath': receiverPath,
        'status': status,
        'error': ?error,
      },
    frb.BridgeAgentTimelinePayloadDto_Unknown(:final kindType) => {
      'type': kindType,
    },
  };
}

SessionRuntimeView _sessionRuntimeFromFrb(frb.BridgeSessionRuntimeDto runtime) {
  return _sessionRuntimeFromFrbWithAgents(runtime, agentCount: 0);
}

SessionRuntimeView _sessionRuntimeFromFrbWithAgents(
  frb.BridgeSessionRuntimeDto runtime, {
  required int agentCount,
}) {
  return SessionRuntimeView(
    model: runtime.model,
    contextTokens: runtime.latestContextTokens.toInt(),
    contextWindow: runtime.contextWindow?.toInt() ?? 0,
    totalTokens: runtime.totalTokens.toInt(),
    costLabel: _costLabel(
      runtime.estimatedCosts
          .map((cost) => {'currency': cost.currency, 'amount': cost.amount})
          .toList(),
      runtime.hasUnpricedUsage,
    ),
    activeSkills: runtime.activeSkills,
    activeMcpServers: runtime.activeMcpServers,
    activeLspServers: runtime.activeLspServers,
    agentCount: agentCount,
  );
}

StudioProject _projectFromFrb(frb.ProjectDto project) {
  return StudioProject(id: project.id, name: project.name, path: project.path);
}

StudioSession _sessionFromFrb(frb.SessionDto session) {
  return StudioSession(
    id: session.id,
    projectId: session.projectId,
    title: session.title,
    mode: _compileMode(session.mode),
    updatedAt: _dateFromUnix(session.updatedAt),
  );
}

McpServerSettingsView _mcpServerFromFrb(frb.BridgeMcpServerDto server) {
  return McpServerSettingsView(
    id: server.id,
    transport: server.transport,
    endpoint: server.endpoint.isNotEmpty
        ? server.endpoint
        : (server.url ?? server.command ?? ''),
    enabled: server.enabled,
    status: server.statusKind.isEmpty
        ? server.availabilityKind
        : server.statusKind,
  );
}

String? _emptyToNull(String value) {
  return value.isEmpty ? null : value;
}

StudioState studioStateFromBootstrapJson(Map<String, Object?> json) {
  final selectedSessionId = _nullableString(json['selectedSessionId']);
  return _stateFromJson(
    json,
    selectedProjectId: _nullableString(json['selectedProjectId']),
    selectedSessionId: selectedSessionId,
  );
}

StudioState studioStateFromSessionJson(Map<String, Object?> json) {
  final session = _map(json['session']);
  final sessionId = _string(json['sessionId']).isNotEmpty
      ? _string(json['sessionId'])
      : _string(session['id']);
  return _stateFromJson(
    json,
    selectedProjectId: _nullableString(session['projectId']),
    selectedSessionId: sessionId.isEmpty ? null : sessionId,
  );
}

PendingInteraction _pendingInteractionFromFrb(
  frb.BridgeInteractionChangedDto event,
) {
  final payload = _interactionPayloadFromFrb(event.payload);
  final kind = _interactionKind(event.kind);
  return PendingInteraction(
    id: event.interactionId,
    sessionId: event.sessionId,
    kind: kind,
    title: _interactionTitle(kind, payload),
    body: _interactionBody(kind, payload),
    payload: payload,
  );
}

Map<String, Object?> _interactionPayloadFromFrb(
  frb.BridgeInteractionPayloadDto payload,
) {
  return switch (payload) {
    frb.BridgeInteractionPayloadDto_UserInput(:final questions) => {
      'type': 'userInput',
      'questions': questions
          .map(
            (question) => {
              'id': question.id,
              'header': question.header,
              'question': question.question,
              'prompt': question.question,
              'isOther': question.isOther,
              'isSecret': question.isSecret,
              if (question.options != null)
                'options': question.options!
                    .map(
                      (option) => {
                        'label': option.label,
                        'description': option.description,
                      },
                    )
                    .toList(),
            },
          )
          .toList(),
    },
    frb.BridgeInteractionPayloadDto_ToolApproval(
      :final name,
      :final argumentsJson,
      :final workingDirectory,
      :final parentAgentId,
    ) =>
      {
        'type': 'toolApproval',
        'name': name,
        'arguments': _tryDecodeJsonValue(argumentsJson),
        'workingDirectory': ?workingDirectory,
        'parentAgentId': ?parentAgentId,
      },
    frb.BridgeInteractionPayloadDto_PlanConfirmation(
      :final planId,
      :final content,
    ) =>
      {'type': 'planConfirmation', 'planId': planId, 'content': content},
  };
}

Map<String, Map<String, TimelineAgentEvent>> _agentTimelineEventsFromTyped(
  Iterable<TimelineAgentEvent> events,
) {
  final bySession = <String, Map<String, TimelineAgentEvent>>{};
  for (final event in events) {
    if (event.eventId.isEmpty || event.sessionId.isEmpty) {
      continue;
    }
    bySession.putIfAbsent(event.sessionId, () => {})[event.eventId] = event;
  }
  return bySession;
}

StudioState studioStateFromFrbSnapshot(frb.BridgeStudioSnapshotResponse value) {
  return _stateFromTypedSnapshot(
    projects: value.projects.map(_projectFromFrb).toList(),
    sessions: value.sessions.map(_sessionFromFrb).toList(),
    selectedProjectId: value.selectedProjectId,
    selectedSessionId: value.selectedSessionId,
    messages: const [],
    parts: const [],
    agentEvents: value.agentEvents.map(_timelineAgentEventFromFrb).toList(),
    agents: value.agents,
    interactions: value.interactions
        .map(_pendingInteractionFromFrb)
        .where((interaction) => interaction.id.isNotEmpty)
        .toList(),
    runtime: value.sessionRuntime == null
        ? _emptyRuntimeView()
        : _sessionRuntimeFromFrbWithAgents(
            value.sessionRuntime!,
            agentCount: value.agents.length,
          ),
    config: _decodeJson(value.configJson),
    generalSettings: _decodeJson(value.generalSettingsJson),
    eventNextSequence: 0,
  );
}

StudioState studioStateFromFrbSession(frb.BridgeSessionStateResponse value) {
  return _stateFromTypedSnapshot(
    projects: const [],
    sessions: value.sessions.map(_sessionFromFrb).toList(),
    selectedProjectId: value.session.projectId,
    selectedSessionId: value.sessionId.isEmpty
        ? value.session.id
        : value.sessionId,
    messages: value.messages
        .map(
          (item) => _timelineMessageFromFrb(
            item.message,
            sequence: item.sequence.toInt(),
          ),
        )
        .toList(),
    parts: value.parts
        .where((item) => !_isIgnoredTimelinePartType(item.part_.partType))
        .map(
          (item) => _timelinePartSnapshotFromFrb(
            item.part_,
            sequence: item.sequence.toInt(),
          ),
        )
        .toList(),
    events: value.events.map(StudioBridgeEvent.fromFrb).toList(),
    agentEvents: value.agentEvents.map(_timelineAgentEventFromFrb).toList(),
    agents: value.agents,
    interactions: value.interactions
        .map(_pendingInteractionFromFrb)
        .where((interaction) => interaction.id.isNotEmpty)
        .toList(),
    runtime: value.sessionRuntime == null
        ? _emptyRuntimeView()
        : _sessionRuntimeFromFrbWithAgents(
            value.sessionRuntime!,
            agentCount: value.agents.length,
          ),
    config: const {},
    generalSettings: const {},
    eventNextSequence: value.eventNextSequence.toInt(),
  );
}

List<StudioSession> studioSessionsFromJson(Object? value) {
  return _list(value).map(_sessionFromJson).toList();
}

TimelineMessage timelineMessageFromJson(Object? value, {int sequence = 0}) {
  final json = _map(value);
  return TimelineMessage(
    id: _string(json['messageId'], fallback: _string(json['id'])),
    sessionId: _string(json['sessionId']),
    role: _string(json['role'], fallback: 'assistant'),
    createdAt: _dateFromUnix(_int(json['createdAt'])),
    sequence: sequence,
  );
}

TimelinePartSnapshot timelinePartSnapshotFromJson(
  Object? value, {
  int sequence = 0,
}) {
  final json = _map(value);
  final type = _partType(
    _string(json['partType'], fallback: _string(json['type'])),
  );
  return TimelinePartSnapshot(
    id: _string(json['partId'], fallback: _string(json['id'])),
    messageId: _string(json['messageId']),
    sessionId: _string(json['sessionId']),
    turnId: _string(json['turnId']),
    type: type,
    order: _int(json['order']),
    revision: _int(json['revision']),
    sequence: sequence,
    text: _partText(json, type),
    status: _string(json['status'], fallback: 'completed'),
    createdAt: _dateFromUnix(_int(json['createdAt'])),
    updatedAt: _dateFromUnix(_int(json['updatedAt'])),
    completedAt: _nullableInt(json['completedAt']) == null
        ? null
        : _dateFromUnix(_nullableInt(json['completedAt'])!),
    error: _nullableString(json['error']),
    textChannel: _textChannel(json['textChannel']),
    tool: _toolPart(json['tool']),
    agent: _agentPart(json['agent']),
    planContent: _string(_map(json['plan'])['content']),
    synthetic: _bool(json['synthetic']),
    ignored: _bool(json['ignored']),
  );
}

TimelinePartDelta timelinePartDeltaFromJson(Object? value) {
  final json = _map(value);
  return TimelinePartDelta(
    sessionId: _string(json['sessionId']),
    messageId: _string(json['messageId']),
    partId: _string(json['partId']),
    revision: _int(json['revision']),
    field: _timelineDeltaField(json['field']),
    delta: _string(json['delta']),
    chunkIndex: _nullableInt(json['chunkIndex']),
  );
}

String _timelineDeltaField(Object? value) {
  final field = _string(value);
  return switch (field) {
    'text' ||
    'reasoning.summary' ||
    'planContent' ||
    'tool.arguments' ||
    'tool.result' => field,
    _ => throw FormatException('Unknown timeline delta field: $field'),
  };
}

SessionRuntimeView sessionRuntimeFromJson(Object? value) {
  final json = _map(value);
  final usage = _map(json['usage']);
  final source = usage.isEmpty ? json : usage;
  final agentCount = _list(json['agents']).length;
  return SessionRuntimeView(
    model: _string(source['model']),
    contextTokens: _int(source['latestContextTokens']),
    contextWindow: _int(source['contextWindow']),
    totalTokens: _int(source['totalTokens']),
    costLabel: _costLabel(
      source['estimatedCosts'],
      _bool(source['hasUnpricedUsage']),
    ),
    activeSkills: _stringList(json['activeSkills']),
    activeMcpServers: _stringList(json['activeMcpServers']),
    activeLspServers: _stringList(json['activeLspServers']),
    agentCount: agentCount,
  );
}

SessionRuntimeView _emptyRuntimeView() {
  return const SessionRuntimeView(
    model: '',
    contextTokens: 0,
    contextWindow: 0,
    totalTokens: 0,
    costLabel: '',
    activeSkills: [],
    activeMcpServers: [],
    activeLspServers: [],
    agentCount: 0,
  );
}

PendingInteraction pendingInteractionFromJson(Object? value) {
  final json = _map(value);
  final scope = _map(json['scope']);
  final payload = _map(json['payload']);
  final kind = _interactionKind(
    _string(json['kind'], fallback: _string(payload['type'])),
  );
  return PendingInteraction(
    id: _string(json['interactionId'], fallback: _string(json['id'])),
    sessionId: _string(
      scope['sessionId'],
      fallback: _string(json['sessionId']),
    ),
    kind: kind,
    title: _interactionTitle(kind, payload),
    body: _interactionBody(kind, payload),
    payload: payload,
  );
}

ProviderUsageView providerUsageFromJson(Object? value) {
  final json = _map(value);
  final balance = _map(json['balance']);
  final codingPlan = _map(json['codingPlan']);
  return ProviderUsageView(
    providerId: _string(json['providerId']),
    updatedAt: _int(json['updatedAt']),
    status: _string(json['status'], fallback: 'unknown'),
    usageKind: _string(json['usageKind'], fallback: 'unknown'),
    message: _nullableString(json['message']),
    balance: balance.isEmpty
        ? null
        : DeepSeekBalanceUsageView(
            isAvailable: _bool(balance['isAvailable']),
            balances: _list(balance['balances'])
                .map((item) {
                  final value = _map(item);
                  return DeepSeekBalanceInfoView(
                    currency: _string(value['currency']),
                    totalBalance: _string(value['totalBalance']),
                    grantedBalance: _string(value['grantedBalance']),
                    toppedUpBalance: _string(value['toppedUpBalance']),
                  );
                })
                .where((item) => item.currency.isNotEmpty)
                .toList(),
          ),
    codingPlan: codingPlan.isEmpty
        ? null
        : ZhipuCodingPlanUsageView(
            level: _nullableString(codingPlan['level']),
            limits: _list(codingPlan['limits']).map((item) {
              final value = _map(item);
              return ZhipuQuotaLimitView(
                window: _string(value['window'], fallback: 'other'),
                label: _string(value['label']),
                percentage: _double(value['percentage']),
                currentValue: _nullableDouble(value['currentValue']),
                total: _nullableDouble(value['total']),
                remaining: _nullableDouble(value['remaining']),
                nextResetAt: _nullableInt(value['nextResetAt']),
                usageDetails: _list(value['usageDetails']).map((detailValue) {
                  final detail = _map(detailValue);
                  return ZhipuToolUsageDetailView(
                    name: _string(detail['name']),
                    currentValue: _nullableDouble(detail['currentValue']),
                    total: _nullableDouble(detail['total']),
                    percentage: _nullableDouble(detail['percentage']),
                  );
                }).toList(),
              );
            }).toList(),
          ),
  );
}

ProviderUsageView _providerUsageFromFrb(frb.ProviderUsageDto usage) {
  return ProviderUsageView(
    providerId: usage.providerId,
    updatedAt: usage.updatedAt,
    status: usage.status.isEmpty ? 'unknown' : usage.status,
    usageKind: usage.usageKind.isEmpty ? 'unknown' : usage.usageKind,
    message: usage.message,
    balance: usage.balance == null
        ? null
        : DeepSeekBalanceUsageView(
            isAvailable: usage.balance!.isAvailable,
            balances: usage.balance!.balances
                .map(
                  (item) => DeepSeekBalanceInfoView(
                    currency: item.currency,
                    totalBalance: item.totalBalance,
                    grantedBalance: item.grantedBalance,
                    toppedUpBalance: item.toppedUpBalance,
                  ),
                )
                .where((item) => item.currency.isNotEmpty)
                .toList(),
          ),
    codingPlan: usage.codingPlan == null
        ? null
        : ZhipuCodingPlanUsageView(
            level: usage.codingPlan!.level,
            limits: usage.codingPlan!.limits
                .map(
                  (item) => ZhipuQuotaLimitView(
                    window: item.window.isEmpty ? 'other' : item.window,
                    label: item.label,
                    percentage: item.percentage,
                    currentValue: item.currentValue,
                    total: item.total,
                    remaining: item.remaining,
                    nextResetAt: item.nextResetAt,
                    usageDetails: item.usageDetails
                        .map(
                          (detail) => ZhipuToolUsageDetailView(
                            name: detail.name,
                            currentValue: detail.currentValue,
                            total: detail.total,
                            percentage: detail.percentage,
                          ),
                        )
                        .toList(),
                  ),
                )
                .toList(),
          ),
  );
}

StudioState _stateFromJson(
  Map<String, Object?> json, {
  required String? selectedProjectId,
  required String? selectedSessionId,
}) {
  final timeline = _timelineFromJson(
    _list(json['messages']),
    _list(json['parts']),
  );
  for (final session in studioSessionsFromJson(json['sessions'])) {
    timeline.messagesBySession.putIfAbsent(session.id, () => []);
  }
  final config = _map(json['config']);
  final runtimeJson = Map<String, Object?>.from(_map(json['sessionRuntime']));
  if (!runtimeJson.containsKey('agents')) {
    runtimeJson['agents'] = _list(json['agents']);
  }
  final eventNextSequence = _int(
    _firstValue(json, const ['eventNextSequence', 'event_next_sequence']),
  );
  return _stateFromTypedSnapshot(
    projects: _list(json['projects']).map(_projectFromJson).toList(),
    sessions: studioSessionsFromJson(json['sessions']),
    selectedProjectId: selectedProjectId,
    selectedSessionId: selectedSessionId,
    messages: timeline.messagesBySession.values
        .expand((messages) => messages)
        .toList(),
    parts: timeline.partSnapshotsBySession.values
        .expand((parts) => parts.values)
        .toList(),
    agentTimelineEventsBySession: _agentTimelineEventsFromJson(
      json['agentEvents'],
    ),
    interactions: _list(json['interactions'])
        .map(pendingInteractionFromJson)
        .where((interaction) => interaction.id.isNotEmpty)
        .toList(),
    runtime: sessionRuntimeFromJson(runtimeJson),
    config: config,
    generalSettings: _map(json['generalSettings']),
    eventNextSequence: eventNextSequence,
    agents: const [],
  );
}

StudioState _stateFromTypedSnapshot({
  required List<StudioProject> projects,
  required List<StudioSession> sessions,
  required String? selectedProjectId,
  required String? selectedSessionId,
  required List<TimelineMessage> messages,
  required List<TimelinePartSnapshot> parts,
  Iterable<StudioBridgeEvent> events = const [],
  Iterable<TimelineAgentEvent> agentEvents = const [],
  Map<String, Map<String, TimelineAgentEvent>>? agentTimelineEventsBySession,
  required Iterable<frb.BridgeAgentSnapshotDto> agents,
  required List<PendingInteraction> interactions,
  required SessionRuntimeView runtime,
  required Map<String, Object?> config,
  required Map<String, Object?> generalSettings,
  required int eventNextSequence,
}) {
  final messagesBySession = <String, List<TimelineMessage>>{};
  for (final message in messages) {
    if (message.id.isEmpty || message.sessionId.isEmpty) {
      continue;
    }
    messagesBySession.putIfAbsent(message.sessionId, () => []).add(message);
  }
  for (final messages in messagesBySession.values) {
    messages.sort((a, b) => a.createdAt.compareTo(b.createdAt));
  }
  final partSnapshotsBySession = <String, Map<String, TimelinePartSnapshot>>{};
  for (final part in parts) {
    if (part.id.isEmpty || part.messageId.isEmpty || part.sessionId.isEmpty) {
      continue;
    }
    partSnapshotsBySession.putIfAbsent(part.sessionId, () => {})[part.id] =
        part;
  }
  for (final session in sessions) {
    messagesBySession.putIfAbsent(session.id, () => []);
  }
  final agentEventsBySession =
      agentTimelineEventsBySession ??
      _agentTimelineEventsFromTyped(agentEvents);
  final latest = events.fold<StudioState>(
    StudioState(
      projects: projects,
      sessions: sessions,
      messagesBySession: messagesBySession,
      partSnapshotsBySession: partSnapshotsBySession,
      partOverlaysBySession: const {},
      agentTimelineEventsBySession: agentEventsBySession,
      providers: _providersFromConfig(config),
      defaultProviderId: _defaultProviderIdFromConfig(config),
      roles: _rolesFromConfig(config),
      mcpServers: _mcpServersFromConfig(config),
      instructions: _instructionsFromConfig(config),
      skills: _skillsFromConfig(config),
      general: _generalFromJson(generalSettings),
      selectedProjectId: selectedProjectId,
      selectedSessionId: selectedSessionId,
      permissionMode: _permissionMode(
        _firstValue(_map(config['runtime']), const [
          'permissionMode',
          'permission_mode',
        ]),
      ),
      turnPhase: TurnPhase.idle,
      runtime: runtime,
      pendingInteractions: interactions,
      eventCursorsBySession: selectedSessionId == null || eventNextSequence <= 0
          ? const {}
          : {selectedSessionId: eventNextSequence - 1},
    ),
    _applySnapshotEvent,
  );
  return latest.copyWith(
    runtime: latest.runtime.copyWith(
      agentCount: agents
          .where((agent) => agent.sessionId == selectedSessionId)
          .length,
    ),
  );
}

StudioState _applySnapshotEvent(StudioState state, StudioBridgeEvent event) {
  return switch (event.payload) {
    TurnChangedPayload(:final turn) => state.copyWith(
      turnPhase: _turnPhaseFromStatus(turn.status),
    ),
    InteractionChangedPayload(:final interaction, :final status) =>
      _withInteraction(state, interaction, status),
    SessionRuntimeChangedPayload(:final runtime) => state.copyWith(
      runtime: runtime.copyWith(agentCount: state.runtime.agentCount),
    ),
    AgentTimelineChangedPayload(:final event) => state.copyWith(
      agentTimelineEventsBySession: {
        ...state.agentTimelineEventsBySession,
        event.sessionId: {
          ...(state.agentTimelineEventsBySession[event.sessionId] ?? const {}),
          event.eventId: event,
        },
      },
    ),
    McpHealthChangedPayload(:final activeMcpServers, :final servers) =>
      state.copyWith(
        mcpServers: servers,
        runtime: state.runtime.copyWith(activeMcpServers: activeMcpServers),
      ),
    LspHealthChangedPayload(:final activeLspServers) => state.copyWith(
      runtime: state.runtime.copyWith(activeLspServers: activeLspServers),
    ),
    SessionListChangedPayload(:final projectId, :final sessions)
        when projectId == null || projectId == state.selectedProjectId =>
      state.copyWith(sessions: sessions),
    _ => state,
  };
}

StudioState _withInteraction(
  StudioState state,
  PendingInteraction interaction,
  String status,
) {
  final interactions = [...state.pendingInteractions];
  final index = interactions.indexWhere((item) => item.id == interaction.id);
  if (status == 'pending') {
    if (index >= 0) {
      interactions[index] = interaction;
    } else {
      interactions.add(interaction);
    }
  } else if (index >= 0) {
    interactions.removeAt(index);
  }
  return StudioState(
    projects: state.projects,
    sessions: state.sessions,
    messagesBySession: state.messagesBySession,
    partSnapshotsBySession: state.partSnapshotsBySession,
    partOverlaysBySession: state.partOverlaysBySession,
    agentTimelineEventsBySession: state.agentTimelineEventsBySession,
    providers: state.providers,
    defaultProviderId: state.defaultProviderId,
    providerUsages: state.providerUsages,
    roles: state.roles,
    mcpServers: state.mcpServers,
    instructions: state.instructions,
    skills: state.skills,
    general: state.general,
    selectedProjectId: state.selectedProjectId,
    selectedSessionId: state.selectedSessionId,
    permissionMode: state.permissionMode,
    turnPhase: state.turnPhase,
    runtime: state.runtime,
    pendingInteractions: interactions,
    eventCursorsBySession: state.eventCursorsBySession,
    composerText: state.composerText,
  );
}

Map<String, Map<String, TimelineAgentEvent>> _agentTimelineEventsFromJson(
  Object? value,
) {
  final bySession = <String, Map<String, TimelineAgentEvent>>{};
  for (final item in _list(value)) {
    final json = _map(item);
    final event = timelineAgentEventFromPayload(
      json,
      eventId: _string(json['eventId']),
      sessionId: _string(json['sessionId']),
      sequence: _int(json['sequence']),
      createdAt: _dateFromUnix(_int(json['createdAt'])),
      kindType: _string(json['kindType']),
    );
    if (event.eventId.isEmpty || event.sessionId.isEmpty) {
      continue;
    }
    bySession.putIfAbsent(event.sessionId, () => {})[event.eventId] = event;
  }
  return bySession;
}

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

class _TimelineLoadResult {
  const _TimelineLoadResult({
    required this.messagesBySession,
    required this.partSnapshotsBySession,
  });

  final Map<String, List<TimelineMessage>> messagesBySession;
  final Map<String, Map<String, TimelinePartSnapshot>> partSnapshotsBySession;
}

_TimelineLoadResult _timelineFromJson(
  List<Object?> messageValues,
  List<Object?> partValues,
) {
  final snapshotsBySession = <String, Map<String, TimelinePartSnapshot>>{};
  for (final value in partValues) {
    final wrapper = _map(value);
    final nested = _map(wrapper['part']);
    final partJson = nested.isEmpty ? _map(value) : nested;
    final part = timelinePartSnapshotFromJson(
      partJson,
      sequence: _int(wrapper['sequence']),
    );
    if (part.id.isEmpty ||
        part.messageId.isEmpty ||
        part.sessionId.isEmpty ||
        part.ignored ||
        isInternalTimelinePartType(part.type)) {
      continue;
    }
    snapshotsBySession.putIfAbsent(part.sessionId, () => {})[part.id] = part;
  }

  final bySession = <String, List<TimelineMessage>>{};
  for (final value in messageValues) {
    final wrapper = _map(value);
    final nested = _map(wrapper['message']);
    final messageJson = nested.isEmpty ? _map(value) : nested;
    final message = timelineMessageFromJson(
      messageJson,
      sequence: _int(wrapper['sequence']),
    );
    if (message.id.isEmpty || message.sessionId.isEmpty) {
      continue;
    }
    bySession.putIfAbsent(message.sessionId, () => []).add(message);
  }
  for (final messages in bySession.values) {
    messages.sort((a, b) => a.createdAt.compareTo(b.createdAt));
  }
  return _TimelineLoadResult(
    messagesBySession: bySession,
    partSnapshotsBySession: snapshotsBySession,
  );
}

StudioProject _projectFromJson(Object? value) {
  final json = _map(value);
  return StudioProject(
    id: _string(json['id']),
    name: _string(json['name']),
    path: _string(json['path']),
  );
}

StudioSession _sessionFromJson(Object? value) {
  final json = _map(value);
  return StudioSession(
    id: _string(json['id']),
    projectId: _string(json['projectId']),
    title: _string(json['title'], fallback: 'Untitled'),
    mode: _compileMode(json['mode']),
    updatedAt: _dateFromUnix(_int(json['updatedAt'])),
  );
}

List<ProviderSettingsView> _providersFromConfig(Map<String, Object?> config) {
  final providers = _map(config['providers']);
  return providers.entries.map((entry) {
    final value = _map(entry.value);
    final templateKind = _providerTemplateKind(entry.key, value);
    final defaultSlugs = _templateDefaultModelSlugs(templateKind);
    final providerModels = _providerModels(value['models']);
    final defaultModels = providerModels
        .where((model) => defaultSlugs.contains(model.slug))
        .toList();
    final customModels = providerModels
        .where((model) => !defaultSlugs.contains(model.slug))
        .toList();
    final visibleModels = defaultModels.isEmpty
        ? providerModels
        : [...defaultModels, ...customModels];
    final defaultModel = _string(
      _firstValue(value, const ['defaultModel', 'default_model']),
    );
    final bearerToken = _string(
      _firstValue(value, const ['bearerToken', 'bearer_token']),
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
      hasBearerToken: bearerToken.trim().isNotEmpty,
      defaultModel: defaultModel,
      models: visibleModels,
      defaultModels: defaultModels.isEmpty ? providerModels : defaultModels,
      customModels: customModels,
      status: bearerToken.trim().isEmpty ? 'missingCredential' : 'ready',
      usageLabel: visibleModels.isEmpty
          ? defaultModel
          : '${visibleModels.length} models',
      modelCount: '${visibleModels.length}',
      updatedAt: 'Loaded',
      providerKind: _providerKindName(_string(value['provider_kind'])),
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

String _providerTemplateKind(String providerId, Map<String, Object?> provider) {
  final direct = _string(
    _firstValue(provider, const ['templateKind', 'template_kind']),
  );
  if (direct.isNotEmpty) {
    return direct;
  }
  final providerKind = _string(provider['provider_kind']);
  final baseUrl = _string(_firstValue(provider, const ['baseUrl', 'base_url']));
  if (providerId == 'zhipu-coding-plan' ||
      baseUrl.contains('/api/coding/paas/')) {
    return 'zhipu-coding-plan';
  }
  return switch (providerKind) {
    'deep_seek' => 'deepseek',
    'zhipu' => 'zhipu',
    _ => 'openai',
  };
}

String _providerKindName(String value) {
  return switch (value) {
    'deep_seek' => 'deep_seek',
    'zhipu' => 'zhipu',
    'open_ai' => 'open_ai',
    _ => value.isEmpty ? 'open_ai' : value,
  };
}

Set<String> _templateDefaultModelSlugs(String templateKind) {
  return switch (templateKind) {
    'deepseek' => {'deepseek-v4-flash', 'deepseek-v4-pro'},
    'zhipu' || 'zhipu-coding-plan' => {
      'glm-5.2',
      'glm-5',
      'glm-5-turbo',
      'glm-4.7',
      'glm-4.7-flashx',
      'glm-4.7-flash',
    },
    _ => {'gpt-5.5', 'gpt-5.4', 'gpt-5.4-mini'},
  };
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
  void addServers(Object? value, {required bool builtin}) {
    for (final entry in _map(value).entries) {
      final server = _map(entry.value);
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
              ? (builtin ? 'builtin' : 'stdio')
              : transport,
          endpoint: url.isEmpty ? command : url,
          enabled: enabled,
          status: enabled ? 'enabled' : 'disabled',
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

String _partText(Map<String, Object?> json, TimelinePartType type) {
  final text = _string(json['text']);
  if (text.isNotEmpty) {
    return text;
  }
  return switch (type) {
    TimelinePartType.tool => [
      _string(_map(json['tool'])['arguments']),
      _string(_map(json['tool'])['result']),
    ].where((part) => part.isNotEmpty).join('\n'),
    TimelinePartType.plan => _string(_map(json['plan'])['content']),
    TimelinePartType.agent => _string(
      _map(json['agent'])['summary'],
      fallback: _string(_map(json['agent'])['task']),
    ),
    TimelinePartType.reasoning ||
    TimelinePartType.text ||
    TimelinePartType.turn ||
    TimelinePartType.inference => '',
  };
}

TimelineTextChannel? _textChannel(Object? value) {
  final label = _string(value);
  if (label.isEmpty) {
    return null;
  }
  return switch (label) {
    'user' => TimelineTextChannel.user,
    'commentary' => TimelineTextChannel.commentary,
    'final' => TimelineTextChannel.finalAnswer,
    _ => throw FormatException('Unknown text channel: $label'),
  };
}

TimelineToolPart? _toolPart(Object? value) {
  final json = _map(value);
  if (json.isEmpty) {
    return null;
  }
  return TimelineToolPart(
    toolCallId: _string(json['toolCallId']),
    callId: _nullableString(json['callId']),
    providerItemId: _nullableString(json['providerItemId']),
    name: _string(json['name'], fallback: 'tool'),
    arguments: _string(json['arguments']),
    result: _nullableString(json['result']),
    exitCode: _nullableInt(json['exitCode']),
    timedOut: _bool(json['timedOut']),
    workingDirectory: _nullableString(json['workingDirectory']),
    denialReason: _nullableString(json['denialReason']),
  );
}

TimelineAgentPart? _agentPart(Object? value) {
  final json = _map(value);
  if (json.isEmpty) {
    return null;
  }
  return TimelineAgentPart(
    id: _string(json['id']),
    path: _string(json['path']),
    parentPath: _nullableString(json['parentPath']),
    role: _string(json['role'], fallback: 'agent'),
    task: _string(json['task']),
    status: _string(json['status']),
    summary: _nullableString(json['summary']),
    depth: _int(json['depth']),
    error: _nullableString(json['error']),
    reason: _nullableString(json['reason']),
  );
}

String _interactionTitle(InteractionKind kind, Map<String, Object?> payload) {
  return switch (kind) {
    InteractionKind.toolApproval => _string(
      payload['name'],
      fallback: 'Tool approval',
    ),
    InteractionKind.userInput => 'User input requested',
    InteractionKind.planConfirmation => 'Plan confirmation',
  };
}

String _interactionBody(InteractionKind kind, Map<String, Object?> payload) {
  return switch (kind) {
    InteractionKind.toolApproval => _jsonText(payload['arguments']),
    InteractionKind.userInput =>
      _list(payload['questions'])
          .map((question) => _string(_map(question)['prompt']))
          .where((prompt) => prompt.isNotEmpty)
          .join('\n'),
    InteractionKind.planConfirmation => _string(payload['content']),
  };
}

TimelinePartType _partType(String value) {
  return switch (value) {
    'text' => TimelinePartType.text,
    'reasoning' => TimelinePartType.reasoning,
    'tool' => TimelinePartType.tool,
    'plan' => TimelinePartType.plan,
    'agent' => TimelinePartType.agent,
    'turn' => TimelinePartType.turn,
    'inference' => TimelinePartType.inference,
    _ => throw FormatException('Unknown timeline part type: $value'),
  };
}

bool _isIgnoredTimelinePartType(String value) {
  return isInternalTimelinePartType(_partType(value));
}

CompileMode _compileMode(Object? value) {
  return _string(value) == 'plan' ? CompileMode.plan : CompileMode.auto;
}

String _compileModeLabel(CompileMode mode) {
  return switch (mode) {
    CompileMode.auto => 'auto',
    CompileMode.plan => 'plan',
  };
}

PermissionMode _permissionMode(Object? value) {
  return switch (_string(value)) {
    'autoReview' || 'auto-review' => PermissionMode.autoReview,
    'fullAccess' || 'full-access' => PermissionMode.fullAccess,
    _ => PermissionMode.requestApproval,
  };
}

TurnPhase _turnPhaseFromStatus(String status) {
  return switch (status) {
    'queued' => TurnPhase.queued,
    'contextLoading' => TurnPhase.contextLoading,
    'waitingForModel' => TurnPhase.waitingForModel,
    'streaming' => TurnPhase.streaming,
    'waitingForInteraction' => TurnPhase.waitingForInteraction,
    'runningTool' => TurnPhase.runningTool,
    'completed' => TurnPhase.completed,
    'failed' => TurnPhase.failed,
    'cancelled' => TurnPhase.cancelled,
    _ => TurnPhase.idle,
  };
}

String _permissionModeLabel(PermissionMode mode) {
  return switch (mode) {
    PermissionMode.requestApproval => 'request-approval',
    PermissionMode.autoReview => 'auto-review',
    PermissionMode.fullAccess => 'full-access',
  };
}

InteractionKind _interactionKind(String value) {
  return switch (value) {
    'userInput' => InteractionKind.userInput,
    'planConfirmation' => InteractionKind.planConfirmation,
    _ => InteractionKind.toolApproval,
  };
}

String _costLabel(Object? value, bool hasUnpricedUsage) {
  final costs = _list(value);
  if (costs.isEmpty) {
    return hasUnpricedUsage ? 'unpriced usage' : '';
  }
  return costs
      .map((cost) {
        final map = _map(cost);
        final amount = _compactAmount(
          _string(map['amount'], fallback: _string(map['value'])),
        );
        final currency = _string(map['currency']);
        return [currency, amount].where((part) => part.isNotEmpty).join(' ');
      })
      .where((label) => label.isNotEmpty)
      .join(', ');
}

String _compactAmount(String value) {
  final parsed = double.tryParse(value);
  if (parsed == null) {
    return value;
  }
  final fixed = parsed.toStringAsFixed(4);
  return fixed.replaceFirst(RegExp(r'\.?0+$'), '');
}

Map<String, Object?> _decodeJson(String json) {
  final value = jsonDecode(json);
  return _map(value);
}

Object? _tryDecodeJsonValue(String json) {
  if (json.trim().isEmpty) {
    return null;
  }
  try {
    return jsonDecode(json);
  } catch (_) {
    return null;
  }
}

Map<String, Object?> _map(Object? value) {
  if (value is Map<String, Object?>) {
    return value;
  }
  if (value is Map) {
    return value.map((key, value) => MapEntry(key.toString(), value));
  }
  return const {};
}

List<Object?> _list(Object? value) {
  if (value is List) {
    return value.cast<Object?>();
  }
  return const [];
}

List<String> _stringList(Object? value) {
  return _list(value).map(_string).where((item) => item.isNotEmpty).toList();
}

Object? _firstValue(Map<String, Object?> json, List<String> keys) {
  for (final key in keys) {
    if (json.containsKey(key)) {
      return json[key];
    }
  }
  return null;
}

String _string(Object? value, {String fallback = ''}) {
  if (value == null) {
    return fallback;
  }
  if (value is String) {
    return value.isEmpty ? fallback : value;
  }
  return value.toString();
}

String? _nullableString(Object? value) {
  final string = _string(value);
  return string.isEmpty ? null : string;
}

int _int(Object? value, {int fallback = 0}) {
  if (value is int) {
    return value;
  }
  if (value is BigInt) {
    return value.toInt();
  }
  return int.tryParse(_string(value)) ?? fallback;
}

int? _nullableInt(Object? value) {
  final string = _string(value);
  if (string.isEmpty) {
    return null;
  }
  return _int(value);
}

double? _nullableDouble(Object? value) {
  if (value is num) {
    return value.toDouble();
  }
  final string = _string(value);
  return string.isEmpty ? null : double.tryParse(string);
}

double _double(Object? value) => _nullableDouble(value) ?? 0;

bool _bool(Object? value) {
  if (value is bool) {
    return value;
  }
  return _string(value) == 'true';
}

bool _boolWithDefault(Object? value, bool fallback) {
  if (value == null) {
    return fallback;
  }
  return _bool(value);
}

DateTime _dateFromUnix(int seconds) {
  return DateTime.fromMillisecondsSinceEpoch(seconds * 1000);
}

String _jsonText(Object? value) {
  if (value == null) {
    return '';
  }
  if (value is String) {
    return value;
  }
  return const JsonEncoder.withIndent('  ').convert(value);
}

class DemoStudioApi implements StudioApi {
  List<ProviderSettingsView>? _providers;
  List<RoleSettingsView>? _roles;
  InstructionsSettingsView _instructions = const InstructionsSettingsView();
  SkillsSettingsView _skills = const SkillsSettingsView();
  GeneralSettingsView _general = const GeneralSettingsView();
  PermissionMode _permissionMode = PermissionMode.requestApproval;
  final Set<String> _archivedProjectIds = <String>{};
  final Map<String, Map<String, Object?>> _settingsDrafts = {};
  final _globalEvents = StreamController<Object>.broadcast();
  final _sessionEvents = StreamController<Object>.broadcast();
  int _eventSequence = 0;

  @override
  Future<StudioState> bootstrap() async {
    final now = DateTime.now();
    const project = StudioProject(
      id: 'project-local',
      name: 'pure-lang',
      path: r'C:\Users\zhoudongsheng\.codex\worktrees\3bc1\pure-lang',
    );
    final session = StudioSession(
      id: 'session-main',
      projectId: project.id,
      title: 'Flutter + FRB 重构',
      mode: CompileMode.auto,
      updatedAt: now,
    );
    final userCreatedAt = now.subtract(const Duration(minutes: 9));
    final assistantCreatedAt = now.subtract(const Duration(minutes: 8));
    final demoParts = [
      TimelinePartSnapshot(
        id: 'turn-demo:user-text',
        messageId: 'turn-demo:user',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.text,
        order: 0,
        revision: 0,
        text:
            '用 Flutter 重构 Pure Studio。\n\n'
            '- timeline 要像 Web 版一样即时渲染 Markdown\n'
            '- streaming 中的代码块和表格不要抖动',
        status: 'completed',
        createdAt: userCreatedAt,
        updatedAt: userCreatedAt,
        textChannel: TimelineTextChannel.user,
      ),
      TimelinePartSnapshot(
        id: 'turn-demo:reasoning-1',
        messageId: 'turn-demo:assistant',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.reasoning,
        order: 0,
        revision: 0,
        text:
            '## 判断\n\n'
            '> UI 只消费当前会话的高频事件，后台会话不应该继续推 delta。\n\n'
            '- `messagePartDelta` 只作为 live overlay\n'
            '- terminal snapshot 到达后覆盖未完成文本',
        status: 'completed',
        createdAt: assistantCreatedAt,
        updatedAt: assistantCreatedAt,
      ),
      TimelinePartSnapshot(
        id: 'turn-demo:tool-1',
        messageId: 'turn-demo:assistant',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.tool,
        order: 1,
        revision: 0,
        text: '',
        status: 'completed',
        createdAt: assistantCreatedAt,
        updatedAt: assistantCreatedAt,
        tool: const TimelineToolPart(
          toolCallId: 'turn-demo:tool-call-1',
          name: 'cargo test -p pl-studio-bridge',
          result: '1 passed; bridge envelope uses typed payload.',
        ),
      ),
      TimelinePartSnapshot(
        id: 'turn-demo:plan-1',
        messageId: 'turn-demo:assistant',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.plan,
        order: 2,
        revision: 0,
        text: '',
        status: 'completed',
        createdAt: assistantCreatedAt,
        updatedAt: assistantCreatedAt,
        planContent:
            '## Implementation checklist\n\n'
            '1. Keep the Flutter shell aligned with runtime contracts.\n'
            '2. Use Riverpod selectors for derived views.\n'
            '3. Subscribe only the selected session stream.\n'
            '4. Verify Markdown in streaming mode.\n\n'
            '| Area | Status |\n'
            '| --- | --- |\n'
            '| FRB runtime | ready |\n'
            '| Timeline Markdown | streaming |\n\n'
            '```text\n'
            'WeatherDay>```\n\n'
            '## Inline fence recovery\n\n'
            '| Renderer | Result |\n'
            '| --- | --- |\n'
            '| Timeline | headings and tables stay live |',
      ),
      TimelinePartSnapshot(
        id: 'turn-demo:final-1',
        messageId: 'turn-demo:assistant',
        sessionId: session.id,
        turnId: 'turn-demo',
        type: TimelinePartType.text,
        order: 3,
        revision: 0,
        text:
            '### Streaming Markdown preview\n\n'
            '正文、**加粗**、`inline code` 和链接都应该按 GFM 渲染。\n\n'
            '- text / plan / reasoning 走同一个 renderer\n'
            '- fenced code block 即使还没收到结束 fence，也应该显示成代码块\n\n'
            '```dart\n'
            'final stream = subscribeSessionEvents(sessionId);\n'
            'await for (final event in stream) {\n'
            '  reducer.apply(event);\n'
            '}',
        status: 'completed',
        createdAt: assistantCreatedAt,
        updatedAt: assistantCreatedAt,
        textChannel: TimelineTextChannel.finalAnswer,
      ),
    ];
    final state = StudioState(
      projects: const [project],
      sessions: [session],
      selectedProjectId: project.id,
      selectedSessionId: session.id,
      permissionMode: _permissionMode,
      turnPhase: TurnPhase.idle,
      runtime: const SessionRuntimeView(
        model: 'planner/local-responses',
        contextTokens: 18342,
        contextWindow: 128000,
        totalTokens: 26320,
        costLabel: 'CNY 0.16',
        activeSkills: [
          'flutter-apply-architecture-best-practices',
          'verification-before-completion',
        ],
        activeMcpServers: ['dart'],
        activeLspServers: ['rust-analyzer'],
        agentCount: 2,
      ),
      messagesBySession: {
        session.id: [
          TimelineMessage(
            id: 'turn-demo:user',
            sessionId: session.id,
            role: 'user',
            createdAt: userCreatedAt,
          ),
          TimelineMessage(
            id: 'turn-demo:assistant',
            sessionId: session.id,
            role: 'assistant',
            createdAt: assistantCreatedAt,
          ),
        ],
      },
      partSnapshotsBySession: {
        session.id: {for (final part in demoParts) part.id: part},
      },
      providers:
          _providers ??
          const [
            ProviderSettingsView(
              id: 'deepseek',
              templateKind: 'deepseek',
              name: 'DeepSeek',
              subtitle: 'DeepSeek Platform',
              baseUrl: 'https://api.deepseek.com',
              hasBearerToken: true,
              defaultModel: 'deepseek-reasoner',
              models: [
                ProviderModelView(
                  slug: 'deepseek-reasoner',
                  displayName: 'DeepSeek Reasoner',
                  reasoningEfforts: ['high', 'max'],
                  contextWindow: 1000000,
                  maxOutputTokens: 384000,
                  currency: 'CNY',
                  inputPricePerMTok: 3,
                  outputPricePerMTok: 6,
                ),
                ProviderModelView(
                  slug: 'deepseek-v4-flash',
                  displayName: 'DeepSeek V4 Flash',
                  reasoningEfforts: ['high', 'max'],
                  contextWindow: 1000000,
                  maxOutputTokens: 384000,
                  currency: 'CNY',
                  inputPricePerMTok: 1,
                  outputPricePerMTok: 2,
                ),
              ],
              defaultModels: [
                ProviderModelView(
                  slug: 'deepseek-reasoner',
                  displayName: 'DeepSeek Reasoner',
                  reasoningEfforts: ['high', 'max'],
                  contextWindow: 1000000,
                  maxOutputTokens: 384000,
                  currency: 'CNY',
                  inputPricePerMTok: 3,
                  outputPricePerMTok: 6,
                ),
                ProviderModelView(
                  slug: 'deepseek-v4-flash',
                  displayName: 'DeepSeek V4 Flash',
                  reasoningEfforts: ['high', 'max'],
                  contextWindow: 1000000,
                  maxOutputTokens: 384000,
                  currency: 'CNY',
                  inputPricePerMTok: 1,
                  outputPricePerMTok: 2,
                ),
              ],
              status: 'ready',
              usageLabel: 'Balance split available',
              modelCount: '2',
              updatedAt: 'Loaded',
              providerKind: 'deep_seek',
            ),
          ],
      roles:
          _roles ??
          const [
            RoleSettingsView(
              key: 'planner',
              providerId: 'deepseek',
              model: 'deepseek-reasoner',
              effort: 'high',
            ),
            RoleSettingsView(
              key: 'explorer',
              providerId: 'deepseek',
              model: 'deepseek-reasoner',
              effort: 'high',
            ),
            RoleSettingsView(
              key: 'executor',
              providerId: 'deepseek',
              model: 'deepseek-v4-flash',
              effort: 'high',
            ),
            RoleSettingsView(
              key: 'reviewer',
              providerId: 'deepseek',
              model: 'deepseek-v4-flash',
              effort: 'high',
            ),
          ],
      mcpServers: const [],
      instructions: _instructions,
      skills: _skills,
      general: _general,
      pendingInteractions: const [],
    );
    if (_archivedProjectIds.contains(project.id)) {
      return StudioState(
        projects: const [],
        sessions: const [],
        messagesBySession: const {},
        providers: state.providers,
        roles: state.roles,
        mcpServers: state.mcpServers,
        instructions: state.instructions,
        skills: state.skills,
        general: state.general,
        selectedProjectId: null,
        selectedSessionId: null,
        permissionMode: state.permissionMode,
        turnPhase: TurnPhase.idle,
        runtime: state.runtime,
        pendingInteractions: const [],
      );
    }
    return state;
  }

  @override
  Future<StudioState> loadSessionState(String sessionId) => bootstrap();

  @override
  Future<StudioState> openProject(String path) {
    _archivedProjectIds.remove('project-local');
    return bootstrap();
  }

  @override
  Future<StudioState> selectProject(String projectId) => bootstrap();

  @override
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  }) {
    _archivedProjectIds.add(projectId);
    return bootstrap();
  }

  @override
  Future<StudioState> createSession(String projectId, {String? title}) =>
      bootstrap();

  @override
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  }) => bootstrap();

  @override
  Future<StudioState> setSessionMode(String sessionId, CompileMode mode) async {
    final state = await bootstrap();
    return state.copyWith(
      sessions: [
        for (final session in state.sessions)
          session.id == sessionId ? session.copyWith(mode: mode) : session,
      ],
    );
  }

  @override
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedSessionId,
  }) async {
    final state = await bootstrap();
    final roles = [
      for (final role in state.roles)
        role.key == roleKey
            ? RoleSettingsView(
                key: role.key,
                providerId: providerId,
                model: model,
                effort: effort ?? role.effort,
              )
            : role,
    ];
    return state.copyWith(roles: roles);
  }

  @override
  Future<List<StudioBridgeEvent>> loadStudioEvents(
    String sessionId, {
    int? afterSequence,
    int limit = 500,
  }) async => const [];

  @override
  Future<void> resolveInteraction(
    String interactionId,
    Map<String, Object?> resolution,
  ) async {}

  @override
  Future<void> stopPrompt(String sessionId) async {
    _emitSessionEvent(
      sessionId: sessionId,
      payload: TurnChangedPayload(
        turn: StudioTurnView(sessionId: sessionId, status: 'cancelled'),
      ),
    );
  }

  @override
  Stream<Object> subscribeGlobalEvents() => _globalEvents.stream;

  @override
  Stream<Object> subscribeSessionEvents(String sessionId) =>
      _sessionEvents.stream;

  @override
  Future<void> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  ) async {
    final trimmed = prompt.trim();
    if (trimmed.isEmpty) {
      return;
    }
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final userMessageId = 'demo-user-$_eventSequence';
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessageUpdatedPayload(
        message: timelineMessageFromJson({
          'messageId': userMessageId,
          'sessionId': sessionId,
          'role': 'user',
          'createdAt': now,
        }),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: timelinePartSnapshotFromJson({
          'partId': '$userMessageId:text',
          'messageId': userMessageId,
          'sessionId': sessionId,
          'partType': 'text',
          'order': 0,
          'revision': 0,
          'status': 'completed',
          'createdAt': now,
          'updatedAt': now,
          'textChannel': 'user',
          'text': trimmed,
        }),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: TurnChangedPayload(
        turn: StudioTurnView(sessionId: sessionId, status: 'streaming'),
      ),
    );
    await Future<void>.delayed(const Duration(milliseconds: 120));
    final assistantMessageId = 'demo-assistant-$_eventSequence';
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessageUpdatedPayload(
        message: timelineMessageFromJson({
          'messageId': assistantMessageId,
          'sessionId': sessionId,
          'role': 'assistant',
          'createdAt': now + 1,
        }),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: MessagePartUpdatedPayload(
        part: timelinePartSnapshotFromJson({
          'partId': '$assistantMessageId:text',
          'messageId': assistantMessageId,
          'sessionId': sessionId,
          'partType': 'text',
          'order': 1,
          'revision': 0,
          'status': 'completed',
          'createdAt': now + 1,
          'updatedAt': now + 1,
          'textChannel': 'final',
          'text':
              'Demo response for: **$trimmed**\n\n'
              '- FRB session stream is connected\n'
              '- Markdown renders through the live timeline path',
        }),
      ),
    );
    _emitSessionEvent(
      sessionId: sessionId,
      payload: TurnChangedPayload(
        turn: StudioTurnView(sessionId: sessionId, status: 'completed'),
      ),
    );
  }

  @override
  Future<void> saveRuntimePermissionMode(PermissionMode mode) async {
    _permissionMode = mode;
  }

  @override
  Future<StudioState> saveProviderSettings(
    Map<String, Object?> settings,
  ) async {
    final current = await bootstrap();
    _providers = _providersFromSettingsPayload(
      settings,
      previous: current.providers,
    );
    _roles = _rolesFromSettingsPayload(settings);
    return bootstrap();
  }

  @override
  Future<StudioState> saveInstructionsSettings(
    Map<String, Object?> settings,
  ) async {
    _instructions = _instructionsFromSettingsPayload(settings);
    return bootstrap();
  }

  @override
  Future<StudioState> saveSkillsSettings(Map<String, Object?> settings) async {
    _skills = _skillsFromSettingsPayload(settings);
    return bootstrap();
  }

  @override
  Future<StudioState> saveMcpSettings(Map<String, Object?> settings) async {
    return bootstrap();
  }

  @override
  Future<StudioState> saveGeneralSettings(Map<String, Object?> settings) async {
    _general = _generalFromJson(settings);
    return bootstrap();
  }

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async {
    final state = await bootstrap();
    return [
      for (final provider in state.providers) _demoProviderUsage(provider),
    ];
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    if (_archivedProjectIds.contains(projectId)) {
      return const [];
    }
    return const ['flutter-ui-polish', 'runtime-review', 'studio-settings'];
  }

  @override
  Future<void> saveStudioSettingsDraft(
    String section,
    Map<String, Object?> draft,
  ) async {
    _settingsDrafts[section] = Map<String, Object?>.from(draft);
    _globalEvents.add(
      StudioBridgeEvent(
        payload: SettingsDraftSavedPayload(section: section, saved: true),
      ),
    );
  }

  void _emitSessionEvent({
    required String sessionId,
    required StudioBridgeEventPayload payload,
  }) {
    _eventSequence += 1;
    _sessionEvents.add(
      StudioBridgeEvent(
        payload: payload,
        sessionId: sessionId,
        sequence: BigInt.from(_eventSequence),
        createdAt: DateTime.now(),
      ),
    );
  }
}

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
