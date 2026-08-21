import 'package:flutter/foundation.dart' show listEquals;

class RuntimeCostView {
  const RuntimeCostView({required this.currency, required this.amount});

  final String currency;
  final double amount;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is RuntimeCostView &&
            currency == other.currency &&
            amount == other.amount;
  }

  @override
  int get hashCode => Object.hash(currency, amount);
}

/// 已知币种显示为货币符号；未知币种回退为币种代码前缀。
const Map<String, String> _runtimeCurrencySymbols = {'CNY': '￥', 'USD': r'$'};

/// 费用金额显示：最多保留 6 位小数并去掉尾随零。
String formatRuntimeCostAmount(String currency, double amount) {
  final fixed = amount.toStringAsFixed(6);
  var compact = fixed.replaceFirst(RegExp(r'\.?0+$'), '');
  if (compact == '-0' || compact.isEmpty) {
    compact = '0';
  }
  final symbol = _runtimeCurrencySymbols[currency.toUpperCase()];
  if (symbol != null) {
    return '$symbol$compact';
  }
  return '$currency $compact'.trim();
}

/// 多币种实际花费合并显示，如 `￥1.2 + $2.6`；不做汇率换算。
String formatRuntimeCosts(Iterable<RuntimeCostView> costs) {
  return costs
      .map((cost) => formatRuntimeCostAmount(cost.currency, cost.amount))
      .where((label) => label.isNotEmpty)
      .join(' + ');
}

class ThreadRuntimeView {
  const ThreadRuntimeView({
    required this.model,
    required this.contextTokens,
    required this.contextWindow,
    required this.totalTokens,
    required this.costLabel,
    required this.activeSkills,
    required this.activeMcpServers,
    required this.activeLspServers,
    required this.agentCount,
    this.promptTokens = 0,
    this.completionTokens = 0,
    this.cachedPromptTokens = 0,
    this.cacheWriteTokens = 0,
    this.cacheMissTokens = 0,
    this.reasoningTokens = 0,
    this.inferenceCount = 0,
    this.cacheHitRate,
    this.estimatedCosts = const [],
    this.estimatedCacheSavings = const [],
    this.hasUnpricedUsage = false,
    this.promptGeneration,
    this.promptCachePolicy,
    this.prefixChangedReason,
    this.toolRegistryRevision,
    this.toolCatalogHash,
    this.task,
  });

  final String model;
  final int contextTokens;
  final int contextWindow;
  final int totalTokens;
  final String costLabel;
  final List<String> activeSkills;
  final List<String> activeMcpServers;
  final List<String> activeLspServers;
  final int agentCount;
  final int promptTokens;
  final int completionTokens;
  final int cachedPromptTokens;
  final int cacheWriteTokens;
  final int cacheMissTokens;
  final int reasoningTokens;
  final int inferenceCount;
  final double? cacheHitRate;
  final List<RuntimeCostView> estimatedCosts;
  final List<RuntimeCostView> estimatedCacheSavings;
  final bool hasUnpricedUsage;
  final int? promptGeneration;
  final String? promptCachePolicy;
  final String? prefixChangedReason;

  /// 本 Turn 工具 lease 冻结的注册表发布代数；仅诊断，供 Driver 断言。
  final int? toolRegistryRevision;

  /// 本 Turn 工具 lease 冻结的 deferred Tool Search catalog 指纹；仅诊断。
  final String? toolCatalogHash;
  final TaskRuntimeView? task;

  bool get hasActiveTask => task?.isActive ?? false;
  bool get hasUsage =>
      inferenceCount > 0 || promptTokens > 0 || completionTokens > 0;

