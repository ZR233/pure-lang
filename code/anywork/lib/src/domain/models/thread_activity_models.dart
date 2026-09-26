/// 当前执行活动的 typed 事实（后端投影的 Dart 映射）。
///
/// 与 Timeline 窗口无关：活动只描述“此刻在做什么”，其身份、版本、摘要与活跃工具
/// 都来自后端 typed 活动投影（快照或 `ActivityChanged` 通知），完整 reasoning /
/// 输出 / 工具参数按需由 `read_thread_activity_detail` 读取。这里不含正文，也不做
/// 任何窗口内搜索或 SQL 回读。
enum ThreadActivityKind {
  /// Turn 运行中但还没有运行中的 attempt（正在准备）。
  preparing,

  /// 已提交模型请求但尚无任何流式事实（等待 API）。
  waitingApi,

  /// 已开始产出 reasoning。
  thinking,

  /// 正在产出正文。
  responding,

  /// 规划中。
  planning,

  /// 工具正在运行（可能并行多条）。
  runningTool,

  /// 运行中任务存在待审批的权限请求。
  awaitingApproval,

  /// 存在等待用户回答的交互。
  awaitingInput,

  /// 正在打断当前 Turn。
  stopping,
}

/// 活跃工具参数事实的可用程度；参数不完整或无法提取时摘要回落工具名称。
enum ThreadActivityArgumentsKind { commandLine, opaque, streaming, unavailable }

/// 一条活跃工具调用的执行状态；活动条目不会是 [finished]。
enum ThreadActivityToolState { running, awaitingApproval, cancelling, finished }

/// 活动里的活跃工具条目；`summary` 是命令行摘要或工具名称。
class ThreadActivityToolEntry {
  const ThreadActivityToolEntry({
    required this.callId,
    required this.name,
    required this.summary,
    required this.arguments,
    required this.state,
    this.taskId,
    this.ordinal,
    this.startedAt,
  });

  final String callId;
  final String? taskId;
  final String name;
  final String summary;
  final ThreadActivityArgumentsKind arguments;
  final ThreadActivityToolState state;
  final int? ordinal;
  final int? startedAt;

  /// 单行工具预览：命令行摘要优先，缺完整参数时为工具名称。
  String get singleLineLabel {
    final trimmed = summary.trim();
    return trimmed.isEmpty ? name : trimmed;
  }
}

class ThreadActivityTools {
  const ThreadActivityTools({
    this.count = 0,
    this.background = 0,
    this.active = const [],
    this.latestStarted,
  });

  /// 活跃工具数量（= [active] 的长度）。
  final int count;

  /// 其中处于后台的活跃工具数量；前台就是工具本身时为 0。
  final int background;
  final List<ThreadActivityToolEntry> active;

  /// 最近启动的活跃工具；没有活跃工具时为空。
  final ThreadActivityToolEntry? latestStarted;

  static const empty = ThreadActivityTools();
}

/// 当前执行活动的小型 typed 摘要；不携带正文。
class ThreadActivityView {
  const ThreadActivityView({
    required this.threadId,
    required this.identity,
    required this.revision,
    required this.turnId,
    required this.kind,
    required this.summary,
    this.summaryTruncated = false,
    this.tools = ThreadActivityTools.empty,
    this.inputId,
    this.attemptId,
  });

  final String threadId;

  /// 稳定活动身份 `activity:{turnId}:{step}:{kind}`；同一活动内 revision 单调递增，
  /// 身份变化即新活动（版本从 0 重启）。
  final String identity;
  final int revision;
  final String turnId;
  final String? inputId;
  final String? attemptId;
  final ThreadActivityKind kind;

  /// 最新非空逻辑行；`summary` 为空串表示该阶段没有可展示摘要。
  final String summary;
  final bool summaryTruncated;
  final ThreadActivityTools tools;
}

/// 后端 typed 存储故障类别；它不是从错误文本解析推断出来的。
enum ThreadHistoryFault {
  queueFull,
  writeFailed,
  writerUnavailable,
  noProgress,
  checkpointFailed,
  blobFailed,
}

enum ThreadStorageExecution { running, pausing, paused }

/// Thread 持久化状态；所有字段都是 typed 事实，缺失表示未知而不是零。
class ThreadStorageStateView {
  const ThreadStorageStateView({
    this.fault,
    this.faultGeneration = 0,
    this.acceptedSequence,
    this.durableSequence,
    this.execution = ThreadStorageExecution.running,
    this.pressurePaused = false,
    this.resumeRequired = false,
    this.canResume = false,
    this.lastError,
  });

