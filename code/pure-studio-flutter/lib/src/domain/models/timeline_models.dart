import 'dart:convert';

import 'studio_enums.dart';

enum TimelineTextChannel { user, commentary, finalAnswer }

enum TimelineRowType {
  userMessage,
  commentary,
  toolGroup,
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

class TimelineToolGroupItem {
  const TimelineToolGroupItem({required this.part});

  final TimelinePart part;

  TimelineToolPart? get tool => part.tool;

  String get id => part.id;

  String get name => tool?.name ?? part.title ?? 'Tool';

  String get status => part.status;

  String get summary => _commandSummary(tool?.arguments ?? '') ?? '';

  String get details => part.text;
}

class TimelineToolGroup {
  const TimelineToolGroup({
    required this.id,
    required this.sessionId,
    required this.messageId,
    required this.turnId,
    required this.items,
  });

  final String id;
  final String sessionId;
  final String messageId;
  final String turnId;
  final List<TimelineToolGroupItem> items;

  int get count => items.length;

  int get runningCount =>
      items.where((item) => _isActiveToolStatus(item.status)).length;

  int get issueCount =>
      items.where((item) => _isIssueToolStatus(item.status)).length;

  String get status {
    final statuses = items.map((item) => item.status).toSet();
    if (statuses.contains('awaitingApproval')) {
      return 'awaitingApproval';
    }
    if (statuses.any(_isActiveToolStatus)) {
      return 'running';
    }
    for (final status in const [
      'failed',
      'denied',
      'interrupted',
      'budgetLimited',
      'completed',
    ]) {
      if (statuses.contains(status)) {
        return status;
      }
    }
    return statuses.isEmpty ? 'completed' : statuses.first;
  }

  int get order => items.isEmpty ? 0 : items.first.part.order;

  int get sequence => items.fold(0, (value, item) {
    final sequence = item.part.sequence;
    return sequence > value ? sequence : value;
  });

  DateTime? get createdAt => items.isEmpty ? null : items.first.part.createdAt;

  int get renderVersion => Object.hashAll([
    id,
    sessionId,
    messageId,
    turnId,
    status,
    count,
    runningCount,
    issueCount,
    for (final item in items) _timelineRowRenderVersion(item.part),
  ]);
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
    this.activityGroupId,
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
  final String? activityGroupId;
  final TimelineToolPart? tool;
  final TimelineAgentPart? agent;
  final String? planContent;
  final bool synthetic;
  final bool ignored;
}

class TimelinePartDelta {
  const TimelinePartDelta({
    required this.partId,
    required this.revision,
    required this.field,
    required this.delta,
    this.chunkIndex,
  });

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
    this.sessionId = '',
    this.turnId = '',
    this.order = 0,
    this.sequence = 0,
    this.status = 'completed',
    this.revision = 0,
    this.createdAt,
    this.updatedAt,
    this.completedAt,
    this.error,
    this.title,
    this.textChannel,
    this.activityGroupId,
    this.tool,
    this.agent,
    this.planContent,
    this.collapsed = false,
    this.synthetic = false,
    this.ignored = false,
  });

  final String id;
  final String messageId;
  final String sessionId;
  final String turnId;
  final TimelinePartType type;
  final int order;
  final int sequence;
  final String text;
  final String? title;
  final String status;
  final int revision;
  final DateTime? createdAt;
  final DateTime? updatedAt;
  final DateTime? completedAt;
  final String? error;
  final TimelineTextChannel? textChannel;
  final String? activityGroupId;
  final TimelineToolPart? tool;
  final TimelineAgentPart? agent;
  final String? planContent;
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
    TimelinePartType.plan =>
      planContent?.isNotEmpty == true ? planContent! : text,
    TimelinePartType.tool => _toolActivityText(tool),
    TimelinePartType.agent =>
      snapshot.agent?.summary ?? snapshot.agent?.task ?? text,
    TimelinePartType.reasoning => text,
    TimelinePartType.text => text,
    TimelinePartType.turn ||
    TimelinePartType.inference ||
    TimelinePartType.file => '',
  };
  return TimelinePart(
    id: snapshot.id,
    messageId: snapshot.messageId,
    sessionId: snapshot.sessionId,
    turnId: snapshot.turnId,
    type: snapshot.type,
    order: snapshot.order,
    sequence: snapshot.sequence,
    revision: snapshot.revision,
    createdAt: snapshot.createdAt,
    updatedAt: snapshot.updatedAt,
    completedAt: snapshot.completedAt,
    error: snapshot.error,
    title: _partTitleFromSnapshot(snapshot),
    text: visibleText,
    status: snapshot.status,
    textChannel: snapshot.textChannel,
    activityGroupId: snapshot.activityGroupId,
    tool: tool,
    agent: snapshot.agent,
    planContent: planContent,
    collapsed: snapshot.type == TimelinePartType.reasoning,
    synthetic: snapshot.synthetic,
    ignored: snapshot.ignored,
  );
}

