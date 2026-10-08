import 'package:flutter/foundation.dart' show listEquals;

import 'agent_profile_models.dart';
import 'persistence_models.dart';
import 'runtime_models.dart';
import 'studio_state_snapshots.dart';
import 'thread_directory_models.dart';

/// typed 产品订阅 topic；带 payload 的变体表达作用域，不存在无作用域组合形式。
///
/// 值相等是租约身份：同一 topic（含作用域参数）的多个观察者共享同一个底层订阅，
/// 最后一个观察者释放时才真正取消 Rust 订阅。
sealed class ProductTopic {
  const ProductTopic();
}

final class ProjectDirectoryTopic extends ProductTopic {
  const ProjectDirectoryTopic();

  @override
  bool operator ==(Object other) => other is ProjectDirectoryTopic;

  @override
  int get hashCode => 0x0101;
}

final class ThreadDirectoryTopic extends ProductTopic {
  const ThreadDirectoryTopic();

  @override
  bool operator ==(Object other) => other is ThreadDirectoryTopic;

  @override
  int get hashCode => 0x0102;
}

final class AgentDirectoryTopic extends ProductTopic {
  const AgentDirectoryTopic();

  @override
  bool operator ==(Object other) => other is AgentDirectoryTopic;

  @override
  int get hashCode => 0x0103;
}

final class SettingsTopic extends ProductTopic {
  const SettingsTopic();

  @override
  bool operator ==(Object other) => other is SettingsTopic;

  @override
  int get hashCode => 0x0104;
}

final class RecoveryTopic extends ProductTopic {
  const RecoveryTopic();

  @override
  bool operator ==(Object other) => other is RecoveryTopic;

  @override
  int get hashCode => 0x0105;
}

final class McpTopic extends ProductTopic {
  const McpTopic();

  @override
  bool operator ==(Object other) => other is McpTopic;

  @override
  int get hashCode => 0x0106;
}

final class LspTopic extends ProductTopic {
  const LspTopic();

  @override
  bool operator ==(Object other) => other is LspTopic;

  @override
  int get hashCode => 0x0107;
}

final class SkillsTopic extends ProductTopic {
  const SkillsTopic({required this.projectId});

  final String projectId;

  @override
  bool operator ==(Object other) =>
      other is SkillsTopic && projectId == other.projectId;

  @override
  int get hashCode => Object.hash(0x0108, projectId);
}

final class ThreadModeCatalogTopic extends ProductTopic {
  const ThreadModeCatalogTopic();

  @override
  bool operator ==(Object other) => other is ThreadModeCatalogTopic;

  @override
  int get hashCode => 0x0109;
}

final class ProviderUsageTopic extends ProductTopic {
  const ProviderUsageTopic();

  @override
  bool operator ==(Object other) => other is ProviderUsageTopic;

  @override
  int get hashCode => 0x0110;
}

final class ModelPerformanceTopic extends ProductTopic {
  const ModelPerformanceTopic();

  @override
  bool operator ==(Object other) => other is ModelPerformanceTopic;

  @override
  int get hashCode => 0x0111;
}

final class SessionCostsTopic extends ProductTopic {
  const SessionCostsTopic({required this.rootThreadId});

  final String rootThreadId;

  @override
  bool operator ==(Object other) =>
      other is SessionCostsTopic && rootThreadId == other.rootThreadId;

  @override
  int get hashCode => Object.hash(0x0112, rootThreadId);
}

final class UpdaterTopic extends ProductTopic {
  const UpdaterTopic();

  @override
  bool operator ==(Object other) => other is UpdaterTopic;

  @override
  int get hashCode => 0x0113;
}

final class PersistenceTopic extends ProductTopic {
  const PersistenceTopic();

  @override
  bool operator ==(Object other) => other is PersistenceTopic;

  @override
  int get hashCode => 0x0114;
}

final class PersistenceQueueTopic extends ProductTopic {
  const PersistenceQueueTopic();

