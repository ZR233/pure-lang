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
      TimelineSubAgentActivity(:final kind) => switch (kind) {
        'spawned' => 'agentTimeline.spawn',
        'messageQueued' || 'followupStarted' => 'agentTimeline.message',
        'waitCompleted' => 'agentTimeline.waiting',
        'closed' => 'agentTimeline.close',
        _ => 'agentTimeline.activity',
      },
      TimelineTodoListUpdate() => 'agentTimeline.todoList',
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

class TimelineSubAgentActivity extends TimelineAgentEventPayload {
  const TimelineSubAgentActivity({
    required this.callId,
    required this.kind,
    required this.timedOut,
    this.agentId,
    this.path,
    this.parentPath,
    this.statusValue,
    this.message,
    this.error,
  });

  @override
  final String callId;
  final String kind;
  final bool timedOut;
  final String? agentId;
  final String? path;
  final String? parentPath;
  final String? statusValue;
  final String? message;
  final String? error;

  @override
  String get status => statusValue ?? (error == null ? 'completed' : 'errored');

  @override
  String get activityText => _agentActivityText([
    path,
    parentPath,
    message,
    timedOut ? 'timed out' : null,
    error,
  ]);
}

class TimelineTodoListUpdate extends TimelineAgentEventPayload {
  const TimelineTodoListUpdate({
    required this.callId,
    required this.items,
    this.agentId,
    this.path,
    this.parentPath,
    this.explanation,
  });

  @override
  final String callId;
  final String? agentId;
  final String? path;
  final String? parentPath;
  final String? explanation;
  final List<TimelineTodoItem> items;

  @override
  String get status {
    if (items.any((item) => item.status == 'inProgress')) {
      return 'running';
    }
    if (items.isNotEmpty && items.every((item) => item.status == 'completed')) {
      return 'completed';
    }
    return 'pending';
  }

  @override
  String get activityText =>
      _agentActivityText([path, parentPath, explanation]);
}

class TimelineTodoItem {
  const TimelineTodoItem({required this.step, required this.status});

  final String step;
  final String status;
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
  if (event.payload is TimelineTodoListUpdate) {
    return event.eventId;
  }
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
    if (event.payload case TimelineTodoListUpdate(:final items))
      for (final item in items) ...[item.step, item.status],
    event.createdAt.millisecondsSinceEpoch,
  ]);
}

TimelineAgentEventPayload _agentEventPayloadFromMap(
  String kindType,
  Map<String, Object?> kind,
) {
  return switch (kindType) {
    'subAgentActivity' => TimelineSubAgentActivity(
      callId: _stringValue(kind['callId']) ?? '',
      agentId: _stringValue(kind['agentId']),
      path: _stringValue(kind['path']),
      parentPath: _stringValue(kind['parentPath']),
      kind: _stringValue(kind['kind']) ?? 'spawned',
      statusValue: _stringValue(kind['status']),
      message: _stringValue(kind['message']),
      timedOut: _boolValue(kind['timedOut']),
      error: _stringValue(kind['error']),
    ),
    'todoListUpdated' => _todoListPayloadFromMap(kind),
    _ => throw FormatException('Unknown agent timeline event type: $kindType'),
  };
}

TimelineTodoListUpdate _todoListPayloadFromMap(Map<String, Object?> kind) {
  final snapshot = _objectMap(kind['snapshot']);
  return TimelineTodoListUpdate(
    callId: _stringValue(snapshot['callId']) ?? '',
    agentId: _stringValue(snapshot['agentId']),
    path: _stringValue(snapshot['path']),
    parentPath: _stringValue(snapshot['parentPath']),
    explanation: _stringValue(snapshot['explanation']),
    items: [
      for (final item in _listValue(snapshot['items']))
        TimelineTodoItem(
          step: _stringValue(_objectMap(item)['step']) ?? '',
          status: _stringValue(_objectMap(item)['status']) ?? 'pending',
        ),
    ],
  );
}

String _agentActivityText(Iterable<String?> parts) {
  return parts
      .whereType<String>()
      .where((part) => part.trim().isNotEmpty)
      .join('\n');
}
