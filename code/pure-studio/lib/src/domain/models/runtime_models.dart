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
      task: task ?? this.task,
    );
  }
}

enum TaskStateKind {
  planning,
  pendingConfirmation,
  editingDocuments,
  working,
  reviewing,
  completed,
}

sealed class TaskStateView {
  const TaskStateView();

  TaskStateKind get kind;
}

final class PlanningTaskStateView extends TaskStateView {
  const PlanningTaskStateView({required this.request});

  final String request;

  @override
  TaskStateKind get kind => TaskStateKind.planning;
}

final class PendingConfirmationTaskStateView extends TaskStateView {
  const PendingConfirmationTaskStateView({required this.planRevision});

  final int planRevision;

  @override
  TaskStateKind get kind => TaskStateKind.pendingConfirmation;
}

final class EditingDocumentsTaskStateView extends TaskStateView {
  const EditingDocumentsTaskStateView({required this.planRevision});

  final int planRevision;

  @override
  TaskStateKind get kind => TaskStateKind.editingDocuments;
}

final class WorkingTaskStateView extends TaskStateView {
  const WorkingTaskStateView({required this.documentEditSummary});

  final String documentEditSummary;

  @override
  TaskStateKind get kind => TaskStateKind.working;
}

final class ReviewingTaskStateView extends TaskStateView {
  const ReviewingTaskStateView({required this.target});

  final IntegratedReviewTargetView target;

  @override
  TaskStateKind get kind => TaskStateKind.reviewing;
}

final class IntegratedReviewTargetView {
  const IntegratedReviewTargetView({
    required this.reviewRoundId,
    required this.reviewedHead,
    required this.changedFiles,
  });

  final String reviewRoundId;
  final String reviewedHead;
  final List<String> changedFiles;
}

final class CompletedTaskStateView extends TaskStateView {
  const CompletedTaskStateView({required this.outcome});

  final TaskOutcomeView outcome;

  @override
  TaskStateKind get kind => TaskStateKind.completed;
}

sealed class TaskOutcomeView {
  const TaskOutcomeView();
}

final class SucceededTaskOutcomeView extends TaskOutcomeView {
  const SucceededTaskOutcomeView({
    required this.summary,
    required this.completedAt,
    required this.reviewGate,
  });

  final String summary;
  final DateTime completedAt;
  final TaskReviewGateView reviewGate;
}

enum TaskFailureKindView { unableToProceed, fatal }

final class FailedTaskOutcomeView extends TaskOutcomeView {
  const FailedTaskOutcomeView({
    required this.kind,
    required this.summary,
    required this.evidence,
    required this.cause,
    required this.completedAt,
  });

  final TaskFailureKindView kind;
  final String summary;
  final String evidence;
  final String cause;
  final DateTime completedAt;
}

sealed class TaskReviewGateView {
  const TaskReviewGateView();
}

final class NoDeliveryTaskReviewGateView extends TaskReviewGateView {
  const NoDeliveryTaskReviewGateView();
}

final class SingleExecutorTaskReviewGateView extends TaskReviewGateView {
  const SingleExecutorTaskReviewGateView({required this.workUnitId});

  final String workUnitId;
}

final class IntegratedTaskReviewGateView extends TaskReviewGateView {
  const IntegratedTaskReviewGateView({required this.reviewRoundId});

  final String reviewRoundId;
}

class TaskRuntimeView {
  const TaskRuntimeView({
    required this.runId,
    required this.state,
    required this.revision,
    required this.generation,
    this.integratedReviewGate = const IntegratedReviewGateView.required(
      reason: 'review gate unavailable',
    ),
    this.issues = const [],
    required this.workUnits,
    required this.completions,
    required this.merges,
    required this.reviews,
  });

  final String runId;
  final TaskStateView state;
  final int revision;
  final int generation;
  final IntegratedReviewGateView integratedReviewGate;
  final List<TaskIssueView> issues;
  final List<TaskWorkUnitView> workUnits;
  final List<TaskCompletionView> completions;
  final List<TaskMergeView> merges;
  final List<TaskReviewView> reviews;