  double? get effectiveCacheHitRate {
    if (!hasUsage) return null;
    final reported = cacheHitRate;
    if (reported != null) return reported.clamp(0.0, 1.0);
    if (promptTokens <= 0) return null;
    return (cachedPromptTokens / promptTokens).clamp(0.0, 1.0);
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is ThreadRuntimeView &&
            model == other.model &&
            contextTokens == other.contextTokens &&
            contextWindow == other.contextWindow &&
            totalTokens == other.totalTokens &&
            costLabel == other.costLabel &&
            listEquals(activeSkills, other.activeSkills) &&
            listEquals(activeMcpServers, other.activeMcpServers) &&
            listEquals(activeLspServers, other.activeLspServers) &&
            agentCount == other.agentCount &&
            promptTokens == other.promptTokens &&
            completionTokens == other.completionTokens &&
            cachedPromptTokens == other.cachedPromptTokens &&
            cacheWriteTokens == other.cacheWriteTokens &&
            cacheMissTokens == other.cacheMissTokens &&
            reasoningTokens == other.reasoningTokens &&
            inferenceCount == other.inferenceCount &&
            cacheHitRate == other.cacheHitRate &&
            listEquals(estimatedCosts, other.estimatedCosts) &&
            listEquals(estimatedCacheSavings, other.estimatedCacheSavings) &&
            hasUnpricedUsage == other.hasUnpricedUsage &&
            promptGeneration == other.promptGeneration &&
            promptCachePolicy == other.promptCachePolicy &&
            prefixChangedReason == other.prefixChangedReason &&
            toolRegistryRevision == other.toolRegistryRevision &&
            toolCatalogHash == other.toolCatalogHash &&
            task == other.task;
  }

  @override
  int get hashCode => Object.hashAll([
    model,
    contextTokens,
    contextWindow,
    totalTokens,
    costLabel,
    Object.hashAll(activeSkills),
    Object.hashAll(activeMcpServers),
    Object.hashAll(activeLspServers),
    agentCount,
    promptTokens,
    completionTokens,
    cachedPromptTokens,
    cacheWriteTokens,
    cacheMissTokens,
    reasoningTokens,
    inferenceCount,
    cacheHitRate,
    Object.hashAll(estimatedCosts),
    Object.hashAll(estimatedCacheSavings),
    hasUnpricedUsage,
    promptGeneration,
    promptCachePolicy,
    prefixChangedReason,
    toolRegistryRevision,
    toolCatalogHash,
    task,
  ]);

  ThreadRuntimeView copyWith({
    String? model,
    int? contextTokens,
    int? contextWindow,
    int? totalTokens,
    String? costLabel,
    List<String>? activeSkills,
    List<String>? activeMcpServers,
    List<String>? activeLspServers,
    int? agentCount,
    int? promptTokens,
    int? completionTokens,
    int? cachedPromptTokens,
    int? cacheWriteTokens,
    int? cacheMissTokens,
    int? reasoningTokens,
    int? inferenceCount,
    double? cacheHitRate,
    List<RuntimeCostView>? estimatedCosts,
    List<RuntimeCostView>? estimatedCacheSavings,
    bool? hasUnpricedUsage,
    int? promptGeneration,
    String? promptCachePolicy,
    String? prefixChangedReason,
    int? toolRegistryRevision,
    String? toolCatalogHash,
    TaskRuntimeView? task,
  }) {
    return ThreadRuntimeView(
      model: model ?? this.model,
      contextTokens: contextTokens ?? this.contextTokens,
      contextWindow: contextWindow ?? this.contextWindow,
      totalTokens: totalTokens ?? this.totalTokens,
      costLabel: costLabel ?? this.costLabel,
      activeSkills: activeSkills ?? this.activeSkills,
      activeMcpServers: activeMcpServers ?? this.activeMcpServers,
      activeLspServers: activeLspServers ?? this.activeLspServers,
      agentCount: agentCount ?? this.agentCount,
      promptTokens: promptTokens ?? this.promptTokens,
      completionTokens: completionTokens ?? this.completionTokens,
      cachedPromptTokens: cachedPromptTokens ?? this.cachedPromptTokens,
      cacheWriteTokens: cacheWriteTokens ?? this.cacheWriteTokens,
      cacheMissTokens: cacheMissTokens ?? this.cacheMissTokens,
      reasoningTokens: reasoningTokens ?? this.reasoningTokens,
      inferenceCount: inferenceCount ?? this.inferenceCount,
      cacheHitRate: cacheHitRate ?? this.cacheHitRate,
      estimatedCosts: estimatedCosts ?? this.estimatedCosts,
      estimatedCacheSavings:
          estimatedCacheSavings ?? this.estimatedCacheSavings,
      hasUnpricedUsage: hasUnpricedUsage ?? this.hasUnpricedUsage,
      promptGeneration: promptGeneration ?? this.promptGeneration,
      promptCachePolicy: promptCachePolicy ?? this.promptCachePolicy,
      prefixChangedReason: prefixChangedReason ?? this.prefixChangedReason,
      toolRegistryRevision: toolRegistryRevision ?? this.toolRegistryRevision,
      toolCatalogHash: toolCatalogHash ?? this.toolCatalogHash,
      task: task ?? this.task,
    );
  }
}

