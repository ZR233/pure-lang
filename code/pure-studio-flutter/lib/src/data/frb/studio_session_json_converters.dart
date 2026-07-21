part of 'studio_api.dart';

TimelineMessage timelineMessageFromJson(Object? value, {int sequence = 0}) {
  final json = _map(value);
  final createdAt = _int(json['createdAt']);
  final updatedAt = _nullableInt(json['updatedAt']) ?? createdAt;
  return TimelineMessage(
    id: _string(json['messageId']),
    sessionId: _string(json['sessionId']),
    turnId: _string(json['turnId']),
    role: _string(json['role'], fallback: 'assistant'),
    status: _string(json['status'], fallback: 'completed'),
    createdAt: _dateFromUnix(createdAt),
    updatedAt: _dateFromUnix(updatedAt),
    completedAt: _nullableInt(json['completedAt']) == null
        ? null
        : _dateFromUnix(_nullableInt(json['completedAt'])!),
    error: _nullableString(json['error']),
    sequence: sequence,
  );
}

TimelinePartSnapshot timelinePartSnapshotFromJson(
  Object? value, {
  int sequence = 0,
}) {
  final json = _map(value);
  final type = _partType(json['type']);
  return TimelinePartSnapshot(
    id: _string(json['partId']),
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
    activityGroupId: _nullableString(json['activityGroupId']),
    tool: _toolPart(json['tool']),
    agent: _agentPart(json['agent']),
    planContent: _partPlanContent(json),
    synthetic: _bool(json['synthetic']),
    ignored: _bool(json['ignored']),
  );
}

TimelinePartDelta timelinePartDeltaFromJson(Object? value) {
  final json = _map(value);
  return TimelinePartDelta(
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
  final agentCount = _int(json['agentCount']);
  return SessionRuntimeView(
    model: _string(usage['model']),
    contextTokens: _int(usage['latestContextTokens']),
    contextWindow: _int(usage['contextWindow']),
    totalTokens: _int(usage['totalTokens']),
    costLabel: _costLabel(
      usage['estimatedCosts'],
      _bool(usage['hasUnpricedUsage']),
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
  final kind = _interactionKind(_string(json['kind']));
  return PendingInteraction(
    id: _string(json['interactionId']),
    sessionId: _string(scope['sessionId']),
    kind: kind,
    title: _interactionTitle(kind, payload),
    body: _interactionBody(kind, payload),
    payload: payload,
  );
}