String _partTitleFromSnapshot(TimelinePartSnapshot snapshot) {
  return switch (snapshot.type) {
    TimelinePartType.tool => snapshot.tool?.name ?? 'Tool',
    TimelinePartType.plan => '',
    TimelinePartType.agent => snapshot.agent?.role ?? 'Agent',
    TimelinePartType.reasoning => '',
    TimelinePartType.text => '',
    TimelinePartType.turn => 'Turn',
    TimelinePartType.inference => 'Inference',
    TimelinePartType.file => 'File',
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
    this.turnId = '',
    this.status = 'completed',
    DateTime? updatedAt,
    this.completedAt,
    this.error,
    this.sequence = 0,
  }) : updatedAt = updatedAt ?? createdAt;

  final String id;
  final String sessionId;
  final String turnId;
  final String role;
  final String status;
  final DateTime createdAt;
  final DateTime updatedAt;
  final DateTime? completedAt;
  final String? error;
  final int sequence;

  TimelineMessage copyWith({
    String? turnId,
    String? role,
    String? status,
    DateTime? createdAt,
    DateTime? updatedAt,
    DateTime? completedAt,
    String? error,
    int? sequence,
  }) {
    return TimelineMessage(
      id: id,
      sessionId: sessionId,
      turnId: turnId ?? this.turnId,
      role: role ?? this.role,
      status: status ?? this.status,
      createdAt: createdAt ?? this.createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      completedAt: completedAt ?? this.completedAt,
      error: error ?? this.error,
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
    required this.order,
    required this.sequence,
    required this.renderVersion,
    this.turnId,
    this.part,
    this.toolGroup,
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
      order: part.order,
      sequence: sequence,
      renderVersion: _timelineRowRenderVersion(part),
      turnId: part.turnId,
      part: part,
    );
  }

  factory TimelineRow.agentActivity(TimelineAgentEvent event) {
    return TimelineRow._(
      id: 'agent-activity:${timelineAgentEventGroupKey(event)}',
      sessionId: event.sessionId,
      messageId: null,
      role: null,
      type: TimelineRowType.agentActivity,
      createdAt: event.createdAt,
      order: 0,
      sequence: event.sequence,
      renderVersion: _timelineAgentEventRenderVersion(event),
      agentEvent: event,
    );
  }

  factory TimelineRow.toolGroup({
    required TimelineMessage message,
    required TimelineToolGroup group,
  }) {
    return TimelineRow._(
      id: group.id,
      sessionId: group.sessionId,
      messageId: message.id,
      role: message.role,
      type: TimelineRowType.toolGroup,
      createdAt: group.createdAt ?? message.createdAt,
      order: group.order,
      sequence: group.sequence == 0 ? message.sequence : group.sequence,
      renderVersion: group.renderVersion,
      turnId: group.turnId,
      toolGroup: group,
    );
  }

  final String id;
  final String sessionId;
  final String? messageId;
  final String? role;
  final TimelineRowType type;
  final DateTime createdAt;
  final int order;
  final int sequence;
  final int renderVersion;
  final String? turnId;
  final TimelinePart? part;
  final TimelineToolGroup? toolGroup;
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
    if (!messagesById.containsKey(part.messageId) ||
        part.ignored ||
        isInternalTimelinePartType(part.type) ||
        _isLowSignalRuntimeToolProgress(part)) {
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
  final rows = <TimelineRow>[];
  final toolGroupsById = _toolGroupsForMessage(message, sortedParts);
  final insertedToolGroups = <String>{};

  for (final part in sortedParts) {
    if (message.role != 'user' && part.type == TimelinePartType.tool) {
      final groupId = _toolGroupId(message, part);
      final toolGroup = toolGroupsById[groupId];
      if (toolGroup != null && insertedToolGroups.add(groupId)) {
        rows.add(TimelineRow.toolGroup(message: message, group: toolGroup));
      }
      continue;
    }
    rows.add(
      TimelineRow.messagePart(
        id: '${message.id}:${part.id}',
        sessionId: message.sessionId,
        messageId: message.id,
        role: message.role,
        type: _timelineRowType(message, part),
        createdAt: part.createdAt ?? message.createdAt,
        sequence: part.sequence == 0 ? message.sequence : part.sequence,
        part: part,
      ),
    );
  }
  return rows;
}