enum TaskStateKind {
  designUpdating,
  implementing,
  merging,
  reviewing,
  reworking,
  stopping,
  blocked,
  completed,
  failed,
  cancelled,
}

sealed class TaskStateView {
  const TaskStateView(this.data);

  factory TaskStateView.facts({
    required TaskStateKind kind,
    int generation = 0,
    String? statusMessage,
    String? stopRequestedOrigin,
    String? stopRequestedReason,
  }) {
    final data = TaskStateDataView(
      generation: generation,
      statusMessage: statusMessage,
      stopRequestedOrigin: stopRequestedOrigin,
      stopRequestedReason: stopRequestedReason,
    );
    return switch (kind) {
      TaskStateKind.designUpdating => DesignUpdatingTaskStateView(data),
      TaskStateKind.implementing => ImplementingTaskStateView(data),
      TaskStateKind.merging => MergingTaskStateView(data),
      TaskStateKind.reviewing => ReviewingTaskStateView(data),
      TaskStateKind.reworking => ReworkingTaskStateView(data),
      TaskStateKind.stopping => StoppingTaskStateView(data),
      TaskStateKind.blocked => BlockedTaskStateView(data),
      TaskStateKind.completed => CompletedTaskStateView(data),
      TaskStateKind.failed => FailedTaskStateView(data),
      TaskStateKind.cancelled => CancelledTaskStateView(data),
    };
  }

  final TaskStateDataView data;
  TaskStateKind get kind;
}

final class DesignUpdatingTaskStateView extends TaskStateView {
  const DesignUpdatingTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.designUpdating;
}

final class ImplementingTaskStateView extends TaskStateView {
  const ImplementingTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.implementing;
}

final class MergingTaskStateView extends TaskStateView {
  const MergingTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.merging;
}

final class ReviewingTaskStateView extends TaskStateView {
  const ReviewingTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.reviewing;
}

final class ReworkingTaskStateView extends TaskStateView {
  const ReworkingTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.reworking;
}

final class StoppingTaskStateView extends TaskStateView {
  const StoppingTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.stopping;
}

final class BlockedTaskStateView extends TaskStateView {
  const BlockedTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.blocked;
}

final class CompletedTaskStateView extends TaskStateView {
  const CompletedTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.completed;
}

final class FailedTaskStateView extends TaskStateView {
  const FailedTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.failed;
}

final class CancelledTaskStateView extends TaskStateView {
  const CancelledTaskStateView(super.data);

  @override
  TaskStateKind get kind => TaskStateKind.cancelled;
}

class TaskStateDataView {
  const TaskStateDataView({
    required this.generation,
    required this.statusMessage,
    required this.stopRequestedOrigin,
    required this.stopRequestedReason,
  });

  final int generation;
  final String? statusMessage;
  final String? stopRequestedOrigin;
  final String? stopRequestedReason;
}

