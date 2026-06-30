part of 'timeline_models.dart';

class TimelineAgentEvent {
  const TimelineAgentEvent({
    required this.eventId,
    required this.sessionId,
    required this.sequence,
    required this.payload,
    required this.createdAt,
  });

  final String eventId;
  final String sessionId;
  final int sequence;
  final TimelineAgentEventPayload payload;
  final DateTime createdAt;

  String get callId => payload.callId;

  String get title {
    return switch (payload) {
      TimelineAgentSpawnBegin() ||
      TimelineAgentSpawnEnd() => 'agentTimeline.spawn',
      TimelineAgentInteractionBegin() ||
      TimelineAgentInteractionEnd() => 'agentTimeline.message',
      TimelineAgentWaitingBegin() ||
      TimelineAgentWaitingEnd() => 'agentTimeline.waiting',
      TimelineAgentCloseBegin() ||
      TimelineAgentCloseEnd() => 'agentTimeline.close',
    };
  }

  String get text => payload.activityText;

  String get status => payload.status;
}

sealed class TimelineAgentEventPayload {
  const TimelineAgentEventPayload();

  String get callId;

  String get status => 'completed';

  String get activityText;
}

class TimelineAgentSpawnBegin extends TimelineAgentEventPayload {
  const TimelineAgentSpawnBegin({
    required this.callId,
    required this.senderPath,
    required this.taskName,
    required this.prompt,
    required this.role,
    this.model,
    this.reasoningEffort,
  });

  @override
  final String callId;
  final String senderPath;
  final String taskName;
  final String prompt;
  final String role;
  final String? model;
  final String? reasoningEffort;

  @override
  String get activityText => _agentActivityText([senderPath, taskName, prompt]);
}

class TimelineAgentSpawnEnd extends TimelineAgentEventPayload {
  const TimelineAgentSpawnEnd({
    required this.callId,
    required this.senderPath,
    required this.status,
    required this.prompt,
    this.agentId,
    this.path,
    this.role,
    this.error,
  });

  @override
  final String callId;
  final String senderPath;
  @override
  final String status;
  final String prompt;
  final String? agentId;
  final String? path;
  final String? role;
  final String? error;

  @override
  String get activityText =>
      _agentActivityText([path, senderPath, prompt, error]);
}

class TimelineAgentInteractionBegin extends TimelineAgentEventPayload {
  const TimelineAgentInteractionBegin({
    required this.callId,
    required this.senderPath,
    required this.receiverPath,
    required this.prompt,
  });

  @override
  final String callId;
  final String senderPath;
  final String receiverPath;
  final String prompt;

  @override
  String get activityText =>
      _agentActivityText([receiverPath, senderPath, prompt]);
}

class TimelineAgentInteractionEnd extends TimelineAgentEventPayload {
  const TimelineAgentInteractionEnd({
    required this.callId,
    required this.senderPath,
    required this.receiverPath,
    required this.status,
    required this.prompt,
    this.error,
  });

  @override
  final String callId;
  final String senderPath;
  final String receiverPath;
  @override
  final String status;
  final String prompt;
  final String? error;

  @override
  String get activityText =>
      _agentActivityText([receiverPath, senderPath, prompt, error]);
}

class TimelineAgentWaitingBegin extends TimelineAgentEventPayload {
  const TimelineAgentWaitingBegin({
    required this.callId,
    required this.senderPath,
  });

  @override
  final String callId;
  final String senderPath;

  @override
  String get activityText => _agentActivityText([senderPath]);
}

class TimelineAgentWaitingEnd extends TimelineAgentEventPayload {
  const TimelineAgentWaitingEnd({
    required this.callId,
    required this.senderPath,
    required this.timedOut,
  });

  @override
  final String callId;
  final String senderPath;
  final bool timedOut;

  @override
  String get activityText => _agentActivityText([senderPath]);
}

class TimelineAgentCloseBegin extends TimelineAgentEventPayload {
  const TimelineAgentCloseBegin({
    required this.callId,
    required this.senderPath,
    required this.receiverPath,
  });

  @override
  final String callId;
  final String senderPath;
  final String receiverPath;

  @override
  String get activityText => _agentActivityText([receiverPath, senderPath]);
}

class TimelineAgentCloseEnd extends TimelineAgentEventPayload {
  const TimelineAgentCloseEnd({
    required this.callId,
    required this.senderPath,
    required this.receiverPath,
    required this.status,
    this.error,
  });

  @override
  final String callId;
  final String senderPath;
  final String receiverPath;
  @override
  final String status;
  final String? error;

  @override
  String get activityText =>
      _agentActivityText([receiverPath, senderPath, error]);
}

TimelineAgentEvent timelineAgentEventFromPayload(
  Object? value, {
  String? eventId,
  String? sessionId,
  int sequence = 0,
  DateTime? createdAt,
  String? kindType,
}) {
  final payload = _objectMap(value);
  final kind = _objectMap(payload['kind']);
  final eventKind = kind.isEmpty ? payload : kind;
  final resolvedKindType = _nonEmpty(kindType, _stringValue(eventKind['type']));
  return TimelineAgentEvent(
    eventId: _nonEmpty(eventId, _stringValue(payload['eventId'])),
    sessionId: _nonEmpty(sessionId, _stringValue(payload['sessionId'])),
    sequence: sequence == 0 ? _intValue(payload['sequence']) : sequence,
    payload: _agentEventPayloadFromMap(resolvedKindType, eventKind),
    createdAt:
        createdAt ??
        DateTime.fromMillisecondsSinceEpoch(
          _intValue(payload['createdAt']) * 1000,
        ),
  );
}

