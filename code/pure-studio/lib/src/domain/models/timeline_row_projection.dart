part of 'timeline_models.dart';

class TimelineRow {
  const TimelineRow._({
    required this.id,
    required this.threadId,
    required this.type,
    required this.createdAt,
    required this.order,
    required this.sequence,
    required this.renderVersion,
    this.turnId,
    this.part,
    this.toolGroup,
    this.reasoningGroup,
    this.agentEvent,
    this.isRolledBack = false,
  });

  factory TimelineRow.item({
    required ThreadItemView item,
    required TimelineEntry part,
    required TimelineRowType type,
  }) {
    return TimelineRow._(
      id: item.id,
      threadId: item.threadId,
      type: type,
      createdAt: item.createdAt,
      order: item.ordinal,
      sequence: item.ordinal,
      renderVersion: _timelineRowRenderVersion(part),
      turnId: item.turnId,
      part: part,
      isRolledBack:
          part.contextDisposition == ThreadContextDisposition.rolledBack,
    );
  }

  factory TimelineRow.agentActivity(TimelineAgentEvent event) {
    return TimelineRow._(
      id: 'agent-activity:${timelineAgentEventGroupKey(event)}',
      threadId: event.threadId,
      type: TimelineRowType.agentActivity,
      createdAt: event.createdAt,
      order: 0,
      sequence: event.sequence,
      renderVersion: _timelineAgentEventRenderVersion(event),
      agentEvent: event,
    );
  }

  factory TimelineRow.toolGroup(TimelineToolGroup group) {
    return TimelineRow._(
      id: group.id,
      threadId: group.threadId,
      type: TimelineRowType.toolGroup,
      createdAt: group.createdAt ?? DateTime.fromMillisecondsSinceEpoch(0),
      order: group.order,
      sequence: group.sequence,
      renderVersion: group.renderVersion,
      turnId: group.turnId,
      toolGroup: group,
      isRolledBack: group.isRolledBack,
    );
  }

  factory TimelineRow.reasoningGroup(TimelineReasoningGroup group) {
    return TimelineRow._(
      id: group.id,
      threadId: group.threadId,
      type: TimelineRowType.reasoningSummary,
      createdAt: group.createdAt ?? DateTime.fromMillisecondsSinceEpoch(0),
      order: group.order,
      sequence: group.sequence,
      renderVersion: group.renderVersion,
      turnId: group.turnId,
      reasoningGroup: group,
      isRolledBack: group.isRolledBack,
    );
  }

  final String id;
  final String threadId;
  final TimelineRowType type;
  final DateTime createdAt;
  final int order;
  final int sequence;
  final int renderVersion;
  final String? turnId;
  final TimelineEntry? part;
  final TimelineToolGroup? toolGroup;
  final TimelineReasoningGroup? reasoningGroup;
  final TimelineAgentEvent? agentEvent;
  final bool isRolledBack;
}

List<TimelineRow> timelineRowsFromThreadItems(List<ThreadItemView> source) {
  final items = [...source]
    ..sort((left, right) {
      final ordinal = left.ordinal.compareTo(right.ordinal);
      return ordinal != 0 ? ordinal : left.id.compareTo(right.id);
    });
  final rows = <TimelineRow>[];
  final adjacentTools = <TimelineEntry>[];
  final adjacentReasoning = <TimelineEntry>[];

  void flushTools() {
    if (adjacentTools.isEmpty) return;
    final first = adjacentTools.first;
    rows.add(
      TimelineRow.toolGroup(
        TimelineToolGroup(
          id: 'tool-group:${first.turnId}:${first.id}',
          threadId: first.threadId,
          groupId: first.turnId,
          turnId: first.turnId,
          items: [
            for (final part in adjacentTools) TimelineToolGroupItem(part: part),
          ],
        ),
      ),
    );
    adjacentTools.clear();
  }

  void flushReasoning() {
    if (adjacentReasoning.isEmpty) return;
    final first = adjacentReasoning.first;
    rows.add(
      TimelineRow.reasoningGroup(
        TimelineReasoningGroup(
          id: 'reasoning-group:${first.turnId}:${first.id}',
          threadId: first.threadId,
          groupId: first.turnId,
          turnId: first.turnId,
          parts: [...adjacentReasoning],
        ),
      ),
    );
    adjacentReasoning.clear();
  }

  for (final item in items) {
    if (const {
      ThreadItemKind.file,
      ThreadItemKind.turn,
      ThreadItemKind.inference,
      ThreadItemKind.contextCompaction,
    }.contains(item.kind)) {
      continue;
    }
    final part = _timelineEntryFromThreadItem(item);
    if (item.kind == ThreadItemKind.toolCall) {
      flushReasoning();
      if (adjacentTools.isNotEmpty &&
          adjacentTools.last.turnId != item.turnId) {
        flushTools();
      }
      adjacentTools.add(part);
      continue;
    }
    if (item.kind == ThreadItemKind.reasoning) {
      flushTools();
      if (adjacentReasoning.isNotEmpty &&
          adjacentReasoning.last.turnId != item.turnId) {
        flushReasoning();
      }
      adjacentReasoning.add(part);
      continue;
    }
    flushTools();
    flushReasoning();
    rows.add(
      TimelineRow.item(
        item: item,
        part: part,
        type: switch (item.kind) {
          ThreadItemKind.userMessage => TimelineRowType.userMessage,
          ThreadItemKind.agentMessage =>
            item.channel == AgentMessageChannel.commentary
                ? TimelineRowType.commentary
                : TimelineRowType.finalAnswer,
          ThreadItemKind.plan => TimelineRowType.plan,
          ThreadItemKind.skill => TimelineRowType.skillActivation,
          ThreadItemKind.reasoning => TimelineRowType.reasoningSummary,
          ThreadItemKind.toolCall => TimelineRowType.toolGroup,
          ThreadItemKind.agent => TimelineRowType.agentActivity,
          ThreadItemKind.turn ||
          ThreadItemKind.inference ||
          ThreadItemKind.file ||
          ThreadItemKind.contextCompaction => TimelineRowType.finalAnswer,
        },
      ),
    );
  }
  flushTools();
  flushReasoning();
  rows.sort(_compareRows);
  return rows;
}