  @override
  bool operator ==(Object other) => other is PersistenceQueueTopic;

  @override
  int get hashCode => 0x0115;
}

final class AgentProfilesTopic extends ProductTopic {
  const AgentProfilesTopic();

  @override
  bool operator ==(Object other) => other is AgentProfilesTopic;

  @override
  int get hashCode => 0x0116;
}

/// 订阅首帧携带的 canonical 基线 payload；每个 topic 只有自己的领域状态。
sealed class ProductTopicBaselineState {
  const ProductTopicBaselineState();
}

final class ProjectDirectoryBaseline extends ProductTopicBaselineState {
  const ProjectDirectoryBaseline(this.state);

  final ProjectDirectoryState state;
}

/// Thread directory 基线是一个分页首页（含 cursor）；领域 revision 由帧携带，
/// 不用 delta 冒充全量，也不清空已加载的更远页。
final class ThreadDirectoryBaseline extends ProductTopicBaselineState {
  const ThreadDirectoryBaseline(this.page);

  final ThreadDirectoryPage page;
}

final class AgentDirectoryBaseline extends ProductTopicBaselineState {
  const AgentDirectoryBaseline(this.state);

  final AgentDirectoryState state;
}

final class SettingsBaseline extends ProductTopicBaselineState {
  const SettingsBaseline(this.state);

  final SettingsStateSnapshot state;
}

final class RecoveryBaseline extends ProductTopicBaselineState {
  const RecoveryBaseline(this.state);

  final RecoveryStateSnapshot state;
}

final class McpBaseline extends ProductTopicBaselineState {
  const McpBaseline(this.state);

  final McpStateSnapshot state;
}

final class LspBaseline extends ProductTopicBaselineState {
  const LspBaseline(this.state);

  final LspStateSnapshot state;
}

final class SkillsBaseline extends ProductTopicBaselineState {
  const SkillsBaseline(this.state);

  final SkillsStateSnapshot state;
}

final class ThreadModeCatalogBaseline extends ProductTopicBaselineState {
  const ThreadModeCatalogBaseline(this.state);

  final ThreadModeCatalogView state;
}

final class ProviderUsageBaseline extends ProductTopicBaselineState {
  const ProviderUsageBaseline(this.state);

  final ProviderUsageStateSnapshot state;
}

final class ModelPerformanceBaseline extends ProductTopicBaselineState {
  const ModelPerformanceBaseline(this.state);

  final ModelPerformanceSnapshotView state;
}

final class SessionCostsBaseline extends ProductTopicBaselineState {
  const SessionCostsBaseline(this.state);

  final SessionCostsStateView state;
}

final class UpdaterBaseline extends ProductTopicBaselineState {
  const UpdaterBaseline(this.state);

  final UpdaterStateSnapshot state;
}

final class PersistenceBaseline extends ProductTopicBaselineState {
  const PersistenceBaseline(this.state);

  final PersistenceStateSnapshot state;
}

final class PersistenceQueueBaseline extends ProductTopicBaselineState {
  const PersistenceQueueBaseline(this.state);

  final PersistenceQueueStateView state;
}

final class AgentProfilesBaseline extends ProductTopicBaselineState {
  const AgentProfilesBaseline(this.state);

  final AgentProfilesStateView state;
}

/// 单个 topic 订阅交付的帧。
sealed class ProductTopicFrame {
  const ProductTopicFrame();
}

/// 首帧：当前领域基线，明确 topic 与领域 revision。
final class ProductTopicBaselineFrame extends ProductTopicFrame {
  const ProductTopicBaselineFrame({
    required this.topic,
    required this.revision,
    required this.state,
  });

  final ProductTopic topic;
  final int revision;
  final ProductTopicBaselineState state;
}

/// 后续 owner 事实；payload 携带领域 revision，旧事实由消费者拒绝。
final class ProductTopicDataFrame extends ProductTopicFrame {
  const ProductTopicDataFrame({required this.topic, required this.event});

