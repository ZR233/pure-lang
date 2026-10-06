/// 应用退出结果的只读分类；canonical 事实由 native/runtime 报告承载。
///
/// `notStarted` 表示 runtime 从未安装；`clean` 表示所有阶段成功且持久化已排空；
/// 其余为 `degraded`。界面绝不把 degraded 或超时折叠成正常退出。
enum StudioShutdownOutcome { notStarted, clean, degraded }

/// 待保存事实的真实进度。
///
/// 无法确定时必须为 [UnknownStudioPendingPersistence]；绝不用 [Drained] 掩盖未知。
sealed class StudioPendingPersistence {
  const StudioPendingPersistence();
}

/// 无法确定待保存事实数量。
final class UnknownStudioPendingPersistence extends StudioPendingPersistence {
  const UnknownStudioPendingPersistence();
}

/// 尚无 durable 确认的待保存事实数量。
final class PendingStudioPendingPersistence extends StudioPendingPersistence {
  const PendingStudioPendingPersistence({required this.count});

  final int count;
}

/// 全部待保存事实已获得 durable 确认。
final class DrainedStudioPendingPersistence extends StudioPendingPersistence {
  const DrainedStudioPendingPersistence();
}

/// 关闭阶段里一个真实的 typed 失败原因（诊断边界，不含凭据或正文）。
class StudioShutdownIssue {
  const StudioShutdownIssue({
    required this.stage,
    required this.code,
    required this.message,
    required this.retryable,
    required this.correlationId,
  });

  /// 已知阶段的稳定诊断标签。
  final String stage;

  /// typed 失败原因。
  final String code;

  /// 脱敏后的可读信息。
  final String message;

  /// 是否可在剩余期限内重试。
  final bool retryable;

  /// 关联同步日志与诊断的编号。
  final String correlationId;
}

/// 一次应用退出的 typed 报告（冻结的最低接口形状，见 design/18 §18.5.1）。
class StudioShutdownReport {
  const StudioShutdownReport({
    required this.outcome,
    required this.issues,
    required this.persistence,
  });

  final StudioShutdownOutcome outcome;
  final List<StudioShutdownIssue> issues;
  final StudioPendingPersistence persistence;

  bool get isClean => outcome == StudioShutdownOutcome.clean;
  bool get isNotStarted => outcome == StudioShutdownOutcome.notStarted;

  /// 是否允许以正常退出码结束：Clean，或未安装且没有任何问题。
  bool get allowsCleanExit => isClean || (isNotStarted && issues.isEmpty);

  /// runtime 从未安装、没有待保存事实的默认报告。
  static const notStarted = StudioShutdownReport(
    outcome: StudioShutdownOutcome.notStarted,
    issues: [],
    persistence: UnknownStudioPendingPersistence(),
  );
}