  String get stateSummary => switch (state) {
    PlanningTaskStateView(:final request) => request,
    PendingConfirmationTaskStateView(:final planRevision) =>
      'Plan revision $planRevision awaits confirmation',
    EditingDocumentsTaskStateView(:final planRevision) =>
      'Editing documents for plan revision $planRevision',
    WorkingTaskStateView(:final documentEditSummary) => documentEditSummary,
    ReviewingTaskStateView(:final target) => 'Reviewing ${target.reviewedHead}',
    CompletedTaskStateView(:final outcome) => switch (outcome) {
      SucceededTaskOutcomeView(:final summary) ||
      FailedTaskOutcomeView(:final summary) => summary,
    },
  };

  bool get isActive => state.kind != TaskStateKind.completed;
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

class TaskIssueView {
  const TaskIssueView({
    required this.id,
    required this.sourceThreadId,
    required this.sourceTurnId,
    required this.sourceAgentId,
    required this.sourceRole,
    required this.workUnitId,
    required this.reviewRoundId,
    required this.state,
    required this.createdAt,
  });

  final String id;
  final String sourceThreadId;
  final String sourceTurnId;
  final String sourceAgentId;
  final String sourceRole;
  final String? workUnitId;
  final String? reviewRoundId;
  final TaskIssueStateView state;
  final DateTime createdAt;

  String get disposition => switch (state) {
    OpenRecoverableTaskIssueView() || ResolvedTaskIssueView() => 'recoverable',
    OpenFatalTaskIssueView() => 'fatal',
  };
  TaskFailureDetailView get failure => state.failure;
  String get category => failure.category;
  String? get providerKind => failure.providerKind;
  String? get code => failure.code;
  int? get httpStatus => failure.httpStatus;
  String get message => failure.message;
  bool get retryable => failure.retryable;
  DateTime? get resolvedAt => switch (state) {
    ResolvedTaskIssueView(:final resolvedAt) => resolvedAt,
    OpenRecoverableTaskIssueView() || OpenFatalTaskIssueView() => null,
  };
  bool get isFatal => state is OpenFatalTaskIssueView;
}

sealed class TaskIssueStateView {
  const TaskIssueStateView(this.failure);

  final TaskFailureDetailView failure;
}

final class OpenRecoverableTaskIssueView extends TaskIssueStateView {
  const OpenRecoverableTaskIssueView(super.failure);
}

final class OpenFatalTaskIssueView extends TaskIssueStateView {
  const OpenFatalTaskIssueView(super.failure);
}

final class ResolvedTaskIssueView extends TaskIssueStateView {
  const ResolvedTaskIssueView(super.failure, this.resolvedAt);

  final DateTime resolvedAt;
}

class TaskFailureDetailView {
  const TaskFailureDetailView({
    required this.category,
    required this.providerKind,
    required this.code,
    required this.httpStatus,
    required this.message,
    required this.retryable,
  });

  final String category;
  final String? providerKind;
  final String? code;
  final int? httpStatus;
  final String message;
  final bool retryable;
}

enum TaskWorkUnitStateKind {
  pending,
  running,
  waitingReview,
  reviewPassed,
  changesRequired,
  paused,
  completed,
  failed,
  cancelled,
}

enum TaskExecutorContinuationStateKind {
  idle,
  compacting,
  pendingStart,
  plannerWakePending,
  needsAttention,
}

sealed class TaskWorkUnitStateView {
  const TaskWorkUnitStateView();

  TaskWorkUnitStateKind get kind;
}

final class PendingTaskWorkUnitStateView extends TaskWorkUnitStateView {
  const PendingTaskWorkUnitStateView();

  @override
  TaskWorkUnitStateKind get kind => TaskWorkUnitStateKind.pending;
}

final class RunningTaskWorkUnitStateView extends TaskWorkUnitStateView {
  const RunningTaskWorkUnitStateView({
    required this.activity,
    required this.continuation,
  });

  final TaskRunningWorkUnitActivityView activity;
  final TaskExecutorContinuationView continuation;

  @override
  TaskWorkUnitStateKind get kind => TaskWorkUnitStateKind.running;
}

final class WaitingReviewTaskWorkUnitStateView extends TaskWorkUnitStateView {
  const WaitingReviewTaskWorkUnitStateView(this.phase);