class TaskRuntimeView {
  const TaskRuntimeView({
    required this.runId,
    required this.state,
    required this.branch,
    required this.expectedHead,
    required this.revision,
    this.integratedReviewGate = const IntegratedReviewGateView.required(
      reason: 'review gate unavailable',
    ),
    this.failures = const [],
    this.terminalFailure,
    required this.workUnits,
    required this.completions,
    required this.merges,
    required this.reviews,
  });

  final String runId;
  final TaskStateView state;
  final String branch;
  final String expectedHead;
  final int revision;
  final IntegratedReviewGateView integratedReviewGate;
  final List<TaskFailureView> failures;
  final TaskFailureView? terminalFailure;
  final List<TaskWorkUnitView> workUnits;
  final List<TaskCompletionView> completions;
  final List<TaskMergeView> merges;
  final List<TaskReviewView> reviews;

  String? get statusMessage => state.data.statusMessage;
  String? get stopRequestedOrigin => state.data.stopRequestedOrigin;
  String? get stopRequestedReason => state.data.stopRequestedReason;
  int get taskGeneration => state.data.generation;

  bool get isActive => switch (state.kind) {
    TaskStateKind.completed ||
    TaskStateKind.failed ||
    TaskStateKind.cancelled => false,
    TaskStateKind.designUpdating ||
    TaskStateKind.implementing ||
    TaskStateKind.merging ||
    TaskStateKind.reviewing ||
    TaskStateKind.reworking ||
    TaskStateKind.stopping ||
    TaskStateKind.blocked => true,
  };

  bool get hasRecoverableExecutorFailure {
    if (!const {
      TaskStateKind.designUpdating,
      TaskStateKind.implementing,
      TaskStateKind.reworking,
    }.contains(state.kind)) {
      return false;
    }
    final hasFailedExecutor = workUnits.any(
      (unit) => const {'failed', 'interrupted'}.contains(unit.executionStatus),
    );
    final hasInFlightWork = workUnits.any(
      (unit) =>
          const {'queued', 'running'}.contains(unit.executionStatus) ||
          const {'running', 'reviewing'}.contains(unit.status),
    );
    return hasFailedExecutor && !hasInFlightWork;
  }
}

enum IntegratedReviewGateKind {
  required,
  satisfiedByReview,
  notRequiredNoDelivery,
  notRequiredSingleExecutorEquivalent,
}

class IntegratedReviewGateView {
  const IntegratedReviewGateView.required({required this.reason})
    : kind = IntegratedReviewGateKind.required,
      reviewRoundId = null,
      reviewedHead = null,
      workUnitId = null,
      completionRevision = null,
      mergeRecordId = null;

  const IntegratedReviewGateView.satisfiedByReview({
    required this.reviewRoundId,
    required this.reviewedHead,
  }) : kind = IntegratedReviewGateKind.satisfiedByReview,
       reason = null,
       workUnitId = null,
       completionRevision = null,
       mergeRecordId = null;

  const IntegratedReviewGateView.notRequiredNoDelivery()
    : kind = IntegratedReviewGateKind.notRequiredNoDelivery,
      reason = null,
      reviewRoundId = null,
      reviewedHead = null,
      workUnitId = null,
      completionRevision = null,
      mergeRecordId = null;

  const IntegratedReviewGateView.notRequiredSingleExecutorEquivalent({
    required this.workUnitId,
    required this.completionRevision,
    required this.mergeRecordId,
  }) : kind = IntegratedReviewGateKind.notRequiredSingleExecutorEquivalent,
       reason = null,
       reviewRoundId = null,
       reviewedHead = null;

  final IntegratedReviewGateKind kind;
  final String? reason;
  final String? reviewRoundId;
  final String? reviewedHead;
  final String? workUnitId;
  final int? completionRevision;
  final String? mergeRecordId;
}

