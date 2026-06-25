import 'studio_enums.dart';

enum TimelineTextChannel { user, commentary, finalAnswer }

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

class TimelinePartSnapshot {
  const TimelinePartSnapshot({
    required this.id,
    required this.messageId,
    required this.sessionId,
    required this.turnId,
    required this.type,
    required this.order,
    required this.revision,
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

class TimelineMessage {
  const TimelineMessage({
    required this.id,
    required this.sessionId,
    required this.role,
    required this.createdAt,
    required this.parts,
  });

  final String id;
  final String sessionId;
  final String role;
  final DateTime createdAt;
  final List<TimelinePart> parts;

  TimelineMessage copyWith({List<TimelinePart>? parts}) {
    return TimelineMessage(
      id: id,
      sessionId: sessionId,
      role: role,
      createdAt: createdAt,
      parts: parts ?? this.parts,
    );
  }
}
