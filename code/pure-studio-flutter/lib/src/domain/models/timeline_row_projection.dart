part of 'timeline_models.dart';

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
      if (event.payload is! TimelineTodoListUpdate)
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
