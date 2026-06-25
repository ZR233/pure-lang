import 'dart:convert';

import 'studio_enums.dart';

enum TimelineTextChannel { user, commentary, finalAnswer }

enum TimelineRowType {
  userMessage,
  commentary,
  toolActivity,
  reasoningSummary,
  plan,
  agentActivity,
  finalAnswer,
}

class TimelineToolPart {
  const TimelineToolPart({
    required this.toolCallId,
    required this.name,
    this.callId,
    this.providerItemId,
    this.arguments = '',
    this.result,
    this.exitCode,
    this.timedOut = false,
    this.workingDirectory,
    this.denialReason,
  });

  final String toolCallId;
  final String name;
  final String? callId;
  final String? providerItemId;
  final String arguments;
  final String? result;
  final int? exitCode;
  final bool timedOut;
  final String? workingDirectory;
  final String? denialReason;

  TimelineToolPart copyWith({String? arguments, String? result}) {
    return TimelineToolPart(
      toolCallId: toolCallId,
      name: name,
      callId: callId,
      providerItemId: providerItemId,
      arguments: arguments ?? this.arguments,
      result: result ?? this.result,
      exitCode: exitCode,
      timedOut: timedOut,
      workingDirectory: workingDirectory,
      denialReason: denialReason,
    );
  }
}

class TimelineAgentPart {
  const TimelineAgentPart({
    required this.id,
    required this.path,
    required this.role,
    required this.task,
    required this.status,
    this.parentPath,
    this.summary,
    this.depth = 0,
    this.error,
    this.reason,
  });

  final String id;
  final String path;
  final String? parentPath;
  final String role;
  final String task;
  final String status;
  final String? summary;
  final int depth;
  final String? error;
  final String? reason;
}

class TimelineAgentEvent {
  const TimelineAgentEvent({
    required this.eventId,
    required this.sessionId,
    required this.sequence,
    required this.callId,
    required this.kindType,
    required this.title,
    required this.text,
    required this.status,
    required this.createdAt,
  });

  final String eventId;
  final String sessionId;
  final int sequence;
  final String callId;
  final String kindType;
  final String title;
  final String text;
  final String status;
  final DateTime createdAt;
}

class TimelinePartSnapshot {
  const TimelinePartSnapshot({
    required this.id,
    required this.messageId,
    required this.sessionId,
    required this.turnId,
    required this.type,
    required this.order,
    required this.revision,
    this.sequence = 0,
    required this.text,
    required this.status,
    required this.createdAt,
    required this.updatedAt,
    this.completedAt,
    this.error,
    this.textChannel,
    this.tool,
    this.agent,
    this.planContent,
    this.synthetic = false,
    this.ignored = false,
  });

  final String id;
  final String messageId;
  final String sessionId;
  final String turnId;
  final TimelinePartType type;
  final int order;
  final int revision;
  final int sequence;
  final String text;
  final String status;
  final DateTime createdAt;
  final DateTime updatedAt;
  final DateTime? completedAt;
  final String? error;
  final TimelineTextChannel? textChannel;
  final TimelineToolPart? tool;
  final TimelineAgentPart? agent;
  final String? planContent;
  final bool synthetic;
  final bool ignored;
}

class TimelinePartDelta {
  const TimelinePartDelta({
    required this.sessionId,
    required this.messageId,
    required this.partId,
    required this.revision,
    required this.field,
    required this.delta,
    this.chunkIndex,
  });

  final String sessionId;
  final String messageId;
  final String partId;
  final int revision;
  final String field;
  final String delta;
  final int? chunkIndex;
}

class TimelinePartOverlay {
  const TimelinePartOverlay({
    this.values = const {},
    this.lastRevisions = const {},
    this.lastChunkIndexes = const {},
  });

  final Map<String, String> values;
  final Map<String, int> lastRevisions;
  final Map<String, int> lastChunkIndexes;

  TimelinePartOverlay append({
    required String field,
    required String value,
    required int revision,
    int? chunkIndex,
  }) {
    return TimelinePartOverlay(
      values: {...values, field: value},
      lastRevisions: {...lastRevisions, field: revision},
      lastChunkIndexes: chunkIndex == null
          ? lastChunkIndexes
          : {...lastChunkIndexes, field: chunkIndex},
    );
  }
}

