import 'dart:convert';

import 'studio_enums.dart';
import 'thread_models.dart';

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
    this.outputArtifacts = const [],
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
  final List<Object?> outputArtifacts;
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
      outputArtifacts: outputArtifacts,
      exitCode: exitCode,
      timedOut: timedOut,
      workingDirectory: workingDirectory,
      denialReason: denialReason,
    );
  }
}

class TimelineToolGroupItem {
  const TimelineToolGroupItem({required this.part});

  final TimelineEntry part;

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
    required this.threadId,
    required this.groupId,
    required this.turnId,
    required this.items,
  });

  final String id;
  final String threadId;
  final String groupId;
  final String turnId;
  final List<TimelineToolGroupItem> items;

  int get count => items.length;

  bool get isRolledBack =>
      items.isNotEmpty &&
      items.every(
        (item) =>
            item.part.contextDisposition == ThreadContextDisposition.rolledBack,
      );

  int get runningCount =>
      items.where((item) => _isActiveToolStatus(item.status)).length;

  int get issueCount =>
      items.where((item) => _isIssueToolStatus(item.status)).length;

  String? get firstIssueReason {
    for (final item in items.where((item) => _isIssueToolStatus(item.status))) {
      final candidates = [
        item.part.error,
        item.tool?.denialReason,
        item.tool?.result,
      ];
      for (final candidate in candidates) {
        final normalized = candidate?.trim().replaceAll(RegExp(r'\s+'), ' ');
        if (normalized != null && normalized.isNotEmpty) {
          return normalized.length <= 120
              ? normalized
              : '${normalized.substring(0, 117)}...';
        }
      }
    }
    return null;
  }

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
    threadId,
    groupId,
    turnId,
    status,
    count,
    runningCount,
    issueCount,
    for (final item in items) _timelineRowRenderVersion(item.part),
  ]);
}

class TimelineReasoningGroup {
  const TimelineReasoningGroup({
    required this.id,
    required this.threadId,
    required this.groupId,
    required this.turnId,
    required this.parts,
  });

  final String id;
  final String threadId;
  final String groupId;
  final String turnId;
  final List<TimelineEntry> parts;

  int get count => parts.length;

  bool get isRolledBack =>
      parts.isNotEmpty &&
      parts.every(
        (part) =>
            part.contextDisposition == ThreadContextDisposition.rolledBack,
      );

  int get order => parts.isEmpty ? 0 : parts.first.order;

  int get sequence => parts.fold(0, (value, part) {
    return part.sequence > value ? part.sequence : value;
  });

  DateTime? get createdAt => parts.isEmpty ? null : parts.first.createdAt;

  bool get isActive =>
      parts.any((part) => !isTerminalTimelineStatus(part.status));

  String get status {
    for (final part in parts.reversed) {
      if (!isTerminalTimelineStatus(part.status)) {
        return part.status;
      }
    }
    for (final status in const [
      'failed',
      'interrupted',
      'cancelled',
      'denied',
      'budgetLimited',
      'completed',
    ]) {
      if (parts.any((part) => part.status == status)) {
        return status;
      }
    }
    return parts.isEmpty ? 'completed' : parts.last.status;
  }

  List<String> get summaries => parts
      .map(_reasoningPartSummary)
      .whereType<String>()
      .toList(growable: false);

  String? get latestSummary {
    for (final part in parts.reversed) {
      final summary = _reasoningPartSummary(part);
      if (summary != null) {
        return summary;
      }
    }
    return null;
  }

  String get details => parts
      .map((part) {
        final raw = part.reasoningContent
            .where((value) => value.trim().isNotEmpty)
            .join('\n\n');
        return raw.isNotEmpty ? raw : part.text.trim();
      })
      .where((text) => text.isNotEmpty)
      .join('\n\n');

  int get renderVersion => Object.hashAll([
    id,
    threadId,
    groupId,
    turnId,
    status,
    count,
    for (final part in parts) _timelineRowRenderVersion(part),
  ]);
}

class TimelineEntry {
  const TimelineEntry({
    required this.id,
    required this.groupId,
    required this.type,
    required this.text,
    this.reasoningSummary = const [],
    this.reasoningContent = const [],
    this.threadId = '',
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
    this.tool,
    this.planContent,
    this.contextDisposition = ThreadContextDisposition.active,
  });

  final String id;
  final String groupId;
  final String threadId;
  final String turnId;
  final TimelineEntryType type;
  final int order;
  final int sequence;
  final String text;
  final List<String> reasoningSummary;
  final List<String> reasoningContent;
  final String? title;
  final String status;
  final int revision;
  final DateTime? createdAt;
  final DateTime? updatedAt;
  final DateTime? completedAt;
  final String? error;
  final TimelineTextChannel? textChannel;
  final TimelineToolPart? tool;
  final String? planContent;
  final ThreadContextDisposition contextDisposition;
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

String? _reasoningPartSummary(TimelineEntry part) {
  final title = _plainReasoningSummary(part.title ?? '');
  if (title != null) {
    return title;
  }
  final lines = part.text.replaceAll('\r\n', '\n').split('\n');
  for (final line in lines) {
    final summary = _plainReasoningSummary(line);
    if (summary != null) {
      return summary;
    }
  }
  return null;
}

String? _plainReasoningSummary(String value) {
  var summary = value.trim();
  if (summary.isEmpty || summary == '<!-- -->') {
    return null;
  }
  if ((summary.startsWith('**') && summary.endsWith('**')) ||
      (summary.startsWith('__') && summary.endsWith('__'))) {
    summary = summary.substring(2, summary.length - 2);
  }
  summary = summary
      .replaceFirst(RegExp(r'^#{1,6}\s+'), '')
      .replaceFirst(RegExp(r'^>\s*'), '')
      .replaceFirst(RegExp(r'^(?:[-+*]|\d+[.)])\s+'), '')
      .replaceAll(RegExp(r'[*_`]'), '')
      .replaceAll(RegExp(r'\s+'), ' ')
      .trim();
  return summary.isEmpty ? null : summary;
}

bool isTerminalTimelineStatus(String status) {
  return const {
    'completed',
    'failed',
    'interrupted',
    'cancelled',
    'denied',
    'budgetLimited',
  }.contains(status);
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
