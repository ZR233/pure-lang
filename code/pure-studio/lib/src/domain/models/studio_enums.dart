enum PermissionMode { requestApproval, autoReview, fullAccess }

/// 完整 Mode Skill ID；内置与自定义模式使用同一种身份。
final class StudioMode {
  const StudioMode(this.id);

  static const simple = StudioMode('mode.simple');
  static const task = StudioMode('mode.task');
  static const values = [simple, task];

  final String id;

  /// 供稳定 ValueKey 使用；完整 ID 本身就是模式身份。
  String get name => id;

  factory StudioMode.fromId(String id) => switch (id) {
    'mode.simple' => simple,
    'mode.task' => task,
    _ => StudioMode(id),
  };

  @override
  bool operator ==(Object other) => other is StudioMode && other.id == id;

  @override
  int get hashCode => id.hashCode;

  @override
  String toString() => id;
}

enum ThreadContextDisposition { active, rolledBack }

enum TimelineEntryType { text, reasoning, tool, skill, file }

enum InteractionKind { toolApproval, userInput }