Map<String, TimelineToolGroup> _toolGroupsForMessage(
  TimelineMessage message,
  List<TimelinePart> sortedParts,
) {
  if (message.role == 'user') {
    return const {};
  }
  final partsByGroupId = <String, List<TimelinePart>>{};
  for (final part in sortedParts) {
    if (part.type != TimelinePartType.tool) {
      continue;
    }
    partsByGroupId.putIfAbsent(_toolGroupId(message, part), () => []).add(part);
  }
  return {
    for (final entry in partsByGroupId.entries)
      entry.key: TimelineToolGroup(
        id: entry.key,
        sessionId: message.sessionId,
        messageId: message.id,
        turnId: _toolGroupTurnId(message, entry.value.first),
        items: entry.value
            .map((part) => TimelineToolGroupItem(part: part))
            .toList(growable: false),
      ),
  };
}

String _toolGroupId(TimelineMessage message, TimelinePart part) {
  final activityGroupId = part.activityGroupId;
  if (activityGroupId != null && activityGroupId.isNotEmpty) {
    return activityGroupId;
  }
  return 'tool-group:${message.sessionId}:${message.id}:${part.id}';
}

String _toolGroupTurnId(TimelineMessage message, TimelinePart part) {
  return part.turnId.isEmpty ? message.turnId : part.turnId;
}

bool _isLowSignalRuntimeToolProgress(TimelinePart part) {
  if (part.type != TimelinePartType.text ||
      !part.synthetic ||
      part.textChannel != TimelineTextChannel.commentary) {
    return false;
  }
  final text = part.text.trim();
  if (RegExp(r'^模型请求调用 \d+ 个工具。$').hasMatch(text)) {
    return true;
  }
  if (RegExp(r'^正在执行工具 `[^`]+`。$').hasMatch(text)) {
    return true;
  }
  if (RegExp(r'^工具 `[^`]+` 已完成。$').hasMatch(text)) {
    return true;
  }
  return text == '工具执行完成，准备回写结果。' || text == '工具结果已写入上下文，准备继续调用模型。';
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
  if (left.messageId != null && left.messageId == right.messageId) {
    final order = left.order.compareTo(right.order);
    if (order != 0) {
      return order;
    }
  }
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
    part.sessionId,
    part.turnId,
    part.order,
    part.revision,
    part.sequence,
    part.status,
    part.textChannel,
    part.title,
    part.text,
    part.planContent,
    part.createdAt?.millisecondsSinceEpoch,
    part.updatedAt?.millisecondsSinceEpoch,
    part.completedAt?.millisecondsSinceEpoch,
    part.error,
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
      agent.depth,
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
    TimelinePartType.tool => TimelineRowType.toolGroup,
    TimelinePartType.reasoning => TimelineRowType.reasoningSummary,
    TimelinePartType.plan => TimelineRowType.plan,
    TimelinePartType.agent => TimelineRowType.agentActivity,
    TimelinePartType.text => switch (part.textChannel) {
      TimelineTextChannel.commentary => TimelineRowType.commentary,
      TimelineTextChannel.user => TimelineRowType.userMessage,
      TimelineTextChannel.finalAnswer || null => TimelineRowType.finalAnswer,
    },
    TimelinePartType.turn ||
    TimelinePartType.inference ||
    TimelinePartType.file => throw StateError(
      'Internal timeline part type cannot be projected: ${part.type.name}',
    ),
  };
}

bool _isActiveToolStatus(String status) {
  return const {'started', 'streaming', 'approved', 'running'}.contains(status);
}

bool _isIssueToolStatus(String status) {
  return const {
    'failed',
    'denied',
    'interrupted',
    'budgetLimited',
  }.contains(status);
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

bool _boolValue(Object? value) {
  if (value is bool) {
    return value;
  }
  return value.toString() == 'true';
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