  final TaskWaitingReviewPhaseView phase;

  @override
  TaskWorkUnitStateKind get kind => TaskWorkUnitStateKind.waitingReview;
}

final class ReviewPassedTaskWorkUnitStateView extends TaskWorkUnitStateView {
  const ReviewPassedTaskWorkUnitStateView({
    required this.completionId,
    required this.completionRevision,
    required this.reviewRoundId,
    required this.outcome,
    required this.verificationSummary,
  });

  final String completionId;
  final int completionRevision;
  final String reviewRoundId;
  final TaskReviewPassedOutcomeView outcome;
  final String verificationSummary;

  @override
  TaskWorkUnitStateKind get kind => TaskWorkUnitStateKind.reviewPassed;
}

final class ChangesRequiredTaskWorkUnitStateView extends TaskWorkUnitStateView {
  const ChangesRequiredTaskWorkUnitStateView({
    required this.completionId,
    required this.completionRevision,
    required this.reviewRoundId,
    required this.continuationRevision,
    required this.sliceCount,
  });

  final String completionId;
  final int completionRevision;
  final String reviewRoundId;
  final BigInt continuationRevision;
  final int sliceCount;

  @override
  TaskWorkUnitStateKind get kind => TaskWorkUnitStateKind.changesRequired;
}

final class PausedTaskWorkUnitStateView extends TaskWorkUnitStateView {
  const PausedTaskWorkUnitStateView({
    required this.reason,
    required this.continuation,
  });

  final TaskWorkUnitPauseReasonView reason;
  final TaskExecutorContinuationView continuation;

  @override
  TaskWorkUnitStateKind get kind => TaskWorkUnitStateKind.paused;
}

final class CompletedTaskWorkUnitStateView extends TaskWorkUnitStateView {
  const CompletedTaskWorkUnitStateView(this.outcome);

  final TaskWorkUnitCompletionOutcomeView outcome;

  @override
  TaskWorkUnitStateKind get kind => TaskWorkUnitStateKind.completed;
}

final class FailedTaskWorkUnitStateView extends TaskWorkUnitStateView {
  const FailedTaskWorkUnitStateView({
    required this.failure,
    required this.worktreeDisposition,
  });

  final TaskWorkUnitFailureView failure;
  final TaskWorktreeDispositionView worktreeDisposition;

  @override
  TaskWorkUnitStateKind get kind => TaskWorkUnitStateKind.failed;
}

enum TaskWorktreeDispositionView { protect, cleanupRequested }

final class CancelledTaskWorkUnitStateView extends TaskWorkUnitStateView {
  const CancelledTaskWorkUnitStateView({
    required this.operationId,
    required this.reason,
    required this.worktreeDisposition,
  });

  final String operationId;
  final String reason;
  final TaskWorktreeDispositionView worktreeDisposition;

  @override
  TaskWorkUnitStateKind get kind => TaskWorkUnitStateKind.cancelled;
}

sealed class TaskRunningWorkUnitActivityView {
  const TaskRunningWorkUnitActivityView();
}

final class AllocatedTaskRunningActivityView
    extends TaskRunningWorkUnitActivityView {
  const AllocatedTaskRunningActivityView();
}

final class ActiveTaskRunningActivityView
    extends TaskRunningWorkUnitActivityView {
  const ActiveTaskRunningActivityView(this.turnId);

  final String turnId;
}

sealed class TaskWaitingReviewPhaseView {
  const TaskWaitingReviewPhaseView();
}

final class AwaitingReportTaskWaitingReviewView
    extends TaskWaitingReviewPhaseView {
  const AwaitingReportTaskWaitingReviewView({
    required this.outcome,
    required this.continuation,
  });

  final TaskExecutorTerminalOutcomeView outcome;
  final TaskExecutorContinuationView continuation;
}

final class ReadyTaskWaitingReviewView extends TaskWaitingReviewPhaseView {
  const ReadyTaskWaitingReviewView({
    required this.completionId,
    required this.completionRevision,
    required this.verificationSummary,
  });

