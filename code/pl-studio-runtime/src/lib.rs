//! Pure Studio 产品运行时。
//!
//! 本 crate 负责 Studio 配置、SQLite projection、project/thread/task 编排与 bridge DTO；
//! 通用模型循环、agent actor、工具和协议基础类型由 `pl-core` 提供。

pub use pl_core::*;
pub use pl_lsp::LspScope;
pub use pl_protocol::*;

pub mod agent;
pub mod config;
mod config_editor;
mod first_run;
mod protocol;
mod provider_usage;
mod studio;
mod updater;

pub use agent::{
    DurableWorktreeDisposition, DurableWorktreePresence, DurableWorktreeResource,
    LocalWorktreeBackend, WorktreeBackend, WorktreeCreateFailure, WorktreeCreateSpec,
    WorktreeError, WorktreeHandle, WorktreeManager, WorktreeReconciliation, WorktreeRef,
    reconcile_task_worktree_group, same_worktree_path,
};
pub use config::{
    ConfigPaths, ConfigRuntimeSnapshot, ConfigStore, STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig,
    StudioInstructionsConfig, StudioMcpConfig, StudioMode, StudioRole, StudioRuntimeConfig,
    StudioSkillsConfig, StudioUiConfig, StudioWebSearchConfig, WebSearchContextSize,
    WebSearchLocation, WebSearchMode,
};
pub use config_editor::{
    ProviderEdit, ProviderModelEdit, ProviderSettingsEdit, RoleEdit, provider_template_kind,
};
pub use first_run::{
    FirstRunConfigDraft, FirstRunModelDraft, FirstRunProviderDraft, ProviderTemplateKind,
};
pub use protocol::*;
pub use provider_usage::{
    DeepSeekBalanceInfo, DeepSeekBalanceUsage, ProviderUsageData, ProviderUsageRecord,
    ProviderUsageState, ZhipuCodingPlanUsage, ZhipuQuotaLimit, ZhipuQuotaWindow,
    ZhipuToolUsageDetail, provider_usage_records, zhipu_limit_by_window,
};
pub use studio::*;
pub use updater::*;
