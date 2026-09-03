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

/// 费用金额显示：四舍五入并固定保留 2 位小数。
String formatRuntimeCostAmount(String currency, double amount) {
  final rounded = amount.toStringAsFixed(2);
  final display = rounded == '-0.00' ? '0.00' : rounded;
  final symbol = _runtimeCurrencySymbols[currency.toUpperCase()];
  if (symbol != null) {
    return '$symbol$display';
  }
  return '$currency $display'.trim();
}

/// 多币种实际花费合并显示，如 `￥1.20 + $2.60`；不做汇率换算。
String formatRuntimeCosts(Iterable<RuntimeCostView> costs) {
  return costs
      .map((cost) => formatRuntimeCostAmount(cost.currency, cost.amount))
      .where((label) => label.isNotEmpty)
      .join(' + ');
}

String formatTokenThroughput(double? tokensPerSecond) {
  if (tokensPerSecond == null || !tokensPerSecond.isFinite) return '- t/s';
  final value = tokensPerSecond >= 10
      ? tokensPerSecond.round().toString()
      : tokensPerSecond.toStringAsFixed(1);
  return '$value t/s';
}

class SessionCostView {
  const SessionCostView({
    required this.rootThreadId,
    required this.estimatedCosts,
    required this.hasUnpricedUsage,
  });

  final String rootThreadId;
  final List<RuntimeCostView> estimatedCosts;
  final bool hasUnpricedUsage;

  String get label {
    final value = formatRuntimeCosts(estimatedCosts);
    return value.isEmpty ? '-' : value;
  }
}

class ModelPerformanceSummaryView {
  const ModelPerformanceSummaryView({
    required this.providerInstanceId,
    required this.providerDisplayName,
    required this.model,
    required this.sampleCount,
    required this.completionTokens,
    required this.totalTtftMillis,
    required this.totalDecodeMillis,
    required this.totalResponseMillis,
    required this.tokensPerSecond,
    required this.averageTtftMillis,
    required this.averageResponseMillis,
  });

  final String providerInstanceId;
  final String providerDisplayName;
  final String model;
  final int sampleCount;
  final int completionTokens;
  final int totalTtftMillis;
  final int totalDecodeMillis;
  final int totalResponseMillis;
  final double tokensPerSecond;
  final double averageTtftMillis;
  final double averageResponseMillis;

  String get filterKey => '$providerInstanceId\u0000$model';
}

class ModelPerformanceSampleView {
  const ModelPerformanceSampleView({
    required this.completedAt,
    required this.providerInstanceId,
    required this.providerDisplayName,
    required this.model,
    required this.completionTokens,
    required this.ttftMillis,
    required this.decodeMillis,
    required this.totalResponseMillis,
    required this.tokensPerSecond,
  });

  final DateTime completedAt;
  final String providerInstanceId;
  final String providerDisplayName;
  final String model;
  final int completionTokens;
  final int ttftMillis;
  final int decodeMillis;
  final int totalResponseMillis;
  final double tokensPerSecond;

  String get filterKey => '$providerInstanceId\u0000$model';
}

class ModelPerformanceSnapshotView {
  const ModelPerformanceSnapshotView({
    this.revision = 0,
    this.updatedAt,
    this.sessionCosts = const [],
    this.summaries = const [],
    this.history = const [],
  });

  final int revision;
  final DateTime? updatedAt;
  final List<SessionCostView> sessionCosts;
  final List<ModelPerformanceSummaryView> summaries;
  final List<ModelPerformanceSampleView> history;

  SessionCostView? sessionCost(String? rootThreadId) {
    if (rootThreadId == null) return null;
    for (final cost in sessionCosts) {
      if (cost.rootThreadId == rootThreadId) return cost;
    }
    return null;
  }
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
    this.turnCompletionTokens = 0,
    this.turnDecodeMillis = 0,
    this.workflow,
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
  final int turnCompletionTokens;
  final int turnDecodeMillis;
  final WorkflowRuntimeView? workflow;

  bool get hasActiveWorkflow => workflow?.isActive ?? false;
  bool get hasUsage =>
      inferenceCount > 0 || promptTokens > 0 || completionTokens > 0;
  double? get turnTokensPerSecond => turnDecodeMillis > 0
      ? turnCompletionTokens * 1000 / turnDecodeMillis
      : null;
  String get turnThroughputLabel => formatTokenThroughput(turnTokensPerSecond);

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
            turnCompletionTokens == other.turnCompletionTokens &&
            turnDecodeMillis == other.turnDecodeMillis &&
            workflow == other.workflow;
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
    turnCompletionTokens,
    turnDecodeMillis,
    workflow,
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
    int? turnCompletionTokens,
    int? turnDecodeMillis,
    WorkflowRuntimeView? workflow,
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
      turnCompletionTokens: turnCompletionTokens ?? this.turnCompletionTokens,
      turnDecodeMillis: turnDecodeMillis ?? this.turnDecodeMillis,
      workflow: workflow ?? this.workflow,
    );
  }
}

class WorkflowRuntimeView {
  const WorkflowRuntimeView({required this.revision, this.currentRun});

  final int revision;
  final WorkflowRunView? currentRun;

  bool get isActive => currentRun?.terminal == false;

  @override
  bool operator ==(Object other) =>
      other is WorkflowRuntimeView &&
      revision == other.revision &&
      currentRun == other.currentRun;

  @override
  int get hashCode => Object.hash(revision, currentRun);
}

class WorkflowRunView {
  const WorkflowRunView({
    required this.lineageId,
    required this.runId,
    required this.modeId,
    required this.graphRevision,
    required this.graphHash,
    required this.currentStateId,
    required this.terminal,
    required this.startedAt,
    required this.updatedAt,
  });

  final String lineageId;
  final String runId;
  final String modeId;
  final int graphRevision;
  final String graphHash;
  final String currentStateId;
  final bool terminal;
  final DateTime startedAt;
  final DateTime updatedAt;

  @override
  bool operator ==(Object other) =>
      other is WorkflowRunView &&
      runId == other.runId &&
      currentStateId == other.currentStateId &&
      updatedAt == other.updatedAt;

  @override
  int get hashCode => Object.hash(runId, currentStateId, updatedAt);
}
