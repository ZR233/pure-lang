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

class TaskRuntimeView {
  const TaskRuntimeView({
    required this.runId,
    required this.phase,
    required this.branch,
    required this.expectedHead,
    required this.statusMessage,
    required this.stopRequestedOrigin,
    required this.stopRequestedReason,
    required this.taskGeneration,
    this.failures = const [],
    this.terminalFailure,
    required this.workUnits,
    required this.completions,
    required this.merges,
    required this.reviews,
  });

  final String runId;
  final String phase;
  final String branch;
  final String expectedHead;
  final String? statusMessage;
  final String? stopRequestedOrigin;
  final String? stopRequestedReason;
  final int taskGeneration;
  final List<TaskFailureView> failures;
  final TaskFailureView? terminalFailure;
  final List<TaskWorkUnitView> workUnits;
  final List<TaskCompletionView> completions;
  final List<TaskMergeView> merges;
  final List<TaskReviewView> reviews;

  bool get isActive =>
      !const {'completed', 'blocked', 'failed', 'cancelled'}.contains(phase);

  bool get hasRecoverableExecutorFailure {
    if (!const {
      'planning',
      'pendingConfirmation',
      'designUpdating',
      'implementing',
      'reworking',
    }.contains(phase)) {
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

class TaskWorkUnitView {
  const TaskWorkUnitView({
    required this.id,
    required this.title,
    required this.status,
    required this.worktreePath,
    required this.branch,
    required this.agentId,
    required this.executionStatus,
    required this.executionError,
    required this.budgetLimit,
    required this.budgetSliceCount,
    required this.budgetSliceLimit,
    required this.continuationState,
    required this.continuationSourceTurnId,
    required this.continuationRevision,
    required this.executorProgressRevision,
  });

  final String id;
  final String title;
  final String status;
  final String worktreePath;
  final String branch;
  final String? agentId;
  final String executionStatus;
  final String? executionError;
  final TaskBudgetLimitView? budgetLimit;
  final int budgetSliceCount;
  final int budgetSliceLimit;
  final String continuationState;
  final String? continuationSourceTurnId;
  final BigInt continuationRevision;
  final BigInt executorProgressRevision;
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

class TaskReviewView {
  const TaskReviewView({
    required this.id,
    required this.round,
    required this.scope,
    required this.workUnitId,
    required this.completionId,
    required this.completionRevision,
    required this.reviewedHead,
    required this.verdict,
    required this.requestedByCallId,
    required this.reviewerAgentId,
    required this.summary,
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
  final String verdict;
  final String requestedByCallId;
  final String? reviewerAgentId;
  final String? summary;
  final List<TaskDesignReferenceView> designReferences;
  final List<TaskReviewFindingView> findings;
  final DateTime createdAt;
  final DateTime updatedAt;
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