  final String completionId;
  final int completionRevision;
  final String verificationSummary;
}

final class ReviewingTaskWaitingReviewView extends TaskWaitingReviewPhaseView {
  const ReviewingTaskWaitingReviewView({
    required this.completionId,
    required this.completionRevision,
    required this.reviewRoundId,
    required this.verificationSummary,
  });

  final String completionId;
  final int completionRevision;
  final String reviewRoundId;
  final String verificationSummary;
}

sealed class TaskExecutorTerminalOutcomeView {
  const TaskExecutorTerminalOutcomeView({
    required this.sourceTurnId,
    required this.detail,
  });

  final String sourceTurnId;
  final String detail;
}

final class CompletedTaskExecutorOutcomeView
    extends TaskExecutorTerminalOutcomeView {
  const CompletedTaskExecutorOutcomeView({
    required super.sourceTurnId,
    required super.detail,
  });
}

final class FailedTaskExecutorOutcomeView
    extends TaskExecutorTerminalOutcomeView {
  const FailedTaskExecutorOutcomeView({
    required super.sourceTurnId,
    required super.detail,
  });
}

enum TaskReviewPassedOutcomeView { delivery, noDelivery }

sealed class TaskWorkUnitPauseReasonView {
  const TaskWorkUnitPauseReasonView();
}

final class BudgetTaskWorkUnitPauseReasonView
    extends TaskWorkUnitPauseReasonView {
  const BudgetTaskWorkUnitPauseReasonView(this.limit);

  final TaskBudgetLimitView limit;
}

final class OperationalTaskWorkUnitPauseReasonView
    extends TaskWorkUnitPauseReasonView {
  const OperationalTaskWorkUnitPauseReasonView({
    required this.operationId,
    required this.detail,
  });

  final String operationId;
  final String detail;
}

sealed class TaskWorkUnitCompletionOutcomeView {
  const TaskWorkUnitCompletionOutcomeView();
}

final class MergedTaskWorkUnitCompletionView
    extends TaskWorkUnitCompletionOutcomeView {
  const MergedTaskWorkUnitCompletionView(this.mergeRecordId);

  final String mergeRecordId;
}

final class NoDeliveryTaskWorkUnitCompletionView
    extends TaskWorkUnitCompletionOutcomeView {
  const NoDeliveryTaskWorkUnitCompletionView(this.completionId);

  final String completionId;
}

sealed class TaskWorkUnitFailureView {
  const TaskWorkUnitFailureView();
}

final class SpawnTaskWorkUnitFailureView extends TaskWorkUnitFailureView {
  const SpawnTaskWorkUnitFailureView(this.failure);

  final TaskSpawnFailureView failure;
}

final class ExecutionTaskWorkUnitFailureView extends TaskWorkUnitFailureView {
  const ExecutionTaskWorkUnitFailureView({
    required this.operationId,
    required this.detail,
  });

  final String operationId;
  final String detail;
}

class TaskSpawnFailureView {
  const TaskSpawnFailureView({
    required this.code,
    required this.phase,
    required this.recoverable,
    required this.message,
    required this.taskRunId,
    required this.workUnitId,
    required this.agentId,
    required this.resource,
    required this.cause,
    required this.compensation,
    required this.nextAction,
  });

  final TaskSpawnFailureCodeView code;
  final TaskSpawnFailurePhaseView phase;
  final bool recoverable;
  final String message;
  final String? taskRunId;
  final String? workUnitId;
  final String agentId;
  final TaskSpawnResourceView? resource;
  final TaskWorktreeFailureCauseView cause;
  final TaskSpawnCompensationView compensation;
  final TaskSpawnNextActionView nextAction;
}

enum TaskSpawnFailureCodeView {
  allocation,
  worktreeCreate,
  childThreadCreate,
  agentRegistration,
  activation,
}

enum TaskSpawnFailurePhaseView {
  allocation,
  worktreeCreate,
  childThreadCreate,
  agentRegistration,
  activation,
}

enum TaskSpawnNextActionView {
  retryTaskSpawnExecutor,
  recoverWorktreeResources,
}

class TaskSpawnResourceView {
  const TaskSpawnResourceView({
    required this.repoRoot,
    required this.path,
    required this.branch,
    required this.baseRef,
  });

