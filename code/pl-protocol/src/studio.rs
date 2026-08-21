//! Studio transport-neutral command, response, event, and error protocol.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

static NEXT_CORRELATION_ID: AtomicU64 = AtomicU64::new(1);

/// Stable Studio API error categories shared by FRB and HTTP.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioErrorCode {
    NotInitialized,
    RuntimeStopped,
    InstanceBusy,
    InvalidArgument,
    NotFound,
    Busy,
    Conflict,
    StaleRevision,
    PermissionDenied,
    Cancelled,
    CancellationTooLate,
    Overloaded,
    Unavailable,
    Protocol,
    Storage,
    Update,
    Internal,
}

/// A redacted API error. Internal diagnostics are correlated through `correlation_id` only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, thiserror::Error, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[error("{message} (correlation id: {correlation_id})")]
pub struct StudioError {
    pub code: StudioErrorCode,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: String,
    #[schema(value_type = Object)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl StudioError {
    pub fn new(code: StudioErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            correlation_id: next_correlation_id(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(StudioErrorCode::InvalidArgument, message, false)
    }

    pub fn not_found(resource: &'static str) -> Self {
        Self::new(
            StudioErrorCode::NotFound,
            format!("The requested Studio {resource} was not found"),
            false,
        )
    }

    pub fn instance_busy() -> Self {
        Self::new(
            StudioErrorCode::InstanceBusy,
            "Another Studio runtime already owns this home",
            true,
        )
    }

    pub fn internal() -> Self {
        Self::new(
            StudioErrorCode::Internal,
            "Studio could not complete the operation",
            false,
        )
    }

    pub fn storage() -> Self {
        Self::new(
            StudioErrorCode::Storage,
            "Studio storage is unavailable",
            true,
        )
    }
}

fn next_correlation_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let sequence = NEXT_CORRELATION_ID.fetch_add(1, Ordering::Relaxed);
    format!("studio-{timestamp:x}-{sequence:x}")
}

pub type StudioResult<T> = Result<T, StudioError>;

/// Canonical shared command/query/stream operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioOperation {
    ReadState,
    OpenProject,
    ActivateProject,
    ArchiveProject,
    ListThreadsPage,
    StartNewThread,
    ReadThread,
    ArchiveThread,
    SetThreadMode,
    ListThreadTurns,
    StartTurn,
    SteerTurn,
    InterruptTurn,
    RespondInteraction,
    LoadProviderCatalog,
    ReadSettings,
    ReloadSettings,
    SaveWebSearchSettings,
    SavePermissionSettings,
    SaveProviderSettings,
    SaveInstructionsSettings,
    SaveSkillsSettings,
    SaveMcpSettings,
    SaveGeneralSettings,
    SetModelRole,
    ReadProviderUsage,
    CheckProviderUsage,
    ReadSkills,
    DiscoverSkills,
    ReadMcp,
    ResetMcp,
    ReadLsp,
    ProbeLsp,
    RepairLsp,
    ResetLsp,
    ReadUpdate,
    CheckUpdate,
    PreviewTaskRecovery,
    ApplyTaskRecovery,
    PreviewRecoveryCleanup,
    CleanupRecoveryIssue,
    RetryRecoveryIssue,
    PreviewProjectCleanup,
    CleanupProject,
    SubscribeProduct,
    SubscribeThread,
}

impl StudioOperation {
    pub const ALL: [Self; 46] = [
        Self::ReadState,
        Self::OpenProject,
        Self::ActivateProject,
        Self::ArchiveProject,
        Self::ListThreadsPage,
        Self::StartNewThread,
        Self::ReadThread,
        Self::ArchiveThread,
        Self::SetThreadMode,
        Self::ListThreadTurns,
        Self::StartTurn,
        Self::SteerTurn,
        Self::InterruptTurn,
        Self::RespondInteraction,
        Self::LoadProviderCatalog,
        Self::ReadSettings,
        Self::ReloadSettings,
        Self::SaveWebSearchSettings,
        Self::SavePermissionSettings,
        Self::SaveProviderSettings,
        Self::SaveInstructionsSettings,
        Self::SaveSkillsSettings,
        Self::SaveMcpSettings,
        Self::SaveGeneralSettings,
        Self::SetModelRole,
        Self::ReadProviderUsage,
        Self::CheckProviderUsage,
        Self::ReadSkills,
        Self::DiscoverSkills,
        Self::ReadMcp,
        Self::ResetMcp,
        Self::ReadLsp,
        Self::ProbeLsp,
        Self::RepairLsp,
        Self::ResetLsp,
        Self::ReadUpdate,
        Self::CheckUpdate,
        Self::PreviewTaskRecovery,
        Self::ApplyTaskRecovery,
        Self::PreviewRecoveryCleanup,
        Self::CleanupRecoveryIssue,
        Self::RetryRecoveryIssue,
        Self::PreviewProjectCleanup,
        Self::CleanupProject,
        Self::SubscribeProduct,
        Self::SubscribeThread,
    ];