class TaskFailureView {
  const TaskFailureView({
    required this.id,
    required this.sourceThreadId,
    required this.sourceTurnId,
    required this.sourceAgentId,
    required this.sourceRole,
    required this.workUnitId,
    required this.reviewRoundId,
    required this.disposition,
    required this.category,
    required this.providerKind,
    required this.code,
    required this.httpStatus,
    required this.message,
    required this.retryable,
    required this.resolvedAt,
    required this.createdAt,
  });

  final String id;
  final String sourceThreadId;
  final String sourceTurnId;
  final String sourceAgentId;
  final String sourceRole;
  final String? workUnitId;
  final String? reviewRoundId;
  final String disposition;
  final String category;
  final String? providerKind;
  final String? code;
  final int? httpStatus;
  final String message;
  final bool retryable;
  final DateTime? resolvedAt;
  final DateTime createdAt;

  bool get isFatal => disposition == 'fatal';
}

enum TaskWorkUnitStateKind {
  pending,
  running,
  awaitingCompletion,
  readyForReview,
  reviewing,
  changesRequested,
  approved,
  merged,
  noDelivery,
  needsAttention,
  failed,
  cancelled,
}

enum TaskWorkUnitExecution {
  queued,
  running,
  budgetLimited,
  completed,
  failed,
  cancelled,
}

enum TaskExecutorContinuationState {
  none,
  pendingStart,
  compacting,
  plannerWakePending,
  needsAttention,
}

class TaskWorkUnitStateView {
  const TaskWorkUnitStateView({
    required this.kind,
    required this.execution,
    required this.progress,
  });

  factory TaskWorkUnitStateView.facts({
    required TaskWorkUnitStateKind kind,
    required TaskWorkUnitExecution execution,
    String? executionError,
    TaskBudgetLimitView? budgetLimit,
    int budgetSliceCount = 1,
    TaskExecutorContinuationState continuationState =
        TaskExecutorContinuationState.none,
    String? continuationSourceTurnId,
    required BigInt continuationRevision,
  }) => TaskWorkUnitStateView(
    kind: kind,
    execution: execution,
    progress: TaskWorkUnitProgressView(
      executionError: executionError,
      budgetLimit: budgetLimit,
      budgetSliceCount: budgetSliceCount,
      continuationState: continuationState,
      continuationSourceTurnId: continuationSourceTurnId,
      continuationRevision: continuationRevision,
    ),
  );

  final TaskWorkUnitStateKind kind;
  final TaskWorkUnitExecution execution;
  final TaskWorkUnitProgressView progress;
}

class TaskWorkUnitProgressView {
  const TaskWorkUnitProgressView({
    required this.executionError,
    required this.budgetLimit,
    required this.budgetSliceCount,
    required this.continuationState,
    required this.continuationSourceTurnId,
    required this.continuationRevision,
  });

  final String? executionError;
  final TaskBudgetLimitView? budgetLimit;
  final int budgetSliceCount;
  final TaskExecutorContinuationState continuationState;
  final String? continuationSourceTurnId;
  final BigInt continuationRevision;
}

class TaskWorkUnitView {
  const TaskWorkUnitView({
    required this.id,
    required this.title,
    required this.state,
    required this.worktreePath,
    required this.branch,
    required this.agentId,
    required this.budgetSliceLimit,
    required this.executorProgressRevision,
    this.blueprintFingerprint,
    this.objective,
    this.implementationStepCount = 0,
    this.acceptanceCriterionCount = 0,
    this.verificationCount = 0,
  });

  final String id;
  final String title;
  final TaskWorkUnitStateView state;
  final String worktreePath;
  final String branch;
  final String? agentId;
  final int budgetSliceLimit;
  final BigInt executorProgressRevision;
  final String? blueprintFingerprint;
  final String? objective;
  final int implementationStepCount;
  final int acceptanceCriterionCount;
  final int verificationCount;

  String get status => state.kind.name;
  String get executionStatus => state.execution.name;
  String? get executionError => state.progress.executionError;
  TaskBudgetLimitView? get budgetLimit => state.progress.budgetLimit;
  int get budgetSliceCount => state.progress.budgetSliceCount;
  String get continuationState => state.progress.continuationState.name;
  String? get continuationSourceTurnId =>
      state.progress.continuationSourceTurnId;
  BigInt get continuationRevision => state.progress.continuationRevision;
}

