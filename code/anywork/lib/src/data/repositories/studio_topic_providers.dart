import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/models/studio_models.dart';
import 'studio_controller.dart';
import 'studio_product_topics.dart';

/// 设置页可见 tab / 面板的 topic 租约。
///
/// 这些 provider 只 watch controller 的 ready（`hasValue`）变化，不 watch 整个
/// `AsyncValue<StudioState>`；因此每个数据事件不会 dispose/release/reacquire 租约。
/// 同 topic 的观察者共享同一底层订阅，最后一个释放时取消。隐藏 tab 应停止 watch
/// 以释放租约；重新进入时先取得最新基线。
ProductTopicLeaseBundle? _acquireSettingsScope(
  Ref ref,
  SettingsProductScopeKind kind,
) {
  final ready = ref.watch(
    studioControllerProvider.select((state) => state.hasValue),
  );
  if (!ready) return null;
  final bundle = ref
      .read(studioControllerProvider.notifier)
      .acquireSettingsScope(kind);
  ref.onDispose(() => unawaited(bundle.release()));
  return bundle;
}

/// 设置页「统计」tab 的 topic 租约（模型性能，独立职责）。
final settingsStatisticsScopeProvider = Provider<ProductTopicLeaseBundle?>(
  (ref) => _acquireSettingsScope(ref, SettingsProductScopeKind.statistics),
  name: 'settingsStatisticsScopeProvider',
  isAutoDispose: true,
);

/// 设置页「Agents」tab 的 topic 租约（配置级 Profiles，独立于运行期目录）。
final settingsAgentsScopeProvider = Provider<ProductTopicLeaseBundle?>(
  (ref) => _acquireSettingsScope(ref, SettingsProductScopeKind.agents),
  name: 'settingsAgentsScopeProvider',
  isAutoDispose: true,
);

/// 设置页 MCP 面板的 topic 租约（独立职责，不与 LSP/Skills 合并）。
final settingsMcpScopeProvider = Provider<ProductTopicLeaseBundle?>(
  (ref) => _acquireSettingsScope(ref, SettingsProductScopeKind.mcp),
  name: 'settingsMcpScopeProvider',
  isAutoDispose: true,
);

/// 设置页 LSP 面板的 topic 租约（独立职责）。
final settingsLspScopeProvider = Provider<ProductTopicLeaseBundle?>(
  (ref) => _acquireSettingsScope(ref, SettingsProductScopeKind.lsp),
  name: 'settingsLspScopeProvider',
  isAutoDispose: true,
);

/// 设置页 Skills 面板的 topic 租约：只随 selectedProjectId identity 变化重建，
/// 给 Skills 作用域始终携带当前 project identity。无项目时空作用域。
final settingsSkillsScopeProvider = Provider<ProductTopicLeaseBundle?>(
  (ref) {
    final ready = ref.watch(
      studioControllerProvider.select((state) => state.hasValue),
    );
    // Skills 作用域额外只 watch 选中项目 identity，避免其他领域变化重建租约。
    final projectId = ref.watch(
      studioControllerProvider.select(
        (state) => state.value?.selectedProjectId,
      ),
    );
    if (!ready || projectId == null) return null;
    final bundle = ref
        .read(studioControllerProvider.notifier)
        .acquireSettingsScope(SettingsProductScopeKind.skills);
    ref.onDispose(() => unawaited(bundle.release()));
    return bundle;
  },
  name: 'settingsSkillsScopeProvider',
  isAutoDispose: true,
);

/// 设置页「用量」tab 的 topic 租约（Provider usage）。
final settingsUsageScopeProvider = Provider<ProductTopicLeaseBundle?>(
  (ref) => _acquireSettingsScope(ref, SettingsProductScopeKind.usage),
  name: 'settingsUsageScopeProvider',
  isAutoDispose: true,
);

/// 持久化队列诊断面板的 topic 租约：面板打开时 watch，关闭即释放（替代周期轮询）。
final persistenceQueueTopicProvider = Provider<ProductTopicLease?>(
  (ref) {
    final ready = ref.watch(
      studioControllerProvider.select((state) => state.hasValue),
    );
    if (!ready) return null;
    final lease = ref
        .read(studioControllerProvider.notifier)
        .acquirePersistenceQueueScope();
    ref.onDispose(() => unawaited(lease.release()));
    return lease;
  },
  name: 'persistenceQueueTopicProvider',
  isAutoDispose: true,
);

/// 全局模型统计的独立投影：从 SettingsPageView 拆出，统计更新不再重建设置壳。
final settingsStatisticsProvider =
    Provider<AsyncValue<ModelPerformanceSnapshotView>>(
      (ref) {
        return ref.watch(
          studioControllerProvider.select(
            (state) => state.whenData((value) => value.modelPerformance),
          ),
        );
      },
      name: 'settingsStatisticsProvider',
      isAutoDispose: true,
    );

/// 配置级 Agent Profiles 快照：AgentsTab 事件驱动的数据入口。
final settingsAgentProfilesProvider =
    Provider<AsyncValue<AgentProfilesStateView?>>(
      (ref) {
        return ref.watch(
          studioControllerProvider.select(
            (state) => state.whenData((value) => value.agentProfilesState),
          ),
        );
      },
      name: 'settingsAgentProfilesProvider',
      isAutoDispose: true,
    );

/// 进程级持久化队列 typed 观测：诊断面板的展示入口。
final persistenceQueueStateProvider =
    Provider<AsyncValue<PersistenceQueueStateView?>>(
      (ref) {
        return ref.watch(
          studioControllerProvider.select(
            (state) => state.whenData((value) => value.persistenceQueueState),
          ),
        );
      },
      name: 'persistenceQueueStateProvider',
      isAutoDispose: true,
    );

/// 选中会话所属 root 的费用状态；`cost == null` 是显式清除（与零费用区分展示）。
final selectedSessionCostsProvider =
    Provider<AsyncValue<SessionCostsStateView?>>(
      (ref) {
        return ref.watch(
          studioControllerProvider.select(
            (state) =>
                state.whenData((value) => value.selectedSessionCostState),
          ),
        );
      },
      name: 'selectedSessionCostsProvider',
      isAutoDispose: true,
    );

/// 单个 topic 的局部传输连接状态 selector（值相等）；无租用时为 null。
///
/// 可见组件用它读取受影响的局部区域状态并在 failure/closed 后调用
/// [productTopicRetryProvider]；业务内容仍来自 canonical state，selector 只表达
/// transport。
final productTopicConnectionProvider =
    Provider.family<ProductTopicConnectionStateView?, ProductTopic>(
      (ref, topic) => ref.watch(
        studioControllerProvider.select(
          (state) => state.value?.topicConnections[topic],
        ),
      ),
      name: 'productTopicConnectionProvider',
      isAutoDispose: true,
    );

/// 显式重试某个 topic 的订阅；局部 failure/closed 后由可见组件触发。
typedef ProductTopicRetry = void Function(ProductTopic topic);

final productTopicRetryProvider = Provider<ProductTopicRetry>(
  (ref) => ref.read(studioControllerProvider.notifier).retryProductTopic,
  name: 'productTopicRetryProvider',
);
