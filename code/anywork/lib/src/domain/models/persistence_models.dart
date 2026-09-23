import 'studio_state_snapshots.dart';

/// 持久化 owner 发布的单调快照。
class PersistenceStateSnapshot {
  const PersistenceStateSnapshot({required this.revision, required this.state});

  const PersistenceStateSnapshot.ready()
    : revision = 0,
      state = const ReadyPersistenceState(pendingCommits: 0);

  final int revision;
  final PersistenceState state;

  bool get needsAttention => state.needsAttention;
}

sealed class PersistenceState {
  const PersistenceState();

  int get pendingCommits;
  int? get oldestPendingRevision;
  int? get firstFailedAt => null;
  ObservedResourceError? get error => null;
  bool get needsAttention =>
      this is DegradedPersistenceState ||
      this is RecoveringPersistenceState ||
      this is BlockedPersistenceState;
}

final class ReadyPersistenceState extends PersistenceState {
  const ReadyPersistenceState({required this.pendingCommits});

  @override
  final int pendingCommits;
  @override
  int? get oldestPendingRevision => null;
}

final class FlushingPersistenceState extends PersistenceState {
  const FlushingPersistenceState({
    required this.pendingCommits,
    required this.oldestPendingRevision,
  });

  @override
  final int pendingCommits;
  @override
  final int? oldestPendingRevision;
}

final class DegradedPersistenceState extends PersistenceState {
  const DegradedPersistenceState({
    required this.pendingCommits,
    required this.oldestPendingRevision,
    required this.firstFailedAt,
    required this.error,
  });

  @override
  final int pendingCommits;
  @override
  final int? oldestPendingRevision;
  @override
  final int firstFailedAt;
  @override
  final ObservedResourceError error;
}

final class RecoveringPersistenceState extends PersistenceState {
  const RecoveringPersistenceState({
    required this.pendingCommits,
    required this.oldestPendingRevision,
    required this.firstFailedAt,
  });

  @override
  final int pendingCommits;
  @override
  final int? oldestPendingRevision;
  @override
  final int firstFailedAt;
}

final class BlockedPersistenceState extends PersistenceState {
  const BlockedPersistenceState({
    required this.pendingCommits,
    required this.oldestPendingRevision,
    required this.firstFailedAt,
    required this.error,
  });

  @override
  final int pendingCommits;
  @override
  final int? oldestPendingRevision;
  @override
  final int firstFailedAt;
  @override
  final ObservedResourceError error;
}

/// 进程级持久化队列压力与逐 Thread 水位。
///
/// 这是诊断观测，不承载任何权威会话状态：队列字节、最老待保存年龄、在途字节与最近错误
/// 都直接来自后端持久化协调器的观测值，缺失即未知（`null`），而不是本地推算的零。
class PersistenceQueueSnapshot {
  const PersistenceQueueSnapshot({
    this.statisticsGap = false,
    required this.pendingOperations,
    required this.pendingBytes,
    required this.inFlightBytes,
    this.oldestPendingAgeMillis,
    this.lastError,
    this.pressurePaused = false,
    this.threads = const [],
  });

  const PersistenceQueueSnapshot.empty()
    : statisticsGap = false,
      pendingOperations = 0,
      pendingBytes = 0,
      inFlightBytes = 0,
      oldestPendingAgeMillis = null,
      lastError = null,
      pressurePaused = false,
      threads = const [];

  /// 跨全部 Thread 的排队操作数（含一次保留的 checkpoint 发布）。
  final bool statisticsGap;

  final int pendingOperations;

  /// 排队事实的编码字节数。
  final int pendingBytes;

  /// 正在写入的批次字节数。
  final int inFlightBytes;

  /// 最老排队操作的年龄；没有排队时为空。
  final int? oldestPendingAgeMillis;

  /// 最近一次写入错误；下一次成功落库前一直保留。
  final String? lastError;

  /// 是否有 Thread 因存储压力暂停了新推理准入。
  final bool pressurePaused;

  /// 逐 Thread 水位与压力，按 Thread 身份排序。
  final List<ThreadPersistenceSnapshot> threads;

  bool get isBacklogged => pendingOperations > 0 || pendingBytes > 0;
}

/// 单个 Thread 的持久化水位与队列压力。
///
/// 水位为 `null` 表示没有 writer 报告过该值（未知），`0` 才是已观测到的零。UI 必须把两者
/// 分开显示，不把未知折叠成零。
class ThreadPersistenceSnapshot {
  const ThreadPersistenceSnapshot({
    required this.threadId,
    this.faultGeneration = 0,
    this.fault,
    this.stateDirtyRevision,
    this.stateSavingRevision,
    this.stateDurableRevision,
    this.historyAdmittedSequence,
    this.historyDurableSequence,
    this.callsAdmittedSequence,
    this.callsDurableSequence,
    this.pendingOperations = 0,
    this.pendingBytes = 0,
    this.oldestPendingAgeMillis,
    this.inFlightBytes = 0,
    this.lastError,
    this.pressurePaused = false,
  });

  final String threadId;
  final int faultGeneration;
  final String? fault;

  /// 已受理待发布的 checkpoint 修订；未知为 null。
  final int? stateDirtyRevision;

  /// 正在序列化或同步的 checkpoint 修订；未知为 null。
  final int? stateSavingRevision;

  /// 已作为 `state.toml` 发布的 checkpoint 修订；未知为 null。
  final int? stateDurableRevision;
  final int? historyAdmittedSequence;
  final int? historyDurableSequence;
  final int? callsAdmittedSequence;
  final int? callsDurableSequence;
  final int pendingOperations;
  final int pendingBytes;
  final int? oldestPendingAgeMillis;
  final int inFlightBytes;
  final String? lastError;
  final bool pressurePaused;
}
