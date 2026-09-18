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

/// 根会话工作区模式 canonical 值。
///
/// `local` 使用 Project 目录，`worktree` 从 Project 的 Git 仓库 `HEAD` 新建独立工作树。
/// 它是创建会话时确定的 Thread 产品事实，GUI 只读消费，不推导也不本地改写。
enum ThreadWorkspaceMode {
  local,
  worktree;

  /// Canonical wire 值与存储列一致：`local` | `worktree`。
  String get id => name;

  bool get isWorktree => this == ThreadWorkspaceMode.worktree;

  /// 解析 canonical 工作区模式；未知值按 `local` 显示，不构造第二份状态。
  static ThreadWorkspaceMode fromId(String id) => switch (id.trim()) {
    'worktree' => worktree,
    _ => local,
  };
}