class TaskBudgetLimitView {
  const TaskBudgetLimitView({required this.kind, required this.usage});

  final String kind;
  final TaskBudgetUsageView usage;
}

class TaskBudgetUsageView {
  const TaskBudgetUsageView({
    required this.modelSteps,
    required this.toolCalls,
    required this.waitCalls,
    required this.elapsedMs,
  });

  final int modelSteps;
  final int toolCalls;
  final int waitCalls;
  final BigInt elapsedMs;
}

class TaskCompletionView {
  const TaskCompletionView({
    required this.id,
    required this.workUnitId,
    required this.executorAgentId,
    required this.revision,
    required this.kind,
    required this.status,
    required this.baseCommit,
    required this.headCommit,
    required this.changedFiles,
    required this.verificationSummary,
    required this.worktreePath,
    required this.branch,
    required this.createdAt,
    required this.updatedAt,
  });

  final String id;
  final String workUnitId;
  final String executorAgentId;
  final int revision;
  final String kind;
  final String status;
  final String baseCommit;
  final String? headCommit;
  final List<String> changedFiles;
  final String verificationSummary;
  final String worktreePath;
  final String branch;
  final DateTime createdAt;
  final DateTime updatedAt;
}

class TaskMergeView {
  const TaskMergeView({
    required this.id,
    required this.workUnitId,
    required this.completionId,
    required this.completionRevision,
    required this.executorAgentId,
    required this.expectedPreviousHead,
    required this.resultingHead,
    required this.deliveryHead,
    required this.method,
    required this.summary,
    required this.cleanupStatus,
    required this.cleanupDetail,
    required this.createdAt,
    required this.updatedAt,
  });

  final String id;
  final String workUnitId;
  final String completionId;
  final int completionRevision;
  final String executorAgentId;
  final String expectedPreviousHead;
  final String resultingHead;
  final String deliveryHead;
  final String method;
  final String summary;
  final String cleanupStatus;
  final String? cleanupDetail;
  final DateTime createdAt;
  final DateTime updatedAt;
}

enum TaskReviewStateKind { pending, pass, changesRequired, blocked, failed }

class TaskReviewStateView {
  const TaskReviewStateView({required this.kind, this.summary, this.error});

  final TaskReviewStateKind kind;
  final String? summary;
  final String? error;
}

class TaskReviewView {
  const TaskReviewView({
    required this.id,
    required this.round,
    required this.scope,
    required this.workUnitId,
    required this.completionId,
    required this.completionRevision,
    required this.reviewedHead,
    required this.state,
    required this.requestedByCallId,
    required this.reviewerAgentId,
    required this.designReferences,
    required this.findings,
    required this.createdAt,
    required this.updatedAt,
  });

  final String id;
  final int round;
  final String scope;
  final String? workUnitId;
  final String? completionId;
  final int? completionRevision;
  final String reviewedHead;
  final TaskReviewStateView state;
  final String requestedByCallId;
  final String? reviewerAgentId;
  final List<TaskDesignReferenceView> designReferences;
  final List<TaskReviewFindingView> findings;
  final DateTime createdAt;
  final DateTime updatedAt;

  String get verdict => state.kind.name;
  String? get summary => state.summary;
}

class TaskDesignReferenceView {
  const TaskDesignReferenceView({required this.path, required this.section});

  final String path;
  final String section;
}

class TaskReviewFindingView {
  const TaskReviewFindingView({
    required this.severity,
    required this.title,
    required this.body,
    required this.recommendation,
    required this.path,
    required this.line,
    required this.designReferences,
  });

  final String severity;
  final String title;
  final String body;
  final String recommendation;
  final String? path;
  final int? line;
  final List<TaskDesignReferenceView> designReferences;
}
