import 'studio_enums.dart';

class TimelinePart {
  const TimelinePart({
    required this.id,
    required this.messageId,
    required this.type,
    required this.text,
    this.title,
    this.status = 'completed',
    this.collapsed = false,
  });

  final String id;
  final String messageId;
  final TimelinePartType type;
  final String text;
  final String? title;
  final String status;
  final bool collapsed;

  TimelinePart copyWith({String? text, String? status, bool? collapsed}) {
    return TimelinePart(
      id: id,
      messageId: messageId,
      type: type,
      text: text ?? this.text,
      title: title,
      status: status ?? this.status,
      collapsed: collapsed ?? this.collapsed,
    );
  }
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