    pub const fn operation_id(self) -> &'static str {
        match self {
            Self::ReadState => "studio.readState",
            Self::OpenProject => "project.open",
            Self::ActivateProject => "project.activate",
            Self::ArchiveProject => "project.archive",
            Self::ListThreadsPage => "thread.listPage",
            Self::StartNewThread => "thread.create",
            Self::ReadThread => "thread.read",
            Self::ArchiveThread => "thread.archive",
            Self::SetThreadMode => "thread.setMode",
            Self::ListThreadTurns => "thread.listTurns",
            Self::StartTurn => "turn.start",
            Self::SteerTurn => "turn.steer",
            Self::InterruptTurn => "turn.interrupt",
            Self::RespondInteraction => "interaction.respond",
            Self::LoadProviderCatalog => "settings.loadProviderCatalog",
            Self::ReadSettings => "settings.read",
            Self::ReloadSettings => "settings.reload",
            Self::SaveWebSearchSettings => "settings.saveWebSearch",
            Self::SavePermissionSettings => "settings.savePermission",
            Self::SaveProviderSettings => "settings.saveProviders",
            Self::SaveInstructionsSettings => "settings.saveInstructions",
            Self::SaveSkillsSettings => "settings.saveSkills",
            Self::SaveMcpSettings => "settings.saveMcp",
            Self::SaveGeneralSettings => "settings.saveGeneral",
            Self::SetModelRole => "settings.setModelRole",
            Self::ReadProviderUsage => "providerUsage.read",
            Self::CheckProviderUsage => "providerUsage.check",
            Self::ReadSkills => "skills.read",
            Self::DiscoverSkills => "skills.discover",
            Self::ReadMcp => "mcp.read",
            Self::ResetMcp => "mcp.reset",
            Self::ReadLsp => "lsp.read",
            Self::ProbeLsp => "lsp.probe",
            Self::RepairLsp => "lsp.repair",
            Self::ResetLsp => "lsp.reset",
            Self::ReadUpdate => "update.read",
            Self::CheckUpdate => "update.check",
            Self::PreviewTaskRecovery => "recovery.taskPreview",
            Self::ApplyTaskRecovery => "recovery.taskApply",
            Self::PreviewRecoveryCleanup => "recovery.issueCleanupPreview",
            Self::CleanupRecoveryIssue => "recovery.issueCleanup",
            Self::RetryRecoveryIssue => "recovery.issueRetry",
            Self::PreviewProjectCleanup => "recovery.projectCleanupPreview",
            Self::CleanupProject => "recovery.projectCleanup",
            Self::SubscribeProduct => "studio.subscribeProduct",
            Self::SubscribeThread => "thread.subscribe",
        }
    }
}

