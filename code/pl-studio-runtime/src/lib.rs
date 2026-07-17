//! Pure Studio 产品运行时。
//!
//! 本 crate 负责 Studio 配置、SQLite projection、project/session/task 编排与 bridge DTO；
//! 通用模型循环、agent actor、工具和协议基础类型由 `pl-core` 提供。

pub use pl_core::*;
pub use pl_protocol::*;

pub mod agent;
pub mod config;
mod config_editor;
mod first_run;
mod protocol;
mod provider_usage;
mod studio;

pub use agent::{
    CloseDisposition, CloseOutcome, DurableWorktreeDisposition, DurableWorktreePresence,
    DurableWorktreeResource, LocalWorktreeBackend, MergeOutcome, WorktreeBackend,
    WorktreeCreateFailure, WorktreeCreateSpec, WorktreeError, WorktreeHandle, WorktreeManager,
    WorktreeReconciliation, WorktreeRef, reconcile_task_worktree_group, same_worktree_path,
};
pub use config::{
    ConfigPaths, ConfigStore, STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig, StudioInstructionsConfig,
    StudioMcpConfig, StudioMode, StudioRole, StudioRuntimeConfig, StudioSkillsConfig,
    StudioUiConfig, StudioWebSearchAvailability, StudioWebSearchBackend, StudioWebSearchConfig,
    StudioWebSearchPath, StudioWebSearchResolution, WebSearchContextSize, WebSearchLocation,
    WebSearchMode, resolve_web_search,
};
pub use config_editor::{
    ProviderEdit, ProviderModelEdit, ProviderSettingsEdit, RoleEdit, provider_template_kind,
};
pub use first_run::{
    FirstRunConfigDraft, FirstRunModelDraft, FirstRunProviderDraft, ProviderTemplateKind,
};
pub use protocol::*;
pub use provider_usage::{
    ProviderUsageData, ProviderUsageRecord, ProviderUsageState, provider_usage_records,
};
pub use studio::*;