class TimelinePart {
  const TimelinePart({
    required this.id,
    required this.messageId,
    required this.type,
    required this.text,
    this.order = 0,
    this.sequence = 0,
    this.status = 'completed',
    this.revision = 0,
    this.title,
    this.textChannel,
    this.tool,
    this.agent,
    this.collapsed = false,
    this.synthetic = false,
    this.ignored = false,
  });

  final String id;
  final String messageId;
  final TimelinePartType type;
  final int order;
  final int sequence;
  final String text;
  final String? title;
  final String status;
  final int revision;
  final TimelineTextChannel? textChannel;
  final TimelineToolPart? tool;
  final TimelineAgentPart? agent;
  final bool collapsed;
  final bool synthetic;
  final bool ignored;
}

TimelinePart timelinePartFromSnapshot(
  TimelinePartSnapshot snapshot, {
  TimelinePartOverlay? overlay,
}) {
  final text = overlay?.values['text'] ?? snapshot.text;
  final planContent = overlay?.values['planContent'] ?? snapshot.planContent;
  final snapshotTool = snapshot.tool;
  final tool = snapshotTool?.copyWith(
    arguments: overlay?.values['tool.arguments'] ?? snapshotTool.arguments,
    result: overlay?.values['tool.result'] ?? snapshotTool.result,
  );
  final visibleText = switch (snapshot.type) {
    TimelinePartType.plan => planContent ?? text,
    TimelinePartType.tool => _toolActivityText(tool),
    TimelinePartType.agent =>
      snapshot.agent?.summary ?? snapshot.agent?.task ?? text,
    TimelinePartType.reasoning => '',
    TimelinePartType.text => text,
  };
  return TimelinePart(
    id: snapshot.id,
    messageId: snapshot.messageId,
    type: snapshot.type,
    order: snapshot.order,
    sequence: snapshot.sequence,
    revision: snapshot.revision,
    title: _partTitleFromSnapshot(snapshot),
    text: visibleText,
    status: snapshot.status,
    textChannel: snapshot.textChannel,
    tool: tool,
    agent: snapshot.agent,
    collapsed: snapshot.type == TimelinePartType.reasoning,
    synthetic: snapshot.synthetic,
    ignored: snapshot.ignored,
  );
}

String _partTitleFromSnapshot(TimelinePartSnapshot snapshot) {
  return switch (snapshot.type) {
    TimelinePartType.tool => snapshot.tool?.name ?? 'Tool',
    TimelinePartType.plan => 'Plan',
    TimelinePartType.agent => snapshot.agent?.role ?? 'Agent',
    TimelinePartType.reasoning => 'Reasoning',
    TimelinePartType.text => '',
  };
}

String _toolActivityText(TimelineToolPart? tool) {
  if (tool == null) {
    return '';
  }
  return [
    _commandSummary(tool.arguments),
    tool.workingDirectory,
    tool.denialReason,
    tool.result,
  ].whereType<String>().where((value) => value.trim().isNotEmpty).join('\n');
}

String? _commandSummary(String arguments) {
  final json = _tryDecodeMap(arguments);
  final command = _stringValue(json['command']);
  final path =
      _stringValue(json['path']) ??
      _stringValue(json['filePath']) ??
      _stringValue(json['targetPath']) ??
      _stringValue(json['workingDirectory']);
  final query = _stringValue(json['query']);
  final value = command?.isNotEmpty == true
      ? command!.split('\n').first
      : path?.isNotEmpty == true
      ? path!
      : query;
  return value == null || value.isEmpty ? null : value;
}

Map<String, Object?> _tryDecodeMap(String value) {
  if (value.trim().isEmpty) {
    return const {};
  }
  try {
    final decoded = jsonDecode(value);
    if (decoded is Map<String, Object?>) {
      return decoded;
    }
    if (decoded is Map) {
      return decoded.map((key, value) => MapEntry(key.toString(), value));
    }
  } catch (_) {
    return const {};
  }
  return const {};
}