TimelineEntry _timelineEntryFromThreadItem(ThreadItemView item) {
  final text = switch (item.kind) {
    ThreadItemKind.reasoning => [
      ...item.reasoningSummary,
      ...item.reasoningContent,
    ].where((value) => value.trim().isNotEmpty).join('\n\n'),
    ThreadItemKind.toolCall => _toolActivityText(item.tool),
    _ => item.text,
  };
  return TimelineEntry(
    id: item.id,
    groupId: item.turnId,
    threadId: item.threadId,
    turnId: item.turnId,
    type: switch (item.kind) {
      ThreadItemKind.userMessage ||
      ThreadItemKind.agentMessage => TimelineEntryType.text,
      ThreadItemKind.reasoning => TimelineEntryType.reasoning,
      ThreadItemKind.plan => TimelineEntryType.plan,
      ThreadItemKind.skill => TimelineEntryType.skill,
      ThreadItemKind.toolCall => TimelineEntryType.tool,
      ThreadItemKind.agent => TimelineEntryType.text,
      ThreadItemKind.turn ||
      ThreadItemKind.inference ||
      ThreadItemKind.file ||
      ThreadItemKind.contextCompaction => TimelineEntryType.file,
    },
    order: item.ordinal,
    sequence: item.ordinal,
    revision: item.revision,
    createdAt: item.createdAt,
    updatedAt: item.updatedAt,
    completedAt: item.completedAt,
    error: item.error,
    text: text,
    reasoningSummary: item.reasoningSummary,
    reasoningContent: item.reasoningContent,
    status: item.status,
    textChannel: switch (item.kind) {
      ThreadItemKind.userMessage => TimelineTextChannel.user,
      ThreadItemKind.agentMessage =>
        item.channel == AgentMessageChannel.commentary
            ? TimelineTextChannel.commentary
            : TimelineTextChannel.finalAnswer,
      _ => null,
    },
    tool: item.tool,
    planContent: item.kind == ThreadItemKind.plan ? item.text : null,
    skill: switch (item.skill) {
      final skill? => TimelineSkillActivation(
        name: skill.name,
        source: skill.source,
        providerId: skill.providerId,
        resourceBase: skill.resourceBase,
        cause: skill.cause,
        activatedAt: skill.activatedAt,
      ),
      null => null,
    },
    attachments: item.attachments,
    contextDisposition: item.contextDisposition,
  );
}

int _compareRows(TimelineRow left, TimelineRow right) {
  final sequence = left.sequence.compareTo(right.sequence);
  if (sequence != 0) return sequence;
  final order = left.order.compareTo(right.order);
  if (order != 0) return order;
  return left.id.compareTo(right.id);
}

int _timelineRowRenderVersion(TimelineEntry part) {
  final tool = part.tool;
  return Object.hashAll([
    part.id,
    part.revision,
    part.status,
    part.text,
    ...part.reasoningSummary,
    ...part.reasoningContent,
    part.planContent,
    part.skill?.name,
    part.skill?.source,
    part.skill?.providerId,
    part.skill?.resourceBase.kind,
    part.skill?.resourceBase.value,
    part.skill?.cause.kind,
    part.skill?.cause.id,
    part.contextDisposition,
    part.updatedAt?.millisecondsSinceEpoch,
    part.error,
    tool?.arguments,
    tool?.result,
    tool?.exitCode,
    tool?.timedOut,
    tool?.denialReason,
  ]);
}

bool _isActiveToolStatus(String status) {
  return const {'started', 'streaming', 'approved', 'running'}.contains(status);
}

bool _isIssueToolStatus(String status) {
  return const {'failed', 'denied', 'cancelled'}.contains(status);
}