  final String repoRoot;
  final String path;
  final String branch;
  final String baseRef;
}

class TaskWorktreeFailureCauseView {
  const TaskWorktreeFailureCauseView({
    required this.kind,
    required this.message,
    required this.args,
    required this.exitCode,
    required this.stderr,
  });

  final TaskWorktreeFailureCauseKindView kind;
  final String message;
  final String? args;
  final int? exitCode;
  final String? stderr;
}

enum TaskWorktreeFailureCauseKindView {
  invalidRepoRoot,
  unsafeBranch,
  gitLaunchFailed,
  gitTimedOut,
  gitExited,
  gitStatusUnknown,
  io,
  disabled,
  operationAndCleanupFailed,
}

class TaskSpawnCompensationView {
  const TaskSpawnCompensationView({
    required this.allocation,
    required this.worktree,
    required this.childThread,
  });

  final TaskSpawnCompensationStateView allocation;
  final TaskSpawnCompensationStateView worktree;
  final TaskSpawnCompensationStateView childThread;
}

enum TaskSpawnCompensationStateView {
  notCreated,
  markedFailed,
  removed,
  faulted,
  cleanupFailed,
  unknown,
}

sealed class TaskExecutorContinuationView {
  const TaskExecutorContinuationView({
    required this.revision,
    required this.sliceCount,
  });

