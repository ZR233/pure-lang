part of 'studio_api.dart';

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

List<StudioSession> studioSessionsFromJson(Object? value) {
  return _list(value).map(_sessionFromJson).toList();
}

TimelineMessage timelineMessageFromJson(Object? value, {int sequence = 0}) {
  final json = _map(value);
  final createdAt = _int(json['createdAt']);
  final updatedAt = _nullableInt(json['updatedAt']) ?? createdAt;
  return TimelineMessage(
    id: _string(json['messageId'], fallback: _string(json['id'])),
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
  final type = _partType(
    _firstValue(json, const ['partType', 'part_type', 'type']),
  );
  return TimelinePartSnapshot(
    id: _string(_firstValue(json, const ['partId', 'part_id', 'id'])),
    messageId: _string(_firstValue(json, const ['messageId', 'message_id'])),
    sessionId: _string(_firstValue(json, const ['sessionId', 'session_id'])),
    turnId: _string(_firstValue(json, const ['turnId', 'turn_id'])),
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
    textChannel: _textChannel(
      _firstValue(json, const ['textChannel', 'text_channel']),
    ),
    activityGroupId: _nullableString(
      _firstValue(json, const ['activityGroupId', 'activity_group_id']),
    ),
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