String? _stringValue(Object? value) {
  if (value == null) {
    return null;
  }
  return value.toString();
}

class TimelineMessage {
  const TimelineMessage({
    required this.id,
    required this.sessionId,
    required this.role,
    required this.createdAt,
    this.sequence = 0,
  });

  final String id;
  final String sessionId;
  final String role;
  final DateTime createdAt;
  final int sequence;

  TimelineMessage copyWith({String? role, DateTime? createdAt, int? sequence}) {
    return TimelineMessage(
      id: id,
      sessionId: sessionId,
      role: role ?? this.role,
      createdAt: createdAt ?? this.createdAt,
      sequence: sequence ?? this.sequence,
    );
  }
}

class TimelineRow {
  const TimelineRow._({
    required this.id,
    required this.sessionId,
    required this.messageId,
    required this.role,
    required this.type,
    required this.createdAt,
    required this.sequence,
    required this.renderVersion,
    this.part,
    this.agentEvent,
  });

  factory TimelineRow.messagePart({
    required String id,
    required String sessionId,
    required String messageId,
    required String role,
    required TimelineRowType type,
    required DateTime createdAt,
    required int sequence,
    required TimelinePart part,
  }) {
    return TimelineRow._(
      id: id,
      sessionId: sessionId,
      messageId: messageId,
      role: role,
      type: type,
      createdAt: createdAt,
      sequence: sequence,
      renderVersion: _timelineRowRenderVersion(part),
      part: part,
    );
  }

  factory TimelineRow.agentActivity(TimelineAgentEvent event) {
    return TimelineRow._(
      id: 'agent-activity:${event.eventId}',
      sessionId: event.sessionId,
      messageId: null,
      role: null,
      type: TimelineRowType.agentActivity,
      createdAt: event.createdAt,
      sequence: event.sequence,
      renderVersion: _timelineAgentEventRenderVersion(event),
      agentEvent: event,
    );
  }

  final String id;
  final String sessionId;
  final String? messageId;
  final String? role;
  final TimelineRowType type;
  final DateTime createdAt;
  final int sequence;
  final int renderVersion;
  final TimelinePart? part;
  final TimelineAgentEvent? agentEvent;
}

List<TimelineRow> timelineRowsFromMessages(
  List<TimelineMessage> messages, {
  Iterable<TimelinePart> parts = const [],
  Iterable<TimelineAgentEvent> agentEvents = const [],
}) {
  final messagesById = <String, TimelineMessage>{
    for (final message in messages)
      if (message.id.isNotEmpty) message.id: message,
  };
  final sortedMessages = [...messagesById.values]..sort(_compareMessages);
  final partsByMessage = <String, List<TimelinePart>>{};
  for (final part in parts) {
    if (!messagesById.containsKey(part.messageId)) {
      continue;
    }
    partsByMessage.putIfAbsent(part.messageId, () => []).add(part);
  }
  final rows = <TimelineRow>[
    for (final message in sortedMessages)
      ..._timelineRowsForMessage(message, partsByMessage[message.id]),
    for (final event in latestTimelineAgentEvents(agentEvents))
      timelineRowFromAgentEvent(event),
  ];
  rows.sort(_compareRows);
  return rows;
}

List<TimelineRow> _timelineRowsForMessage(
  TimelineMessage message,
  List<TimelinePart>? parts,
) {
  final sortedParts = [...?parts]..sort(_compareParts);
  return [
    for (final part in sortedParts)
      TimelineRow.messagePart(
        id: '${message.id}:${part.id}',
        sessionId: message.sessionId,
        messageId: message.id,
        role: message.role,
        type: _timelineRowType(message, part),
        createdAt: message.createdAt,
        sequence: message.sequence,
        part: part,
      ),
  ];
}

int _compareMessages(TimelineMessage left, TimelineMessage right) {
  final sequence = left.sequence.compareTo(right.sequence);
  if (sequence != 0) {
    return sequence;
  }
  final createdAt = left.createdAt.compareTo(right.createdAt);
  if (createdAt != 0) {
    return createdAt;
  }
  return left.id.compareTo(right.id);
}