  final BigInt revision;
  final int sliceCount;
  TaskExecutorContinuationStateKind get kind;
  String? get sourceTurnId => null;
  TaskBudgetLimitView? get budgetLimit => null;
  String? get detail => null;
}

final class IdleTaskExecutorContinuationView
    extends TaskExecutorContinuationView {
  const IdleTaskExecutorContinuationView({
    required super.revision,
    required super.sliceCount,
  });

  @override
  TaskExecutorContinuationStateKind get kind =>
      TaskExecutorContinuationStateKind.idle;
}

final class CompactingTaskExecutorContinuationView
    extends TaskExecutorContinuationView {
  const CompactingTaskExecutorContinuationView({
    required super.revision,
    required super.sliceCount,
    required this.turnId,
  });

  final String turnId;

  @override
  TaskExecutorContinuationStateKind get kind =>
      TaskExecutorContinuationStateKind.compacting;

  @override
  String get sourceTurnId => turnId;
}

final class PendingStartTaskExecutorContinuationView
    extends TaskExecutorContinuationView {
  const PendingStartTaskExecutorContinuationView({
    required super.revision,
    required super.sliceCount,
    required this.turnId,
    required this.limit,
  });

  final String turnId;
  final TaskBudgetLimitView limit;

  @override
  TaskExecutorContinuationStateKind get kind =>
      TaskExecutorContinuationStateKind.pendingStart;

  @override
  String get sourceTurnId => turnId;

  @override
  TaskBudgetLimitView get budgetLimit => limit;
}

final class PlannerWakePendingTaskExecutorContinuationView
    extends TaskExecutorContinuationView {
  const PlannerWakePendingTaskExecutorContinuationView({
    required super.revision,
    required super.sliceCount,
    required this.turnId,
  });

  final String turnId;

  @override
  TaskExecutorContinuationStateKind get kind =>
      TaskExecutorContinuationStateKind.plannerWakePending;

  @override
  String get sourceTurnId => turnId;
}

final class NeedsAttentionTaskExecutorContinuationView
    extends TaskExecutorContinuationView {
  const NeedsAttentionTaskExecutorContinuationView({
    required super.revision,
    required super.sliceCount,
    required this.turnId,
    required this.attentionDetail,
  });

  final String turnId;
  final String attentionDetail;

  @override
  TaskExecutorContinuationStateKind get kind =>
      TaskExecutorContinuationStateKind.needsAttention;

  @override
  String get sourceTurnId => turnId;

  @override
  String get detail => attentionDetail;
}

class TaskWorkUnitView {
  const TaskWorkUnitView({
    required this.id,
    required this.title,
    required this.attempt,
    required this.supersedesWorkUnitId,
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
  final int attempt;
  final String? supersedesWorkUnitId;
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
  String get executionStatus => switch (state) {
    PendingTaskWorkUnitStateView() => 'queued',
    RunningTaskWorkUnitStateView() => 'running',
    WaitingReviewTaskWorkUnitStateView(
      phase: AwaitingReportTaskWaitingReviewView(
        outcome: FailedTaskExecutorOutcomeView(),
      ),
    ) =>
      'failed',
    WaitingReviewTaskWorkUnitStateView() ||
    ReviewPassedTaskWorkUnitStateView() ||
    ChangesRequiredTaskWorkUnitStateView() ||
    CompletedTaskWorkUnitStateView() => 'completed',
    PausedTaskWorkUnitStateView(reason: BudgetTaskWorkUnitPauseReasonView()) =>
      'budgetLimited',
    PausedTaskWorkUnitStateView() => 'paused',
    FailedTaskWorkUnitStateView() => 'failed',
    CancelledTaskWorkUnitStateView() => 'cancelled',
  };
  String? get executionError => switch (state) {
    WaitingReviewTaskWorkUnitStateView(
      phase: AwaitingReportTaskWaitingReviewView(
        outcome: FailedTaskExecutorOutcomeView(:final detail),
      ),
    ) =>
      detail,
    PausedTaskWorkUnitStateView(
      reason: OperationalTaskWorkUnitPauseReasonView(:final detail),
    ) =>
      detail,
    PausedTaskWorkUnitStateView(
      continuation: NeedsAttentionTaskExecutorContinuationView(:final detail),
    ) =>
      detail,
    FailedTaskWorkUnitStateView(
      failure: SpawnTaskWorkUnitFailureView(:final failure),
    ) =>
      failure.message,
    FailedTaskWorkUnitStateView(
      failure: ExecutionTaskWorkUnitFailureView(:final detail),
    ) =>
      detail,
    CancelledTaskWorkUnitStateView(:final reason) => reason,
    _ => null,
  };
  TaskExecutorContinuationView? get _continuation => switch (state) {
    RunningTaskWorkUnitStateView(:final continuation) ||
    PausedTaskWorkUnitStateView(:final continuation) => continuation,
    WaitingReviewTaskWorkUnitStateView(
      phase: AwaitingReportTaskWaitingReviewView(:final continuation),
    ) =>
      continuation,
    _ => null,
  };
  TaskBudgetLimitView? get budgetLimit => switch (state) {
    PausedTaskWorkUnitStateView(
      reason: BudgetTaskWorkUnitPauseReasonView(:final limit),
    ) =>
      limit,
    _ => _continuation?.budgetLimit,
  };
  int get budgetSliceCount => switch (state) {
    ChangesRequiredTaskWorkUnitStateView(:final sliceCount) => sliceCount,
    _ => _continuation?.sliceCount ?? 1,
  };
  String get continuationState =>
      _continuation?.kind.name ?? TaskExecutorContinuationStateKind.idle.name;
  String? get continuationSourceTurnId => _continuation?.sourceTurnId;
  BigInt get continuationRevision => switch (state) {
    ChangesRequiredTaskWorkUnitStateView(:final continuationRevision) =>
      continuationRevision,
    _ => _continuation?.revision ?? BigInt.zero,
  };
}

class TaskBudgetLimitView {
  const TaskBudgetLimitView({required this.kind, required this.usage});

  final TaskBudgetLimitKindView kind;
  final TaskBudgetUsageView usage;
}

enum TaskBudgetLimitKindView {
  modelStep,
  toolCall,
  wait,
  wallClock,
  agentCount,
  agentDepth,
  finalization,
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
    required this.content,
    required this.state,
    required this.stateRevision,
    required this.baseCommit,
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
  final TaskCompletionContentView content;
  final TaskCompletionStateView state;
  final BigInt stateRevision;
  final String baseCommit;
  final String verificationSummary;
  final String worktreePath;
  final String branch;
  final DateTime createdAt;
  final DateTime updatedAt;

  String get kind => content.kind;
  String get status => state.status;
  String? get headCommit => content.headCommit;
  List<String> get changedFiles => content.changedFiles;
}

sealed class TaskCompletionContentView {
  const TaskCompletionContentView();

  String get kind;
  String? get headCommit;
  List<String> get changedFiles;
}

final class DeliveryTaskCompletionView extends TaskCompletionContentView {
  const DeliveryTaskCompletionView({
    required this.headCommit,
    required this.changedFiles,
  });

  @override
  String get kind => 'delivery';

  @override
  final String headCommit;

  @override
  final List<String> changedFiles;
}

final class NoDeliveryTaskCompletionView extends TaskCompletionContentView {
  const NoDeliveryTaskCompletionView();

  @override
  String get kind => 'noDelivery';

  @override
  String? get headCommit => null;

  @override
  List<String> get changedFiles => const [];
}

sealed class TaskCompletionStateView {
  const TaskCompletionStateView();

  String get status;
  String? get reviewRoundId;
  DateTime? get decidedAt;
}

final class ReadyForReviewTaskCompletionView extends TaskCompletionStateView {
  const ReadyForReviewTaskCompletionView();

  @override
  String get status => 'readyForReview';

  @override
  String? get reviewRoundId => null;

  @override
  DateTime? get decidedAt => null;
}

sealed class ReviewedTaskCompletionView extends TaskCompletionStateView {
  const ReviewedTaskCompletionView({
    required this.reviewRoundId,
    required this.decidedAt,
  });

  @override
  final String reviewRoundId;

  @override
  final DateTime decidedAt;
}

final class ChangesRequiredTaskCompletionView
    extends ReviewedTaskCompletionView {
  const ChangesRequiredTaskCompletionView({
    required super.reviewRoundId,
    required super.decidedAt,
  });

  @override
  String get status => 'changesRequired';
}

final class ApprovedTaskCompletionView extends ReviewedTaskCompletionView {
  const ApprovedTaskCompletionView({
    required super.reviewRoundId,
    required super.decidedAt,
  });

  @override
  String get status => 'approved';
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
    required this.cleanup,
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
  final TaskMergeMethodView method;
  final String summary;
  final MergeCleanupStateView cleanup;
  final DateTime createdAt;
  final DateTime updatedAt;

  String get cleanupStatus => cleanup.kind.name;
  String? get cleanupDetail => switch (cleanup) {
    FailedMergeCleanupView(:final detail) => detail,
    PendingMergeCleanupView() ||
    DeferredMergeCleanupView() ||
    AttemptingMergeCleanupView() ||
    DiscardedMergeCleanupView() ||
    AlreadyAbsentMergeCleanupView() => null,
  };
}

enum TaskMergeMethodView { merge, cherryPick, squash, rebase, manual }

enum MergeCleanupStateKind {
  pending,
  deferred,
  attempting,
  discarded,
  alreadyAbsent,
  failed,
}

sealed class MergeCleanupStateView {
  const MergeCleanupStateView();

  MergeCleanupStateKind get kind;
}

final class PendingMergeCleanupView extends MergeCleanupStateView {
  const PendingMergeCleanupView();
  @override
  MergeCleanupStateKind get kind => MergeCleanupStateKind.pending;
}

final class DeferredMergeCleanupView extends MergeCleanupStateView {
  const DeferredMergeCleanupView();
  @override
  MergeCleanupStateKind get kind => MergeCleanupStateKind.deferred;
}

final class AttemptingMergeCleanupView extends MergeCleanupStateView {
  const AttemptingMergeCleanupView(this.operationId, this.startedAt);
  final String operationId;
  final DateTime startedAt;
  @override
  MergeCleanupStateKind get kind => MergeCleanupStateKind.attempting;
}

sealed class CompletedMergeCleanupView extends MergeCleanupStateView {
  const CompletedMergeCleanupView(this.operationId, this.completedAt);
  final String operationId;
  final DateTime completedAt;
}

final class DiscardedMergeCleanupView extends CompletedMergeCleanupView {
  const DiscardedMergeCleanupView(super.operationId, super.completedAt);
  @override
  MergeCleanupStateKind get kind => MergeCleanupStateKind.discarded;
}

final class AlreadyAbsentMergeCleanupView extends CompletedMergeCleanupView {
  const AlreadyAbsentMergeCleanupView(super.operationId, super.completedAt);
  @override
  MergeCleanupStateKind get kind => MergeCleanupStateKind.alreadyAbsent;
}

final class FailedMergeCleanupView extends MergeCleanupStateView {
  const FailedMergeCleanupView({
    required this.operationId,
    required this.failedAt,
    required this.detail,
  });
  final String operationId;
  final DateTime failedAt;
  final String detail;
  @override
  MergeCleanupStateKind get kind => MergeCleanupStateKind.failed;
}

enum TaskReviewStateKind {
  pendingDispatch,
  dispatched,
  running,
  passed,
  changesRequired,
  blocked,
  failed,
  cancelled,
}

sealed class TaskReviewStateView {
  const TaskReviewStateView();

  TaskReviewStateKind get kind;
  String? get reviewerAgentId;
  String? get summary => null;
  String? get error => null;
}

final class PendingTaskReviewDispatchView extends TaskReviewStateView {
  const PendingTaskReviewDispatchView();

  @override
  TaskReviewStateKind get kind => TaskReviewStateKind.pendingDispatch;
  @override
  String? get reviewerAgentId => null;
}

final class DispatchedTaskReviewView extends TaskReviewStateView {
  const DispatchedTaskReviewView(this.reviewerAgentId);

  @override
  final String reviewerAgentId;
  @override
  TaskReviewStateKind get kind => TaskReviewStateKind.dispatched;
}

final class RunningTaskReviewView extends TaskReviewStateView {
  const RunningTaskReviewView(this.reviewerAgentId);

  @override
  final String reviewerAgentId;
  @override
  TaskReviewStateKind get kind => TaskReviewStateKind.running;
}

sealed class CompletedTaskReviewView extends TaskReviewStateView {
  const CompletedTaskReviewView(this.reviewerAgentId, this.summary);

  @override
  final String reviewerAgentId;
  @override
  final String summary;
}

final class PassedTaskReviewView extends CompletedTaskReviewView {
  const PassedTaskReviewView(super.reviewerAgentId, super.summary);

  @override
  TaskReviewStateKind get kind => TaskReviewStateKind.passed;
}

final class ChangesRequiredTaskReviewView extends CompletedTaskReviewView {
  const ChangesRequiredTaskReviewView(super.reviewerAgentId, super.summary);

  @override
  TaskReviewStateKind get kind => TaskReviewStateKind.changesRequired;
}

final class BlockedTaskReviewView extends CompletedTaskReviewView {
  const BlockedTaskReviewView(super.reviewerAgentId, super.summary);

  @override
  TaskReviewStateKind get kind => TaskReviewStateKind.blocked;
}

final class FailedTaskReviewView extends TaskReviewStateView {
  const FailedTaskReviewView({
    required this.reviewerAgentId,
    required this.failure,
    required this.failureSummary,
  });

  @override
  final String? reviewerAgentId;
  final String failure;
  final String failureSummary;
  @override
  TaskReviewStateKind get kind => TaskReviewStateKind.failed;
  @override
  String get error => failure;
  @override
  String get summary => failureSummary;
}

final class CancelledTaskReviewView extends TaskReviewStateView {
  const CancelledTaskReviewView({
    required this.reviewerAgentId,
    required this.reason,
    required this.cancellationSummary,
  });

  @override
  final String? reviewerAgentId;
  final String reason;
  final String cancellationSummary;
  @override
  TaskReviewStateKind get kind => TaskReviewStateKind.cancelled;
  @override
  String get error => reason;
  @override
  String get summary => cancellationSummary;
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
    required this.designReferences,
    required this.findings,
    required this.createdAt,
    required this.updatedAt,
  });

  final String id;
  final int round;
  final TaskReviewScopeView scope;
  final String? workUnitId;
  final String? completionId;
  final int? completionRevision;
  final String reviewedHead;
  final TaskReviewStateView state;
  final String requestedByCallId;
  final List<TaskDesignReferenceView> designReferences;
  final List<TaskReviewFindingView> findings;
  final DateTime createdAt;
  final DateTime updatedAt;

  String get verdict => state.kind.name;
  String? get reviewerAgentId => state.reviewerAgentId;
  String? get summary => state.summary;
}

enum TaskReviewScopeView { delivery, integrated }

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