/// Desktop-host-only operations intentionally omitted from the HTTP API.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioHostOperation {
    InitializeFrb,
    StartRuntime,
    ShutdownRuntime,
    SubscribeShutdownProgress,
    InstallUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenProjectRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateThreadRequest {
    pub title: String,
    pub prompt: String,
    #[serde(default)]
    pub attachment_ids: Vec<String>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetThreadModeRequest {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartTurnRequest {
    pub prompt: String,
    #[serde(default)]
    pub attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SteerTurnRequest {
    pub prompt: String,
    #[serde(default)]
    pub attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InterruptTurnRequest {
    pub expected_turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveInteractionRequest {
    pub resolution: crate::InteractionResolution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpectedRevisionRequest {
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpectedOpaqueRevisionRequest {
    pub expected_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeRequest {
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepairLspRequest {
    pub server_id: String,
}

/// Task conversation recovery target role.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskRecoveryTargetKind {
    Planner,
    Executor,
}

/// Git and worktree facts that must remain stable between recovery preview and apply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioTaskGitFingerprint {
    pub workspace_root: String,
    pub git_common_dir: String,
    pub branch: String,
    pub head: String,
    pub base_commit: String,
    pub expected_head: String,
    pub operation: String,
    pub index_diff_hash: String,
    pub working_tree_diff_hash: String,
    pub untracked_content_hash: String,
}

/// Terminal Turn available for conversation recovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioTaskRecoveryTurn {
    pub turn_id: String,
    pub status: String,
    pub updated_at: i64,
    pub item_count: u64,
    pub input_count: u64,
    pub tool_count: u64,
    pub tool_summaries: Vec<String>,
}

/// One Planner or Executor recovery target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioTaskRecoveryTarget {
    pub thread_id: String,
    pub kind: StudioTaskRecoveryTargetKind,
    pub work_unit_id: Option<String>,
    pub attempt: Option<u32>,
    pub continuation_revision: Option<u64>,
    pub expected_runtime_revision: u64,
    pub expected_thread_revision: u64,
    pub branch: String,
    pub worktree_path: String,
    pub turns: Vec<StudioTaskRecoveryTurn>,
    pub default_turn_ids: Vec<String>,
    pub available_modes: Vec<crate::ConversationRecoveryMode>,
    pub git_fingerprint: StudioTaskGitFingerprint,
}

/// Stateless recovery preview used as the apply CAS token and fact set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioTaskRecoveryPreview {
    pub preview_token: String,
    pub root_thread_id: String,
    pub run_id: String,
    pub revision: u64,
    pub task_generation: u64,
    pub state: StudioTaskRecoveryState,
    pub expected_head: String,
    pub stop_requested: bool,
    pub branch_lease_id: String,
    pub branch_lease_branch: String,
    pub branch_lease_git_common_dir: String,
    pub branch_lease_expected_head: String,
    pub recommended_thread_id: String,
    pub targets: Vec<StudioTaskRecoveryTarget>,
    pub main_git_fingerprint: StudioTaskGitFingerprint,
    pub completion_revision_fingerprint: String,
    pub review_revision_fingerprint: String,
    pub merge_revision_fingerprint: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StudioTaskRecoveryState {
    DesignUpdating,
    Implementing,
    Merging,
    Reviewing,
    Reworking,
    Stopping,
    Blocked,
    Completed,
    Failed,
    Cancelled,
}

/// Applies a previously generated Task recovery preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioTaskRecoveryRequest {
    pub recovery_id: String,
    pub root_thread_id: String,
    pub target_thread_id: String,
    pub mode: crate::ConversationRecoveryMode,
    pub turn_ids: Vec<String>,
    pub preview: StudioTaskRecoveryPreview,
}

/// Durable result of applying a Task conversation recovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRecoveryResult {
    pub recovery_id: String,
    pub run_id: String,
    pub work_unit_id: Option<String>,
    pub root_thread_id: String,
    pub target_thread_id: String,
    pub mode: crate::ConversationRecoveryMode,
    pub recovery_revision: u64,
    pub runtime_revision: u64,
    pub thread_revision: u64,
    pub before_transcript_hash: String,
    pub after_transcript_hash: String,
    pub removed_item_count: u64,
    pub removed_input_count: u64,
    pub stop_cleared: bool,
    pub resume_turn_id: String,
    pub git_fingerprint: StudioTaskGitFingerprint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", tag = "scope", deny_unknown_fields)]
pub enum McpResetRequest {
    Server { server_id: String },
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", tag = "scope", deny_unknown_fields)]
pub enum LspResetRequest {
    Server {
        project_id: String,
        server_id: String,
    },
    Workspace {
        project_id: String,
    },
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadPageQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

impl ThreadPageQuery {
    pub const DEFAULT_LIMIT: u32 = 50;
    pub const MAX_LIMIT: u32 = 200;

    pub fn limit(&self) -> usize {
        self.limit
            .unwrap_or(Self::DEFAULT_LIMIT)
            .clamp(1, Self::MAX_LIMIT) as usize
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
}

/// Secret-free canonical Settings snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioSettingsSnapshot {
    pub revision: u64,
    pub updated_at: i64,
    pub settings: StudioSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioSettings {
    pub default_provider_id: Option<String>,
    pub providers: Vec<StudioProviderSettings>,
    pub roles: Vec<StudioRoleSettings>,
    pub permission_mode: String,
    pub instructions: StudioInstructionsSettings,
    pub skills: StudioSkillsSettings,
    pub mcp_servers: Vec<StudioMcpServerSettings>,
    pub general: StudioGeneralSettings,
    pub web_search: StudioWebSearchSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioProviderSettings {
    pub id: String,
    pub template_kind: String,
    pub name: String,
    pub base_url: String,
    pub has_bearer_token: bool,
    pub capability_source: String,
    pub hosted_web_search: bool,
    pub standalone_web_search: Option<String>,
    pub prompt_cache_dialect: String,
    pub responses_tool_search: bool,
    pub responses_programmatic_tool_calling: bool,
    pub default_model: String,
    pub models: Vec<StudioProviderModelSettings>,
    pub custom_models: Vec<StudioProviderModelSettings>,
    pub catalog_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioProviderModelSettings {
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub currency: String,
    pub input_price_per_m_tok: Option<f64>,
    pub output_price_per_m_tok: Option<f64>,
    pub cache_read_price_per_m_tok: Option<f64>,
    pub cache_write_price_per_m_tok: Option<f64>,
    pub reasoning_efforts: Vec<String>,
    pub base_instructions: String,
    pub wire_protocol: String,
    pub supported_connection_modes: Vec<String>,
    pub default_connection_mode: String,
    pub connection_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioRoleSettings {
    pub key: String,
    pub provider_id: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioInstructionsSettings {
    pub base_override: String,
    pub developer: String,
    pub user: String,
    pub project_doc_max_bytes: u64,
    pub project_doc_fallback_filenames: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioSkillsSettings {
    pub enabled: bool,
    pub auto_learn: bool,
    pub system_enabled: bool,
    pub project_dir: String,
    pub user_dir: String,
    pub external_dirs: Vec<String>,
    pub disabled: Vec<String>,
    pub auto_learn_min_tool_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioMcpServerSettings {
    pub id: String,
    pub transport: String,
    pub endpoint: String,
    pub enabled: bool,
    pub status: String,
    pub source_kind: String,
    pub mutation_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioGeneralSettings {
    pub follow_system_theme: bool,
    pub follow_active_turn: bool,
    pub compact_timeline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioWebSearchSettings {
    pub configured_mode: String,
    pub effective_mode: String,
    pub availability: String,
    pub context_size: Option<String>,
    pub allowed_domains: Vec<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub timezone: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatePermissionSettingsRequest {
    pub expected_revision: u64,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateInstructionsSettingsRequest {
    pub expected_revision: u64,
    pub settings: StudioInstructionsSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSkillsSettingsRequest {
    pub expected_revision: u64,
    pub settings: StudioSkillsSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateGeneralSettingsRequest {
    pub expected_revision: u64,
    pub settings: StudioGeneralSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateWebSearchSettingsRequest {
    pub expected_revision: u64,
    pub mode: String,
    pub context_size: Option<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetModelRoleRequest {
    pub expected_revision: u64,
    pub role: String,
    pub provider_id: String,
    pub model: String,
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateMcpSettingsRequest {
    pub expected_revision: u64,
    pub servers: Vec<McpServerUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpServerUpdate {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateProviderSettingsRequest {
    pub expected_revision: u64,
    pub default_provider_id: String,
    pub providers: Vec<ProviderSettingsUpdate>,
    pub roles: Vec<RoleSettingsUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSettingsUpdate {
    pub id: String,
    pub original_id: Option<String>,
    pub template_kind: String,
    pub name: String,
    pub base_url: String,
    pub secret: ProviderSecretUpdate,
    pub capability_source: String,
    pub hosted_web_search: bool,
    pub standalone_web_search: Option<String>,
    pub prompt_cache_dialect: String,
    pub responses_tool_search: bool,
    pub responses_programmatic_tool_calling: bool,
    pub default_model: String,
    pub custom_models: Vec<ProviderModelUpdate>,
    pub model_connection_modes: Vec<ProviderModelConnectionUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderModelUpdate {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
    pub base_instructions: Option<String>,
    pub wire_protocol: String,
    pub supported_connection_modes: Vec<String>,
    pub default_connection_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderModelConnectionUpdate {
    pub slug: String,
    pub connection_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoleSettingsUpdate {
    pub key: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", tag = "action", deny_unknown_fields)]
pub enum ProviderSecretUpdate {
    Preserve,
    Replace { value: String },
    Clear,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_bodies_reject_unknown_fields() {
        let error = serde_json::from_value::<StartTurnRequest>(serde_json::json!({
            "prompt": "hello",
            "attachmentIds": [],
            "legacy": true,
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn studio_error_is_camel_case_and_redacted_by_construction() {
        let error = StudioError::internal();
        let value = serde_json::to_value(&error).unwrap();
        assert_eq!(value["code"], "internal");
        assert!(value.get("correlationId").is_some());
        assert!(!error.message.contains("secret"));
    }

    #[test]
    fn operation_ids_are_unique() {
        let ids = StudioOperation::ALL
            .into_iter()
            .map(StudioOperation::operation_id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), StudioOperation::ALL.len());
    }
}
