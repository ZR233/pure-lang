part of '../widget_test.dart';

StudioBridgeEvent _canonicalSessionEvent({
  required String sessionId,
  required Map<String, Object?> kind,
  int sequence = 1,
  int emittedAt = 1,
  String? eventId,
  String? turnId,
}) {
  return StudioBridgeEvent.fromCanonicalJson({
    'eventId': eventId ?? 'event-$sequence',
    'sessionId': sessionId,
    'turnId': ?turnId,
    'emittedAt': emittedAt,
    'position': {'persistence': 'durable', 'sequence': sequence},
    'kind': kind,
  });
}

StudioBridgeEvent _messageUpdatedEvent({
  required String sessionId,
  required Map<String, Object?> message,
  BigInt? sequence,
}) {
  return StudioBridgeEvent(
    sessionId: sessionId,
    sequence: sequence,
    payload: MessageUpdatedPayload(
      message: timelineMessageFromJson({
        ...message,
        if (!message.containsKey('sessionId')) 'sessionId': sessionId,
      }, sequence: sequence?.toInt() ?? 0),
    ),
  );
}

StudioBridgeEvent _partUpdatedEvent({
  required String sessionId,
  required Map<String, Object?> part,
  BigInt? sequence,
}) {
  return StudioBridgeEvent(
    sessionId: sessionId,
    sequence: sequence,
    payload: MessagePartUpdatedPayload(
      part: timelinePartSnapshotFromJson({
        ...part,
        if (!part.containsKey('sessionId')) 'sessionId': sessionId,
      }, sequence: sequence?.toInt() ?? 0),
    ),
  );
}

StudioBridgeEvent _partDeltaEvent({
  required String sessionId,
  required Map<String, Object?> delta,
  BigInt? sequence,
  String? eventId,
  DateTime? createdAt,
}) {
  return StudioBridgeEvent(
    eventId: eventId,
    sessionId: sessionId,
    sequence: sequence,
    createdAt: createdAt,
    payload: MessagePartDeltaPayload(delta: timelinePartDeltaFromJson(delta)),
  );
}

StudioBridgeEvent _turnChangedEvent({
  required String sessionId,
  required String status,
}) {
  return StudioBridgeEvent(
    sessionId: sessionId,
    payload: TurnChangedPayload(
      turn: StudioTurnView(sessionId: sessionId, status: status),
    ),
  );
}

StudioBridgeEvent _agentTimelineEvent({
  required String sessionId,
  required Map<String, Object?> event,
}) {
  return StudioBridgeEvent(
    sessionId: sessionId,
    payload: AgentTimelineChangedPayload(
      event: timelineAgentEventFromPayload({
        ...event,
        if (!event.containsKey('sessionId')) 'sessionId': sessionId,
      }),
    ),
  );
}

StudioBridgeEvent _sessionListChangedEvent({
  required String? projectId,
  required List<StudioSession> sessions,
}) {
  return StudioBridgeEvent(
    payload: SessionListChangedPayload(
      projectId: projectId,
      sessions: sessions,
    ),
  );
}

StudioBridgeEvent _sessionRuntimeChangedEvent({
  required String sessionId,
  required SessionRuntimeView runtime,
}) {
  return StudioBridgeEvent(
    sessionId: sessionId,
    payload: SessionRuntimeChangedPayload(
      sessionId: sessionId,
      runtime: runtime,
    ),
  );
}

StudioBridgeEvent _interactionChangedEvent({
  required String sessionId,
  required PendingInteraction interaction,
  String status = 'pending',
}) {
  return StudioBridgeEvent(
    sessionId: sessionId,
    payload: InteractionChangedPayload(
      interaction: interaction,
      status: status,
    ),
  );
}
