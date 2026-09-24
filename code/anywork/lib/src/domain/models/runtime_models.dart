import 'dart:convert';

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

/// 只格式化吞吐数值；单位由展示层按 locale 追加，避免在领域模型内写死文案。
String? formatTokenThroughputValue(double? tokensPerSecond) {
  if (tokensPerSecond == null || !tokensPerSecond.isFinite) return null;
  return tokensPerSecond >= 10
      ? tokensPerSecond.round().toString()
      : tokensPerSecond.toStringAsFixed(1);
}

class PurposeCostView {
  const PurposeCostView({
    this.purpose,
    required this.estimatedCosts,
    required this.hasUnpricedUsage,
  });

  final String? purpose;
  final List<RuntimeCostView> estimatedCosts;
  final bool hasUnpricedUsage;

  String get label {
    final value = formatRuntimeCosts(estimatedCosts);
    return value.isEmpty ? '-' : value;
  }
}

class SessionCostView {
  const SessionCostView({
    this.purposeCosts = const [],
    required this.rootThreadId,
    required this.estimatedCosts,
    required this.hasUnpricedUsage,
  });

  final String rootThreadId;
  final List<PurposeCostView> purposeCosts;
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
    this.reasoningEffort,
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
  final String? reasoningEffort;
  final int sampleCount;
  final int completionTokens;
  final int totalTtftMillis;
  final int totalDecodeMillis;
  final int totalResponseMillis;
  final double tokensPerSecond;
  final double averageTtftMillis;
  final double averageResponseMillis;

  String get filterKey =>
      jsonEncode(<Object?>[providerInstanceId, model, reasoningEffort]);
}

enum ModelMatchState { matched, mismatched, unreported, legacyUnknown }

