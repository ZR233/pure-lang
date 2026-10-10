import '../../domain/models/studio_models.dart';
import 'studio_state_reducer.dart';

/// typed 产品 topic 帧的领域分发层。
///
/// Baseline 与 Event 分别处理：Baseline 是订阅建立后的 canonical 首帧，带领域
/// revision 与完整领域状态；Event 携带 owner 事实，各 apply 只按领域 revision
/// 前进。`Lagged`/`Failure` 不改写领域状态（保旧数据），恢复由订阅注册表完成：
/// Lagged 触发单 topic 重建并重新以 Baseline 首帧同步，不做全量 readStudioState。
StudioState applyProductTopicFrame(
  StudioState current,
  ProductTopicFrame frame,
) {
  return switch (frame) {
    ProductTopicBaselineFrame() => _applyBaseline(current, frame),
    ProductTopicDataFrame(:final event) => applyProductTopicPayload(
      current,
      event.payload,
    ),
    ProductTopicLaggedFrame() || ProductTopicFailureFrame() => current,
  };
}

StudioState _applyBaseline(
  StudioState current,
  ProductTopicBaselineFrame frame,
) {
  return switch (frame.state) {
    ProjectDirectoryBaseline(:final state) => applyProjectDirectory(
      current,
      state,
    ),
    ThreadDirectoryBaseline(:final page) => applyThreadDirectoryBaseline(
      current,
      page,
      revision: frame.revision,
    ),
    AgentDirectoryBaseline(:final state) => applyAgentDirectory(current, state),
    SettingsConfigBaseline(:final state) => applySettingsConfigState(
      current,
      state,
    ),
    ModelCatalogBaseline(:final state) => applyModelCatalogState(
      current,
      state,
    ),
    RecoveryBaseline(:final state) => applyRecoveryState(current, state),
    McpBaseline(:final state) => applyMcpState(current, state),
    LspBaseline(:final state) => applyLspState(current, state),
    SkillsBaseline(:final state) => applySkillsState(current, state),
    ThreadModeCatalogBaseline(:final state) => applyThreadModeCatalog(
      current,
      state,
    ),
    ProviderUsageBaseline(:final state) => applyProviderUsageState(
      current,
      state,
    ),
    ModelPerformanceBaseline(:final state) => applyModelPerformanceState(
      current,
      state,
    ),
    SessionCostsBaseline(:final state) => applySessionCostsState(
      current,
      state,
    ),
    UpdaterBaseline(:final state) => applyUpdaterState(current, state),
    PersistenceBaseline(:final state) => applyPersistenceState(current, state),
    PersistenceQueueBaseline(:final state) => applyPersistenceQueueState(
      current,
      state,
    ),
    AgentProfilesBaseline(:final state) => applyAgentProfilesState(
      current,
      state,
    ),
  };
}

/// 单个产品事件 payload 的领域应用；与 Baseline 共用同一组 apply（同一领域
/// revision 语义），保证事件不会用旧事实覆盖 Baseline。
StudioState applyProductTopicPayload(
  StudioState current,
  ProductTopicEventPayload payload,
) {
  return switch (payload) {
    ProjectDirectoryChangedPayload(:final state) => applyProjectDirectory(
      current,
      state,
    ),
    ThreadDirectoryChangedPayload(
      :final revision,
      :final upserted,
      :final removed,
    ) =>
      applyThreadDirectoryDelta(
        current,
        revision: revision,
        upserted: upserted,
        removed: removed,
      ),
    AgentDirectoryChangedPayload(:final state) => applyAgentDirectory(
      current,
      state,
    ),
    SettingsConfigStateChangedPayload(:final state) => applySettingsConfigState(
      current,
      state,
    ),
    ModelCatalogStateChangedPayload(:final state) => applyModelCatalogState(
      current,
      state,
    ),
    RecoveryStateChangedPayload(:final state) => applyRecoveryState(
      current,
      state,
    ),
    McpStateChangedPayload(:final state) => applyMcpState(current, state),
    LspStateChangedPayload(:final state) => applyLspState(current, state),
    SkillsStateChangedPayload(:final state) => applySkillsState(current, state),
    ThreadModeCatalogChangedPayload(:final state) => applyThreadModeCatalog(
      current,
      state,
    ),
    ProviderUsageStateChangedPayload(:final state) => applyProviderUsageState(
      current,
      state,
    ),
    ModelPerformanceStateChangedPayload(:final state) =>
      applyModelPerformanceState(current, state),
    SessionCostsChangedPayload(:final state) => applySessionCostsState(
      current,
      state,
    ),
    UpdaterStateChangedPayload(:final state) => applyUpdaterState(
      current,
      state,
    ),
    PersistenceStateChangedPayload(:final state) => applyPersistenceState(
      current,
      state,
    ),
    PersistenceQueueStateChangedPayload(:final state) =>
      applyPersistenceQueueState(current, state),
    AgentProfilesStateChangedPayload(:final state) => applyAgentProfilesState(
      current,
      state,
    ),
  };
}
