part of '../widget_test.dart';

StudioBridgeEvent _messageUpdatedEvent({
  required String sessionId,
  required TimelineMessage message,
  BigInt? sequence,
}) {
  final eventMessage = TimelineMessage(
    id: message.id,
    sessionId: message.sessionId,
    turnId: message.turnId,
    role: message.role,
    status: message.status,
    createdAt: message.createdAt,
    updatedAt: message.updatedAt,
    completedAt: message.completedAt,
    error: message.error,
    sequence: sequence?.toInt() ?? message.sequence,
  );
  return StudioBridgeEvent(
    sessionId: sessionId,
    sequence: sequence,
    payload: MessageUpdatedPayload(message: eventMessage),
  );
}

StudioBridgeEvent _partUpdatedEvent({
  required String sessionId,
  required TimelinePartSnapshot part,
  BigInt? sequence,
}) {
  final eventPart = TimelinePartSnapshot(
    id: part.id,
    messageId: part.messageId,
    sessionId: part.sessionId,
    turnId: part.turnId,
    type: part.type,
    order: part.order,
    revision: part.revision,
    sequence: sequence?.toInt() ?? part.sequence,
    text: part.text,
    reasoningSummary: part.reasoningSummary,
    reasoningContent: part.reasoningContent,
    status: part.status,
    createdAt: part.createdAt,
    updatedAt: part.updatedAt,
    completedAt: part.completedAt,
    error: part.error,
    textChannel: part.textChannel,
    tool: part.tool,
    agent: part.agent,
    planContent: part.planContent,
    synthetic: part.synthetic,
    ignored: part.ignored,
  );
  return StudioBridgeEvent(
    sessionId: sessionId,
    sequence: sequence,
    payload: MessagePartUpdatedPayload(part: eventPart),
  );
}

StudioBridgeEvent _partDeltaEvent({
  required String sessionId,
  required TimelinePartDelta delta,
  BigInt? sequence,
  String? eventId,
  DateTime? createdAt,
}) {
  return StudioBridgeEvent(
    eventId: eventId,
    sessionId: sessionId,
    sequence: sequence,
    createdAt: createdAt,
    payload: MessagePartDeltaPayload(delta: delta),
  );
}

StudioBridgeEvent _turnChangedEvent({
  required String sessionId,
  required StudioTurnState state,
}) {
  return StudioBridgeEvent(
    sessionId: sessionId,
    payload: TurnChangedPayload(
      turn: _testTurn(sessionId: sessionId, state: state),
    ),
  );
}

StudioBridgeEvent _agentTimelineEvent({
  required String sessionId,
  required TimelineAgentEvent event,
}) {
  return StudioBridgeEvent(
    sessionId: sessionId,
    payload: AgentTimelineChangedPayload(event: event),
  );
}

TimelineMessage _timelineMessageFixture({
  required String id,
  required String sessionId,
  String turnId = '',
  String role = 'assistant',
  String status = 'completed',
  int createdAt = 1,
  int? updatedAt,
  int? completedAt,
  String? error,
  int sequence = 0,
}) {
  return TimelineMessage(
    id: id,
    sessionId: sessionId,
    turnId: turnId,
    role: role,
    status: status,
    createdAt: _fixtureDate(createdAt),
    updatedAt: _fixtureDate(updatedAt ?? createdAt),
    completedAt: completedAt == null ? null : _fixtureDate(completedAt),
    error: error,
    sequence: sequence,
  );
}

TimelinePartSnapshot _timelinePartFixture({
  required String id,
  required String messageId,
  required String sessionId,
  required String turnId,
  required TimelinePartType type,
  int order = 0,
  int revision = 0,
  int sequence = 0,
  String text = '',
  List<String> reasoningSummary = const [],
  List<String> reasoningContent = const [],
  String status = 'completed',
  int createdAt = 1,
  int? updatedAt,
  int? completedAt,
  String? error,
  TimelineTextChannel? textChannel,
  TimelineToolPart? tool,
  TimelineAgentPart? agent,
  String? planContent,
  bool synthetic = false,
  bool ignored = false,
}) {
  return TimelinePartSnapshot(
    id: id,
    messageId: messageId,
    sessionId: sessionId,
    turnId: turnId,
    type: type,
    order: order,
    revision: revision,
    sequence: sequence,
    text: text,
    reasoningSummary: reasoningSummary,
    reasoningContent: reasoningContent,
    status: status,
    createdAt: _fixtureDate(createdAt),
    updatedAt: _fixtureDate(updatedAt ?? createdAt),
    completedAt: completedAt == null ? null : _fixtureDate(completedAt),
    error: error,
    textChannel: textChannel,
    tool: tool,
    agent: agent,
    planContent: planContent,
    synthetic: synthetic,
    ignored: ignored,
  );
}

TimelinePartDelta _timelineDeltaFixture({
  required String partId,
  required int revision,
  required String field,
  required String delta,
  int? chunkIndex,
}) {
  return TimelinePartDelta(
    partId: partId,
    revision: revision,
    field: field,
    delta: delta,
    chunkIndex: chunkIndex,
  );
}

DateTime _fixtureDate(int unixSeconds) =>
    DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000);

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