  final ThreadHistoryFault? fault;

  /// 手动恢复命令需要匹配的代数。
  final int faultGeneration;

  /// 已接纳水位；`null` 表示未知，不是零。
  final int? acceptedSequence;

  /// 已落盘水位；`null` 表示未知，不是零。
  final int? durableSequence;
  final ThreadStorageExecution execution;
  final bool pressurePaused;

  /// 硬故障是否仍闩住新准入：重试保存成功也不会自动解除，只有一次核验过的显式继续
  /// （`resume_thread_history`）才清除。
  final bool resumeRequired;

  /// 后端是否已核验“显式继续现在会被接受”。
  ///
  /// 由 core + writer 按同一代数的重试世代/fence 派生：重试保存已按 `faultGeneration`
  /// 确认、且后端 canonical 状态允许继续时才为 true。**不能**由前端从 [lastError] 文本
  /// 推断，也不能仅凭用户点了重试就本地置真——那是第二份事实源。
  final bool canResume;

  /// core 的原始错误文本；它不是故障类别的来源。
  final String? lastError;

  bool get hasFault => fault != null;

  /// 保存被明确阻塞（有故障、处于压力暂停、已暂停执行，或硬故障仍闩住准入）。
  ///
  /// `pressure_paused` 是自动恢复的短暂背压；[resumeRequired] 是硬故障留下的显式闸门：
  /// 即使故障类别已清空、也不再有压力，`true` 仍表示准入被闩住。
  bool get blocksContinuation =>
      hasFault ||
      pressurePaused ||
      resumeRequired ||
      execution != ThreadStorageExecution.running;

  /// 准确的阻塞原因：故障类别优先，其后是压力暂停与暂停执行；没有权威文本时为空。
  String? get blockingReason {
    if (!blocksContinuation) return null;
    final error = lastError?.trim();
    if (error != null && error.isNotEmpty) return error;
    return null;
  }
}

/// 活动详情里的一段内容（reasoning 或输出正文）。
class ThreadActivityContentPart {
  const ThreadActivityContentPart({
    required this.itemId,
    required this.complete,
    required this.text,
    this.revision,
  });

  final String itemId;

  /// canonical item revision；`null` 表示纯内存推导，没有可证明的 revision。
  final int? revision;
  final bool complete;
  final String text;
}

/// 活动详情里的一条工具调用（含完整参数与已观测输出）。
class ThreadActivityToolDetail {
  const ThreadActivityToolDetail({
    required this.callId,
    required this.name,
    required this.state,
    this.taskId,
    this.arguments,
    this.output,
    this.ordinal,
    this.startedAt,
  });

  final String callId;
  final String? taskId;
  final String name;
  final ThreadActivityToolState state;

  /// canonical 原始调用参数；不可读时为空。
  final String? arguments;

  /// 目前观测到的流式或终态输出；暂无输出为空。
  final String? output;
  final int? ordinal;
  final int? startedAt;
}

/// 按活动身份读取的详情结果；只有 [CurrentThreadActivityDetail] 携带正文。
sealed class ThreadActivityDetail {
  const ThreadActivityDetail();
}

final class CurrentThreadActivityDetail extends ThreadActivityDetail {
  const CurrentThreadActivityDetail({
    required this.activity,
    this.reasoning = const [],
    this.response = const [],
    this.tools = const [],
  });

  final ThreadActivityView activity;
  final List<ThreadActivityContentPart> reasoning;
  final List<ThreadActivityContentPart> response;
  final List<ThreadActivityToolDetail> tools;
}

/// 请求的身份已不再是当前活动；客户端丢弃迟到的展开结果。
final class SupersededThreadActivityDetail extends ThreadActivityDetail {
  const SupersededThreadActivityDetail({
    required this.activity,
    required this.requestedActivityId,
  });

  final ThreadActivityView activity;
  final String requestedActivityId;
}

/// 活动所属 Thread 已不再驻留 / 活动已结束。
final class EndedThreadActivityDetail extends ThreadActivityDetail {
  const EndedThreadActivityDetail({
    required this.threadId,
    required this.activityId,
  });

  final String threadId;
  final String activityId;
}