  final ProductTopic topic;
  final ProductTopicEventEnvelope event;
}

/// 无法证明该 topic 增量连续：只重读该 topic 基线，无 durable replay。
final class ProductTopicLaggedFrame extends ProductTopicFrame {
  const ProductTopicLaggedFrame({required this.topic, required this.dropped});

  final ProductTopic topic;
  final int dropped;
}

/// 单个 topic 的订阅失败；只影响该领域，其他 topic 订阅保持不变。
final class ProductTopicFailureFrame extends ProductTopicFrame {
  const ProductTopicFailureFrame({required this.topic, required this.error});

  final ProductTopic topic;
  final Object error;
}

/// topic 订阅的传输连接阶段（transport，不是业务 snapshot）。
///
/// [connected] 表示已收到该 topic 的 Baseline/Data；[reconnecting] 表示发生
/// Lagged/Failure/closed 后仍在有界重连；[failed] 表示重连预算耗尽、需要显式
/// [connected] 之外的显式 retry 才能恢复。业务内容始终保留最后有效值，连接状态
/// 只表达局部传输事实。
enum ProductTopicConnectionPhase { connecting, connected, reconnecting, failed }

/// 单个 topic 的局部传输连接状态；值相等，只在真实变化时更新。
///
/// 该类型只描述订阅通道本身，不携带或替代任何业务 snapshot：Failure/closed 时
/// canonical 领域数据保持 last-known value，UI 通过此状态呈现局部进度/错误。
class ProductTopicConnectionStateView {
  const ProductTopicConnectionStateView({
    required this.phase,
    this.errorMessage,
    this.retryExhausted = false,
  });

  final ProductTopicConnectionPhase phase;

  /// 最近一次局部失败/关闭的可读原因；无错误时为 null。
  final String? errorMessage;

  /// 重连预算耗尽：需要显式 retry，不再自动重连。
  final bool retryExhausted;

  bool get isConnected => phase == ProductTopicConnectionPhase.connected;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ProductTopicConnectionStateView &&
          phase == other.phase &&
          errorMessage == other.errorMessage &&
          retryExhausted == other.retryExhausted;

  @override
  int get hashCode => Object.hash(phase, errorMessage, retryExhausted);
}

/// 产品事件信封；`sequence` 只用于传输观测，领域顺序由 payload revision 决定。
class ProductTopicEventEnvelope {
  const ProductTopicEventEnvelope({
    required this.eventId,
    required this.sequence,
    required this.createdAt,
    required this.payload,
  });

  final String eventId;
  final BigInt sequence;
  final DateTime? createdAt;
  final ProductTopicEventPayload payload;
}

/// 产品事件 payload；每个 payload 只属于一个 topic。
sealed class ProductTopicEventPayload {
  const ProductTopicEventPayload();

  /// 该事件所属的 topic；分发与租约共享都以此为准。
  ProductTopic get topic => switch (this) {
    ProjectDirectoryChangedPayload() => const ProjectDirectoryTopic(),
    ThreadDirectoryChangedPayload() => const ThreadDirectoryTopic(),
    AgentDirectoryChangedPayload() => const AgentDirectoryTopic(),
    SettingsStateChangedPayload() => const SettingsTopic(),
    RecoveryStateChangedPayload() => const RecoveryTopic(),
    McpStateChangedPayload() => const McpTopic(),
    LspStateChangedPayload() => const LspTopic(),
    SkillsStateChangedPayload(:final state) => SkillsTopic(
      projectId: state.projectId,
    ),
    ThreadModeCatalogChangedPayload() => const ThreadModeCatalogTopic(),
    ProviderUsageStateChangedPayload() => const ProviderUsageTopic(),
    ModelPerformanceStateChangedPayload() => const ModelPerformanceTopic(),
    SessionCostsChangedPayload(:final state) => SessionCostsTopic(
      rootThreadId: state.rootThreadId,
    ),
    UpdaterStateChangedPayload() => const UpdaterTopic(),
    PersistenceStateChangedPayload() => const PersistenceTopic(),
    PersistenceQueueStateChangedPayload() => const PersistenceQueueTopic(),
    AgentProfilesStateChangedPayload() => const AgentProfilesTopic(),
  };
}

