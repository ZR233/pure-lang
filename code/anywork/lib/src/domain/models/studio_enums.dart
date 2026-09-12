enum PermissionMode { requestApproval, autoReview, fullAccess }

/// 完整 Thread Mode ID；内置与外部注册使用同一种 wire 身份。
final class ThreadModeId {
  const ThreadModeId(this.id);

  static const simple = ThreadModeId('mode.simple');
  static const task = ThreadModeId('mode.task');
  static const values = [simple, task];

  final String id;

  /// 供稳定 ValueKey 使用；完整 ID 本身就是模式身份。
  String get name => id;

  factory ThreadModeId.fromId(String id) => switch (id) {
    'mode.simple' => simple,
    'mode.task' => task,
    _ => ThreadModeId(id),
  };

  @override
  bool operator ==(Object other) => other is ThreadModeId && other.id == id;

  @override
  int get hashCode => id.hashCode;

  @override
  String toString() => id;
}

enum ThreadContextDisposition { active, rolledBack }

enum TimelineEntryType { text, reasoning, tool, skill, file }

enum InteractionKind { toolApproval, userInput }
