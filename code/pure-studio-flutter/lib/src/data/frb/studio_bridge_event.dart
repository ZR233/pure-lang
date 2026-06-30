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
