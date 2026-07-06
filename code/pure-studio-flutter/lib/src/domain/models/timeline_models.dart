import 'dart:convert';

import 'studio_enums.dart';

part 'timeline_agent_events.dart';
part 'timeline_row_projection.dart';

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

List<Object?> _listValue(Object? value) {
  if (value is List<Object?>) {
    return value;
  }
  if (value is Iterable) {
    return value.cast<Object?>().toList(growable: false);
  }
  return const [];
}
