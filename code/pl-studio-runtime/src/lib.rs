//! Pure Studio 产品运行时。
//!
//! 本 crate 负责 Studio 配置、SQLite projection、project/thread 编排与 bridge DTO；
//! 通用模型循环、agent actor、工具和协议基础类型由 `pl-core` 提供。

pub(crate) use pl_core::*;
pub use pl_protocol::*;

pub mod config;
mod config_editor;
mod error_mapping;
mod first_run;
mod protocol;
mod provider_usage;
#[allow(hidden_glob_reexports)]
mod studio;
mod updater;

pub use config::{
    ConfigPaths, ConfigRecoveryReport, ConfigRuntimeError, ConfigRuntimeSnapshot, ConfigStore,
    ProviderId, ReasoningEffort, STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig, StudioMcpConfig,
    StudioMode, StudioModeId, StudioRole, StudioUiConfig, UserAgentProfile, WebSearchContextSize,
    WebSearchLocation, WebSearchMode,
};
pub use config_editor::{
    ProviderEdit, ProviderModelEdit, ProviderSettingsEdit, RoleEdit, provider_template_kind,
};
pub use error_mapping::studio_error_from_anyhow;
pub use first_run::{
    FirstRunConfigDraft, FirstRunModelDraft, FirstRunProviderDraft, ProviderTemplateKind,
};
pub use protocol::*;
pub use provider_usage::{
    DeepSeekBalanceInfo, DeepSeekBalanceUsage, FailedProviderUsage, MissingCredentialProviderUsage,
    ProviderUsageCommand, ProviderUsageData, ProviderUsageRecord, ProviderUsageState,
    ProviderUsageTransitionDecision, ProviderUsageTransitionError, ReadyProviderUsage,
    UnsupportedProviderUsage, ZhipuCodingPlanUsage, ZhipuQuotaLimit, ZhipuQuotaWindow,
    ZhipuToolUsageDetail, provider_usage_records, zhipu_limit_by_window,
};
pub use studio::*;
pub use updater::*;