final class ProjectDirectoryChangedPayload extends ProductTopicEventPayload {
  const ProjectDirectoryChangedPayload(this.state);

  final ProjectDirectoryState state;
}

/// Thread directory 增量：GUI 按身份合并进分页窗口，未加载条目的增量忽略；
/// `revision` 是目录领域水位，旧增量被消费者拒绝。
final class ThreadDirectoryChangedPayload extends ProductTopicEventPayload {
  const ThreadDirectoryChangedPayload({
    required this.revision,
    required this.upserted,
    required this.removed,
  });

  final int revision;
  final List<StudioThread> upserted;
  final List<String> removed;
}

final class AgentDirectoryChangedPayload extends ProductTopicEventPayload {
  const AgentDirectoryChangedPayload(this.state);

  final AgentDirectoryState state;
}

final class SettingsStateChangedPayload extends ProductTopicEventPayload {
  const SettingsStateChangedPayload(this.state);

  final SettingsStateSnapshot state;
}

final class RecoveryStateChangedPayload extends ProductTopicEventPayload {
  const RecoveryStateChangedPayload(this.state);

  final RecoveryStateSnapshot state;
}

final class McpStateChangedPayload extends ProductTopicEventPayload {
  const McpStateChangedPayload(this.state);

  final McpStateSnapshot state;
}

final class LspStateChangedPayload extends ProductTopicEventPayload {
  const LspStateChangedPayload(this.state);

  final LspStateSnapshot state;
}

final class SkillsStateChangedPayload extends ProductTopicEventPayload {
  const SkillsStateChangedPayload(this.state);

  final SkillsStateSnapshot state;
}

final class ThreadModeCatalogChangedPayload extends ProductTopicEventPayload {
  const ThreadModeCatalogChangedPayload(this.state);

  final ThreadModeCatalogView state;
}

final class ProviderUsageStateChangedPayload extends ProductTopicEventPayload {
  const ProviderUsageStateChangedPayload(this.state);

  final ProviderUsageStateSnapshot state;
}

final class ModelPerformanceStateChangedPayload
    extends ProductTopicEventPayload {
  const ModelPerformanceStateChangedPayload(this.state);

  final ModelPerformanceSnapshotView state;
}

/// 单个 root 会话的作用域费用事实；`cost == null` 是显式清除，不是零费用。
final class SessionCostsChangedPayload extends ProductTopicEventPayload {
  const SessionCostsChangedPayload(this.state);

  final SessionCostsStateView state;
}

final class UpdaterStateChangedPayload extends ProductTopicEventPayload {
  const UpdaterStateChangedPayload(this.state);

  final UpdaterStateSnapshot state;
}

final class PersistenceStateChangedPayload extends ProductTopicEventPayload {
  const PersistenceStateChangedPayload(this.state);

  final PersistenceStateSnapshot state;
}

final class PersistenceQueueStateChangedPayload
    extends ProductTopicEventPayload {
  const PersistenceQueueStateChangedPayload(this.state);

  final PersistenceQueueStateView state;
}

final class AgentProfilesStateChangedPayload extends ProductTopicEventPayload {
  const AgentProfilesStateChangedPayload(this.state);

  final AgentProfilesStateView state;
}

/// 单个 root 会话的作用域费用状态。
///
/// `cost == null` 是显式清除（例如归档后），调用方必须与“零费用”区分展示。
class SessionCostsStateView {
  const SessionCostsStateView({
    required this.rootThreadId,
    required this.revision,
    this.updatedAt,
    this.statisticsPending = false,
    this.statisticsGap = false,
    this.readFailed = false,
    this.cost,
  });

