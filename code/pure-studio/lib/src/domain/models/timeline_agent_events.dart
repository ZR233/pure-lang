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

String _agentActivityText(Iterable<String?> parts) {
  return parts
      .whereType<String>()
      .where((part) => part.trim().isNotEmpty)
      .join('\n');
}
