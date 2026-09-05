//! Pure Studio 产品运行时。
//!
//! 本 crate 负责 Studio 配置、SQLite projection、project/thread 编排与 bridge DTO；
//! 通用模型循环、agent actor、工具和协议基础类型由 `pl-core` 提供。

pub(crate) use pl_core::*;

pub mod agent;
pub mod config;
mod config_editor;
mod error_mapping;
mod first_run;
mod protocol;
mod provider_usage;
mod studio;
mod updater;

pub use config::{
    ConfigPaths, ConfigRecoveryReport, ConfigRuntimeError, ConfigRuntimeSnapshot, ConfigStore,
    DeepSeekWebSearchConfig, ProviderId, ReasoningEffort, STUDIO_CONFIG_SCHEMA_VERSION,
    StudioConfig, StudioMcpConfig, StudioRole, StudioUiConfig, UserAgentProfile,
    WebSearchContextSize, WebSearchLocation, WebSearchMode,
};
pub use config_editor::{
    ProviderEdit, ProviderModelEdit, ProviderSettingsEdit, RoleEdit, provider_template_kind,
};
pub use error_mapping::studio_error_from_anyhow;
pub use first_run::{
    FirstRunConfigDraft, FirstRunModelDraft, FirstRunProviderDraft, ProviderTemplateKind,
};
pub use protocol::{
    FlushingPersistenceProgress, LspAvailable, LspAvailableActivity, LspBusy, LspChecking,
    LspDisabled, LspIdle, LspIndexing, LspUnavailable, McpAvailable, McpChecking, McpDisabled,
    McpMissingCredential, McpUnavailable, StudioAgentDirectoryData, StudioAgentDirectoryEntry,
    StudioAgentDirectoryState, StudioLspHealth, StudioLspServer, StudioLspServerState,
    StudioLspStateSnapshot, StudioMcpHealth, StudioMcpServer, StudioMcpServerState,
    StudioMcpStateData, StudioMcpStateSnapshot, StudioModelPerformanceSample,
    StudioModelPerformanceSnapshot, StudioModelPerformanceSummary, StudioProductEventEnvelope,
    StudioProductEventKind, StudioProjectDirectoryData, StudioProjectDirectoryState,
    StudioRecoveryStateSnapshot, StudioSessionCostSnapshot, StudioSettingsStateSnapshot,
    StudioShutdownProgress, StudioSkillsStateSnapshot, StudioStateSnapshot,
    StudioThreadDirectoryData, StudioThreadDirectoryDelta, StudioThreadDirectoryPage,
    StudioThreadDirectoryPageData, StudioThreadDirectoryState,
};
pub use provider_usage::{
    DeepSeekBalanceInfo, DeepSeekBalanceUsage, FailedProviderUsage, MissingCredentialProviderUsage,
    ProviderUsageCommand, ProviderUsageData, ProviderUsageRecord, ProviderUsageState,
    ProviderUsageTransitionDecision, ProviderUsageTransitionError, ReadyProviderUsage,
    UnsupportedProviderUsage, ZhipuCodingPlanUsage, ZhipuQuotaLimit, ZhipuQuotaWindow,
    ZhipuToolUsageDetail, provider_usage_records, zhipu_limit_by_window,
};
pub use studio::{
    AttachmentRecord, PersistenceState, PersistenceStateSnapshot, ProductEventBus, ProjectRecord,
    ProviderUsageStateData, ProviderUsageStateSnapshot, SkillSearchResult, SkillsStateSnapshot,
    StudioDatabaseError, StudioHostKind, StudioRecoveryIssue, StudioRecoveryIssueAction,
    StudioRecoveryIssueCategory, StudioRecoveryIssueScope, StudioRuntime,
    StudioRuntimeLifecycleState, StudioRuntimeOptions, StudioRuntimeSnapshot,
    StudioRuntimeStateKind, StudioStartNewThreadResponse, StudioStore, StudioUpdateStateSnapshot,
    StudioWorktreeRecoveryPreview, ThreadRecord,
};
pub use updater::{
    StudioUpdate, StudioUpdateAsset, StudioUpdateCancellation, StudioUpdateCheck,
    StudioUpdateError, StudioUpdateErrorCode, StudioUpdateEvent, StudioUpdater,
};

// 公共签名（bridge DTO 字段、runtime API 返回值）使用的 pl-protocol 类型在此
// 精确重导出，消费方只依赖 pl-studio-runtime 即可命名完整签名。
pub use pl_protocol::{
    AgentProgressStage, AgentState, AgentWorkspaceMode, CancelInteraction, InteractionCommand,
    InteractionContinuationInput, InteractionPurpose, ReopenRecoveredInteraction,
    ResolveToolApproval, ResolveUserInput, ThreadModeId, ToolApprovalResolutionPayload,
    UserInputResolution,
};