TimelineRow timelineRowFromAgentEvent(TimelineAgentEvent event) {
  return TimelineRow.agentActivity(event);
}

String timelineAgentEventGroupKey(TimelineAgentEvent event) {
  return event.callId.isEmpty ? event.eventId : event.callId;
}

int compareTimelineAgentEvents(
  TimelineAgentEvent left,
  TimelineAgentEvent right,
) {
  final sequence = left.sequence.compareTo(right.sequence);
  if (sequence != 0) {
    return sequence;
  }
  final createdAt = left.createdAt.compareTo(right.createdAt);
  if (createdAt != 0) {
    return createdAt;
  }
  return left.eventId.compareTo(right.eventId);
}

List<TimelineAgentEvent> latestTimelineAgentEvents(
  Iterable<TimelineAgentEvent> events,
) {
  final latestByGroup = <String, TimelineAgentEvent>{};
  for (final event in events) {
    if (event.eventId.isEmpty || event.sessionId.isEmpty) {
      continue;
    }
    final key = timelineAgentEventGroupKey(event);
    final existing = latestByGroup[key];
    if (existing == null || compareTimelineAgentEvents(existing, event) < 0) {
      latestByGroup[key] = event;
    }
  }
  return latestByGroup.values.toList()..sort(compareTimelineAgentEvents);
}

int _timelineAgentEventRenderVersion(TimelineAgentEvent event) {
  return Object.hashAll([
    event.eventId,
    event.sequence,
    event.callId,
    event.title,
    event.text,
    event.status,
    event.payload.runtimeType,
    event.createdAt.millisecondsSinceEpoch,
  ]);
}

TimelineAgentEventPayload _agentEventPayloadFromMap(
  String kindType,
  Map<String, Object?> kind,
) {
  return switch (kindType) {
    'spawnBegin' => TimelineAgentSpawnBegin(
      callId: _stringValue(kind['callId']) ?? '',
      senderPath: _stringValue(kind['senderPath']) ?? '',
      taskName: _stringValue(kind['taskName']) ?? '',
      prompt: _stringValue(kind['prompt']) ?? '',
      role: _stringValue(kind['role']) ?? '',
      model: _stringValue(kind['model']),
      reasoningEffort: _stringValue(kind['reasoningEffort']),
    ),
    'spawnEnd' => TimelineAgentSpawnEnd(
      callId: _stringValue(kind['callId']) ?? '',
      senderPath: _stringValue(kind['senderPath']) ?? '',
      agentId: _stringValue(kind['agentId']),
      path: _stringValue(kind['path']),
      role: _stringValue(kind['role']),
      status: _stringValue(kind['status']) ?? 'completed',
      prompt: _stringValue(kind['prompt']) ?? '',
      error: _stringValue(kind['error']),
    ),
    'interactionBegin' => TimelineAgentInteractionBegin(
      callId: _stringValue(kind['callId']) ?? '',
      senderPath: _stringValue(kind['senderPath']) ?? '',
      receiverPath: _stringValue(kind['receiverPath']) ?? '',
      prompt: _stringValue(kind['prompt']) ?? '',
    ),
    'interactionEnd' => TimelineAgentInteractionEnd(
      callId: _stringValue(kind['callId']) ?? '',
      senderPath: _stringValue(kind['senderPath']) ?? '',
      receiverPath: _stringValue(kind['receiverPath']) ?? '',
      status: _stringValue(kind['status']) ?? 'completed',
      prompt: _stringValue(kind['prompt']) ?? '',
      error: _stringValue(kind['error']),
    ),
    'waitingBegin' => TimelineAgentWaitingBegin(
      callId: _stringValue(kind['callId']) ?? '',
      senderPath: _stringValue(kind['senderPath']) ?? '',
    ),
    'waitingEnd' => TimelineAgentWaitingEnd(
      callId: _stringValue(kind['callId']) ?? '',
      senderPath: _stringValue(kind['senderPath']) ?? '',
      timedOut: _boolValue(kind['timedOut']),
    ),
    'closeBegin' => TimelineAgentCloseBegin(
      callId: _stringValue(kind['callId']) ?? '',
      senderPath: _stringValue(kind['senderPath']) ?? '',
      receiverPath: _stringValue(kind['receiverPath']) ?? '',
    ),
    'closeEnd' => TimelineAgentCloseEnd(
      callId: _stringValue(kind['callId']) ?? '',
      senderPath: _stringValue(kind['senderPath']) ?? '',
      receiverPath: _stringValue(kind['receiverPath']) ?? '',
      status: _stringValue(kind['status']) ?? 'completed',
      error: _stringValue(kind['error']),
    ),
    _ => throw FormatException('Unknown agent timeline event type: $kindType'),
  };
}

String _agentActivityText(Iterable<String?> parts) {
  return parts
      .whereType<String>()
      .where((part) => part.trim().isNotEmpty)
      .join('\n');
}