class ModelPerformanceSampleView {
  const ModelPerformanceSampleView({
    required this.completedAt,
    required this.providerInstanceId,
    required this.providerDisplayName,
    required this.model,
    this.configuredModel,
    this.sentModel,
    this.reportedModel,
    this.modelMatchState = ModelMatchState.legacyUnknown,
    this.reasoningEffort,
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
  final String? configuredModel;
  final String? sentModel;
  final String? reportedModel;
  final ModelMatchState modelMatchState;
  final String? reasoningEffort;
  final int completionTokens;
  final int? ttftMillis;
  final int? decodeMillis;
  final int? totalResponseMillis;
  final double? tokensPerSecond;

  String get filterKey =>
      jsonEncode(<Object?>[providerInstanceId, model, reasoningEffort]);

  String get displayModel => sentModel ?? model;
}

class ModelPerformanceSnapshotView {
  const ModelPerformanceSnapshotView({
    this.revision = 0,
    this.updatedAt,
    this.sessionCosts = const [],
    this.summaries = const [],
    this.history = const [],
    this.statisticsPending = false,
    this.statisticsGap = false,
    this.readFailed = false,
  });

  final int revision;
  final DateTime? updatedAt;
  final List<SessionCostView> sessionCosts;
  final List<ModelPerformanceSummaryView> summaries;
  final List<ModelPerformanceSampleView> history;
  final bool statisticsPending;
  final bool statisticsGap;
  final bool readFailed;

  SessionCostView? sessionCost(String? rootThreadId) {
    if (rootThreadId == null) return null;
    for (final cost in sessionCosts) {
      if (cost.rootThreadId == rootThreadId) return cost;
    }
    return null;
  }
}

/// 由已报告输入样本累计出的 prompt cache 有效用量。
///
/// 只有 input 与 cache read 同时报告、且 cache read（连同可选的 cache write）
/// 不超过 input 的样本才计入累计；缺失或矛盾的样本只置 [hasIncompleteUsage]。
/// 命中率分母是累计 input，因为它已经包含 cache read。
class CacheUsageView {
  const CacheUsageView({
    this.inputTokens = 0,
    this.cacheReadTokens = 0,
    this.hitRate,
    this.hasIncompleteUsage = false,
  });

  /// 计入累计的有效样本 input 总量，已包含 cache read。
  final int inputTokens;

  /// 计入累计的有效样本 cache read 总量。
  final int cacheReadTokens;

  /// `cacheReadTokens / inputTokens`；累计 input 为零时为 null。
  final double? hitRate;

  /// 是否存在被排除在累计之外的缺失或矛盾样本。
  final bool hasIncompleteUsage;

  /// 累计分母为正时命中率才有意义。
  bool get hasPositiveDenominator => inputTokens > 0;

  /// 命中率同口径的未命中 token；有效样本保证不出现负值。
  int get missTokens => inputTokens - cacheReadTokens;

  @override
  bool operator ==(Object other) =>
      other is CacheUsageView &&
      inputTokens == other.inputTokens &&
      cacheReadTokens == other.cacheReadTokens &&
      hitRate == other.hitRate &&
      hasIncompleteUsage == other.hasIncompleteUsage;

  @override
  int get hashCode =>
      Object.hash(inputTokens, cacheReadTokens, hitRate, hasIncompleteUsage);
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
    this.reasoningTokens = 0,
    this.inferenceCount = 0,
    this.cacheUsage = const CacheUsageView(),
    this.estimatedCosts = const [],
    this.estimatedCacheSavings = const [],
    this.hasUnpricedUsage = false,
    this.hasIncompleteUsage = false,
    this.promptGeneration,
    this.promptCachePolicy,
    this.prefixChangedReason,
    this.turnCompletionTokens = 0,
    this.turnDecodeMillis = 0,
    this.workflow,
    this.modelRoute,
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
  final int reasoningTokens;
  final int inferenceCount;
  final CacheUsageView cacheUsage;
  final List<RuntimeCostView> estimatedCosts;
  final List<RuntimeCostView> estimatedCacheSavings;
  final bool hasUnpricedUsage;
  final bool hasIncompleteUsage;
  final int? promptGeneration;
  final String? promptCachePolicy;
  final String? prefixChangedReason;
  final int turnCompletionTokens;
  final int turnDecodeMillis;
  final WorkflowRuntimeView? workflow;
  final ThreadModelRouteView? modelRoute;

  bool get hasActiveWorkflow => workflow?.isActive ?? false;
  bool get hasUsage =>
      inferenceCount > 0 || promptTokens > 0 || completionTokens > 0;
  double? get turnTokensPerSecond => turnDecodeMillis > 0
      ? turnCompletionTokens * 1000 / turnDecodeMillis
      : null;

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
            reasoningTokens == other.reasoningTokens &&
            inferenceCount == other.inferenceCount &&
            cacheUsage == other.cacheUsage &&
            listEquals(estimatedCosts, other.estimatedCosts) &&
            listEquals(estimatedCacheSavings, other.estimatedCacheSavings) &&
            hasUnpricedUsage == other.hasUnpricedUsage &&
            hasIncompleteUsage == other.hasIncompleteUsage &&
            promptGeneration == other.promptGeneration &&
            promptCachePolicy == other.promptCachePolicy &&
            prefixChangedReason == other.prefixChangedReason &&
            turnCompletionTokens == other.turnCompletionTokens &&
            turnDecodeMillis == other.turnDecodeMillis &&
            workflow == other.workflow &&
            modelRoute == other.modelRoute;
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
    reasoningTokens,
    inferenceCount,
    cacheUsage,
    Object.hashAll(estimatedCosts),
    Object.hashAll(estimatedCacheSavings),
    hasUnpricedUsage,
    hasIncompleteUsage,
    promptGeneration,
    promptCachePolicy,
    prefixChangedReason,
    turnCompletionTokens,
    turnDecodeMillis,
    workflow,
    modelRoute,
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
    int? reasoningTokens,
    int? inferenceCount,
    CacheUsageView? cacheUsage,
    List<RuntimeCostView>? estimatedCosts,
    List<RuntimeCostView>? estimatedCacheSavings,
    bool? hasUnpricedUsage,
    bool? hasIncompleteUsage,
    int? promptGeneration,
    String? promptCachePolicy,
    String? prefixChangedReason,
    int? turnCompletionTokens,
    int? turnDecodeMillis,
    WorkflowRuntimeView? workflow,
    ThreadModelRouteView? modelRoute,
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
      reasoningTokens: reasoningTokens ?? this.reasoningTokens,
      inferenceCount: inferenceCount ?? this.inferenceCount,
      cacheUsage: cacheUsage ?? this.cacheUsage,
      estimatedCosts: estimatedCosts ?? this.estimatedCosts,
      estimatedCacheSavings:
          estimatedCacheSavings ?? this.estimatedCacheSavings,
      hasUnpricedUsage: hasUnpricedUsage ?? this.hasUnpricedUsage,
      hasIncompleteUsage: hasIncompleteUsage ?? this.hasIncompleteUsage,
      promptGeneration: promptGeneration ?? this.promptGeneration,
      promptCachePolicy: promptCachePolicy ?? this.promptCachePolicy,
      prefixChangedReason: prefixChangedReason ?? this.prefixChangedReason,
      turnCompletionTokens: turnCompletionTokens ?? this.turnCompletionTokens,
      turnDecodeMillis: turnDecodeMillis ?? this.turnDecodeMillis,
      workflow: workflow ?? this.workflow,
      modelRoute: modelRoute ?? this.modelRoute,
    );
  }
}

class ThreadModelRouteView {
  const ThreadModelRouteView({
    required this.providerId,
    required this.model,
    required this.effort,
    required this.revision,
    required this.available,
    this.unavailableReason,
  });

  final String providerId;
  final String model;
  final String? effort;
  final int revision;
  final bool available;
  final String? unavailableReason;

  @override
  bool operator ==(Object other) =>
      other is ThreadModelRouteView &&
      providerId == other.providerId &&
      model == other.model &&
      effort == other.effort &&
      revision == other.revision &&
      available == other.available &&
      unavailableReason == other.unavailableReason;

  @override
  int get hashCode => Object.hash(
    providerId,
    model,
    effort,
    revision,
    available,
    unavailableReason,
  );
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