  final String rootThreadId;
  final int revision;
  final DateTime? updatedAt;
  final bool statisticsPending;
  final bool statisticsGap;
  final bool readFailed;
  final SessionCostView? cost;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is SessionCostsStateView &&
          rootThreadId == other.rootThreadId &&
          revision == other.revision &&
          updatedAt == other.updatedAt &&
          statisticsPending == other.statisticsPending &&
          statisticsGap == other.statisticsGap &&
          readFailed == other.readFailed &&
          cost == other.cost;

  @override
  int get hashCode => Object.hash(
    rootThreadId,
    revision,
    updatedAt,
    statisticsPending,
    statisticsGap,
    readFailed,
    cost,
  );
}

/// 持久化队列的 typed 观测：发布 revision + 时间基线 + 协调器真实观测。
///
/// `updatedAt` 是本次发布的时间基线；“最老待保存年龄”由基线与 payload 的现有
/// age 字段共同表达，不驱动每秒事件或轮询。
class PersistenceQueueStateView {
  const PersistenceQueueStateView({
    required this.revision,
    this.updatedAt,
    required this.queue,
  });

  final int revision;
  final DateTime? updatedAt;
  final PersistenceQueueSnapshot queue;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is PersistenceQueueStateView &&
          revision == other.revision &&
          updatedAt == other.updatedAt &&
          queue == other.queue;

  @override
  int get hashCode => Object.hash(revision, updatedAt, queue);
}

/// 单个 Profile 文件的诊断；只排除对应 Profile，不阻断其余配置。
class AgentProfileDiagnosticView {
  const AgentProfileDiagnosticView({required this.path, required this.message});

  final String path;
  final String message;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AgentProfileDiagnosticView &&
          path == other.path &&
          message == other.message;

  @override
  int get hashCode => Object.hash(path, message);
}

/// 配置级 Agent Profiles 的 canonical 数据；与运行期 Agent directory 互不替代。
class AgentProfilesDataView {
  const AgentProfilesDataView({
    this.profiles = const [],
    this.diagnostics = const [],
  });

  final List<AgentProfileView> profiles;
  final List<AgentProfileDiagnosticView> diagnostics;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AgentProfilesDataView &&
          listEquals(profiles, other.profiles) &&
          listEquals(diagnostics, other.diagnostics);

  @override
  int get hashCode => Object.hashAll([...profiles, ...diagnostics]);
}

/// 配置级 Agent Profiles 的 canonical 资源快照。
class AgentProfilesStateView {
  const AgentProfilesStateView({required this.state});

  final ObservedResource<AgentProfilesDataView> state;

  AgentProfilesDataView get data =>
      state.value ?? const AgentProfilesDataView();

  int get revision => state.revision;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AgentProfilesStateView &&
          _agentProfilesResourceEquals(state, other.state);

  @override
  int get hashCode =>
      Object.hash(state.revision, state.value ?? const AgentProfilesDataView());
}

