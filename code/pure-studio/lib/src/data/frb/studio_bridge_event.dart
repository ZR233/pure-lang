part of 'studio_api.dart';

class StudioBridgeEvent {
  const StudioBridgeEvent({
    required this.payload,
    this.eventId,
    this.sessionId,
    this.turnId,
    this.sequence,
    this.createdAt,
  });

  factory StudioBridgeEvent.fromProduct(frb.BridgeProductEventEnvelope event) {
    return StudioBridgeEvent(
      eventId: event.eventId,
      sequence: event.sequence,
      createdAt: _dateFromUnix(event.createdAt),
      payload: _productPayloadFromFrb(event.payload),
    );
  }

  factory StudioBridgeEvent.fromCanonicalJson(Map<String, Object?> event) {
    final position = _map(event['position']);
    final sequence = _string(position['persistence']) == 'durable'
        ? _int(position['sequence'])
        : 0;
    final sessionId = _string(event['sessionId']);
    return StudioBridgeEvent(
      eventId: _nullableString(event['eventId']),
      sessionId: sessionId,
      turnId: _nullableString(event['turnId']),
      sequence: sequence <= 0 ? null : BigInt.from(sequence),
      createdAt: _dateFromUnix(_int(event['emittedAt'])),
      payload: _canonicalEventPayload(
        _map(event['kind']),
        sequence: sequence,
        sessionId: sessionId,
      ),
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
    this.agentCount,
  });

  final SessionRuntimeView runtime;
  final int? agentCount;

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

final class SessionTaskChangedPayload extends StudioBridgeEventPayload {
  const SessionTaskChangedPayload({required this.sessionId, this.task});

  @override
  final String sessionId;
  final TaskRuntimeView? task;
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

StudioBridgeEventPayload _canonicalEventPayload(
  Map<String, Object?> kind, {
  required int sequence,
  required String sessionId,
}) {
  return switch (_string(kind['type'])) {
    'turnChanged' => switch (_map(kind['turn'])) {
      final turn => TurnChangedPayload(
        turn: StudioTurnView(
          sessionId: _string(turn['sessionId'], fallback: sessionId),
          status: _string(turn['status']),
        ),
      ),
    },
    'messageChanged' => MessageUpdatedPayload(
      message: timelineMessageFromJson(kind['message'], sequence: sequence),
    ),
    'messageRemoved' => MessageRemovedPayload(
      messageId: _string(kind['messageId']),
    ),
    'partChanged' => switch (_canonicalLegacyPartJson(kind['part'])) {
      final part when _isIgnoredTimelinePartType(part['type']) =>
        const IgnoredBridgeEventPayload(),
      final part => MessagePartUpdatedPayload(
        part: timelinePartSnapshotFromJson(part, sequence: sequence),
      ),
    },
    'partRemoved' => MessagePartRemovedPayload(
      messageId: _string(kind['messageId']),
      partId: _string(kind['partId']),
    ),
    'partDelta' => MessagePartDeltaPayload(
      delta: timelinePartDeltaFromJson(kind['delta']),
    ),
    'interactionChanged' => switch (_map(_map(kind['event'])['interaction'])) {
      final interaction => InteractionChangedPayload(
        interaction: pendingInteractionFromJson(interaction),
        status: _string(interaction['status']),
      ),
    },
    'agentChanged' => switch (_map(kind['agent'])) {
      final agent => AgentChangedPayload(
        agent: StudioAgentView(
          id: _string(agent['id']),
          sessionId: _string(agent['sessionId']),
          path: _string(agent['path']),
          parentPath: _nullableString(agent['parentPath']),
          role: _string(agent['role'], fallback: 'agent'),
          task: _string(agent['task']),
          status: _string(agent['status']),
          summary: _nullableString(agent['summary']),
          depth: _int(agent['depth']),
          error: _nullableString(agent['error']),
          reason: _nullableString(agent['reason']),
          updatedAt: _dateFromUnix(_int(agent['updatedAt'])),
        ),
      ),
    },
    'timelineEventAppended' => AgentTimelineChangedPayload(
      event: timelineAgentEventFromPayload(kind['event']),
    ),
    'runtimeChanged' => SessionRuntimeChangedPayload(
      runtime: sessionRuntimeFromJson(kind['runtime']),
      sessionId: sessionId,
      agentCount: _int(_map(kind['runtime'])['agentCount']),
    ),
    'skillActivated' => SkillActivatedPayload(
      name: _string(_map(kind['activation'])['name']),
    ),
    'planChanged' => PlanLifecycleChangedPayload(
      state: _string(_map(kind['event'])['state']),
    ),
    'contextCompacted' || 'errorOccurred' => const IgnoredBridgeEventPayload(),
    final type => throw FormatException('Unknown session event kind: $type'),
  };
}