int _compareParts(TimelinePart left, TimelinePart right) {
  final order = left.order.compareTo(right.order);
  if (order != 0) {
    return order;
  }
  final sequence = left.sequence.compareTo(right.sequence);
  if (sequence != 0) {
    return sequence;
  }
  return left.id.compareTo(right.id);
}

int _compareRows(TimelineRow left, TimelineRow right) {
  final sequence = left.sequence.compareTo(right.sequence);
  if (sequence != 0) {
    return sequence;
  }
  final createdAt = left.createdAt.compareTo(right.createdAt);
  if (createdAt != 0) {
    return createdAt;
  }
  return left.id.compareTo(right.id);
}

int _timelineRowRenderVersion(TimelinePart part) {
  final tool = part.tool;
  final agent = part.agent;
  return Object.hashAll([
    part.revision,
    part.sequence,
    part.status,
    part.title,
    part.text,
    part.collapsed,
    part.synthetic,
    part.ignored,
    if (tool != null) ...[
      tool.name,
      tool.arguments,
      tool.result,
      tool.exitCode,
      tool.timedOut,
      tool.workingDirectory,
      tool.denialReason,
    ],
    if (agent != null) ...[
      agent.id,
      agent.path,
      agent.parentPath,
      agent.role,
      agent.task,
      agent.status,
      agent.summary,
      agent.error,
      agent.reason,
    ],
  ]);
}

TimelineRowType _timelineRowType(TimelineMessage message, TimelinePart part) {
  if (message.role == 'user') {
    return TimelineRowType.userMessage;
  }
  return switch (part.type) {
    TimelinePartType.tool => TimelineRowType.toolActivity,
    TimelinePartType.reasoning => TimelineRowType.reasoningSummary,
    TimelinePartType.plan => TimelineRowType.plan,
    TimelinePartType.agent => TimelineRowType.agentActivity,
    TimelinePartType.text => switch (part.textChannel) {
      TimelineTextChannel.commentary => TimelineRowType.commentary,
      TimelineTextChannel.user => TimelineRowType.userMessage,
      TimelineTextChannel.finalAnswer || null => TimelineRowType.finalAnswer,
    },
  };
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
    callId: _stringValue(eventKind['callId']) ?? '',
    kindType: resolvedKindType,
    title: _agentActivityTitle(resolvedKindType),
    text: _agentActivityText(eventKind),
    status: _stringValue(eventKind['status']) ?? 'completed',
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
    event.kindType,
    event.title,
    event.text,
    event.status,
    event.createdAt.millisecondsSinceEpoch,
  ]);
}

String _agentActivityTitle(String kindType) {
  return switch (kindType) {
    'spawnBegin' || 'spawnEnd' => 'agentTimeline.spawn',
    'interactionBegin' || 'interactionEnd' => 'agentTimeline.message',
    'waitingBegin' || 'waitingEnd' => 'agentTimeline.waiting',
    'closeBegin' || 'closeEnd' => 'agentTimeline.close',
    _ => 'agentTimeline.agent',
  };
}

String _agentActivityText(Map<String, Object?> kind) {
  final receiver = _stringValue(kind['receiverPath']);
  final path =
      _stringValue(kind['path']) ??
      receiver ??
      _stringValue(kind['senderPath']);
  final task = _stringValue(kind['taskName']);
  final prompt = _stringValue(kind['prompt']);
  final error = _stringValue(kind['error']);
  return [
    path,
    task,
    prompt,
    error,
  ].whereType<String>().where((part) => part.trim().isNotEmpty).join('\n');
}

String _nonEmpty(String? preferred, String? fallback) {
  if (preferred != null && preferred.isNotEmpty) {
    return preferred;
  }
  return fallback ?? '';
}

int _intValue(Object? value) {
  if (value is int) {
    return value;
  }
  if (value is BigInt) {
    return value.toInt();
  }
  return int.tryParse(_stringValue(value) ?? '') ?? 0;
}

Map<String, Object?> _objectMap(Object? value) {
  if (value is Map<String, Object?>) {
    return value;
  }
  if (value is Map) {
    return value.map((key, value) => MapEntry(key.toString(), value));
  }
  return const {};
}