/// `ObservedResource` 泛型没有内建值相等；Profiles 快照的值语义在这里显式展开，
/// 保证同内容快照不触发下游重建。
bool _agentProfilesResourceEquals(
  ObservedResource<AgentProfilesDataView> left,
  ObservedResource<AgentProfilesDataView> right,
) {
  if (identical(left, right)) return true;
  if (left.runtimeType != right.runtimeType) return false;
  if (left.revision != right.revision) return false;
  return switch ((left, right)) {
    (
      ReadyObservedResource<AgentProfilesDataView>(),
      ReadyObservedResource<AgentProfilesDataView>(),
    ) =>
      _readyProfilesEquals(
        left as ReadyObservedResource<AgentProfilesDataView>,
        right as ReadyObservedResource<AgentProfilesDataView>,
      ),
    (
      RefreshingObservedResource<AgentProfilesDataView>(),
      RefreshingObservedResource<AgentProfilesDataView>(),
    ) =>
      _refreshingProfilesEquals(
        left as RefreshingObservedResource<AgentProfilesDataView>,
        right as RefreshingObservedResource<AgentProfilesDataView>,
      ),
    (
      StaleObservedResource<AgentProfilesDataView>(),
      StaleObservedResource<AgentProfilesDataView>(),
    ) =>
      _staleProfilesEquals(
        left as StaleObservedResource<AgentProfilesDataView>,
        right as StaleObservedResource<AgentProfilesDataView>,
      ),
    (
      DegradedObservedResource<AgentProfilesDataView>(),
      DegradedObservedResource<AgentProfilesDataView>(),
    ) =>
      _degradedProfilesEquals(
        left as DegradedObservedResource<AgentProfilesDataView>,
        right as DegradedObservedResource<AgentProfilesDataView>,
      ),
    (
      LoadingObservedResource<AgentProfilesDataView>(),
      LoadingObservedResource<AgentProfilesDataView>(),
    ) =>
      _loadingProfilesEquals(
        left as LoadingObservedResource<AgentProfilesDataView>,
        right as LoadingObservedResource<AgentProfilesDataView>,
      ),
    (
      FailedObservedResource<AgentProfilesDataView>(),
      FailedObservedResource<AgentProfilesDataView>(),
    ) =>
      _failedProfilesEquals(
        left as FailedObservedResource<AgentProfilesDataView>,
        right as FailedObservedResource<AgentProfilesDataView>,
      ),
    (
      UninitializedObservedResource<AgentProfilesDataView>(),
      UninitializedObservedResource<AgentProfilesDataView>(),
    ) =>
      (left as UninitializedObservedResource<AgentProfilesDataView>)
              .updatedAt ==
          (right as UninitializedObservedResource<AgentProfilesDataView>)
              .updatedAt,
    (
      StoppedObservedResource<AgentProfilesDataView>(),
      StoppedObservedResource<AgentProfilesDataView>(),
    ) =>
      (left as StoppedObservedResource<AgentProfilesDataView>).stoppedAt ==
          (right as StoppedObservedResource<AgentProfilesDataView>).stoppedAt,
    _ => false,
  };
}

bool _readyProfilesEquals(
  ReadyObservedResource<AgentProfilesDataView> left,
  ReadyObservedResource<AgentProfilesDataView> right,
) {
  return left.updatedAt == right.updatedAt &&
      left.lastCheckedAt == right.lastCheckedAt &&
      left.value == right.value;
}

bool _refreshingProfilesEquals(
  RefreshingObservedResource<AgentProfilesDataView> left,
  RefreshingObservedResource<AgentProfilesDataView> right,
) {
  return left.operation == right.operation &&
      left.operationId == right.operationId &&
      left.startedAt == right.startedAt &&
      left.lastCheckedAt == right.lastCheckedAt &&
      left.value == right.value;
}

bool _staleProfilesEquals(
  StaleObservedResource<AgentProfilesDataView> left,
  StaleObservedResource<AgentProfilesDataView> right,
) {
  return left.staleAt == right.staleAt &&
      left.lastCheckedAt == right.lastCheckedAt &&
      left.value == right.value;
}

bool _degradedProfilesEquals(
  DegradedObservedResource<AgentProfilesDataView> left,
  DegradedObservedResource<AgentProfilesDataView> right,
) {
  return left.failedAt == right.failedAt &&
      left.lastCheckedAt == right.lastCheckedAt &&
      left.operation == right.operation &&
      left.value == right.value;
}

bool _failedProfilesEquals(
  FailedObservedResource<AgentProfilesDataView> left,
  FailedObservedResource<AgentProfilesDataView> right,
) {
  return left.failedAt == right.failedAt &&
      left.operation == right.operation &&
      left.error.code == right.error.code &&
      left.error.message == right.error.message &&
      left.error.retryable == right.error.retryable;
}

bool _loadingProfilesEquals(
  LoadingObservedResource<AgentProfilesDataView> left,
  LoadingObservedResource<AgentProfilesDataView> right,
) {
  return left.operation == right.operation &&
      left.operationId == right.operationId &&
      left.startedAt == right.startedAt;
}
