use super::runtime::{
    BridgeAgentDirectoryEntryDto, BridgeDegradedResource, BridgeFailedResource,
    BridgeLoadingResource, BridgeLspHealthDto, BridgeMcpHealthDto, BridgeReadyResource,
    BridgeRefreshingResource, BridgeStaleResource, BridgeStoppedResource,
    BridgeStudioRecoveryIssueDto, BridgeTaskRuntimeDto, BridgeUninitializedResource,
    RuntimeSnapshot,
};
use super::settings::BridgeStudioSettingsDto;
use super::thread_stream::{BridgeRuntimeCostAmount, BridgeThread};
use super::updater::BridgeUpdaterStateSnapshot;
use serde::{Deserialize, Serialize};
// ── Response types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioStartupResult {
    pub runtime: RuntimeSnapshot,
    pub config_recovery: Option<BridgeConfigRecoveryReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeConfigRecoveryReport {
    pub backup_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioStateSnapshot {
    pub runtime: RuntimeSnapshot,
    pub project_directory: BridgeProjectDirectoryState,
    /// 目录分页窗口的首页；后续页通过 `listThreadsPage` keyset cursor 加载。
    pub thread_directory: BridgeThreadDirectoryPage,
    pub task_directory: BridgeTaskDirectoryState,
    pub agent_directory: BridgeAgentDirectoryState,
    pub settings: BridgeSettingsStateSnapshot,
    pub recovery: BridgeRecoveryStateSnapshot,
    pub mcp: BridgeMcpStateSnapshot,
    pub lsp: BridgeLspStateSnapshot,
    pub skills_by_project: Vec<BridgeSkillsStateSnapshot>,
    pub provider_usage: BridgeProviderUsageStateSnapshot,
    pub model_performance: BridgeModelPerformanceSnapshot,
    pub updater: BridgeUpdaterStateSnapshot,
    pub persistence: BridgePersistenceStateSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeModelPerformanceSnapshot {
    pub revision: u64,
    pub updated_at: i64,
    pub session_costs: Vec<BridgeSessionCostSnapshot>,
    pub summaries: Vec<BridgeModelPerformanceSummary>,
    pub history: Vec<BridgeModelPerformanceSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSessionCostSnapshot {
    pub root_thread_id: String,
    pub estimated_costs: Vec<BridgeRuntimeCostAmount>,
    pub has_unpriced_usage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeModelPerformanceSummary {
    pub provider_instance_id: String,
    pub provider_display_name: String,
    pub model: String,
    pub sample_count: u64,
    pub completion_tokens: u64,
    pub total_ttft_millis: u64,
    pub total_decode_millis: u64,
    pub total_response_millis: u64,
    pub tokens_per_second: f64,
    pub average_ttft_millis: f64,
    pub average_response_millis: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeModelPerformanceSample {
    pub completed_at: i64,
    pub provider_instance_id: String,
    pub provider_display_name: String,
    pub model: String,
    pub completion_tokens: u64,
    pub ttft_millis: u64,
    pub decode_millis: u64,
    pub total_response_millis: u64,
    pub tokens_per_second: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgePersistenceStateSnapshot {
    pub revision: u64,
    pub state: BridgePersistenceState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgePersistenceState {
    Ready {
        pending_commits: u64,
    },
    Flushing {
        pending_commits: u64,
        oldest_pending_revision: Option<u64>,
    },
    Degraded {
        pending_commits: u64,
        oldest_pending_revision: Option<u64>,
        first_failed_at: i64,
        error: super::runtime::BridgeStateError,
    },
    Recovering {
        pending_commits: u64,
        oldest_pending_revision: Option<u64>,
        first_failed_at: i64,
    },
    Blocked {
        pending_commits: u64,
        oldest_pending_revision: Option<u64>,
        first_failed_at: i64,
        error: super::runtime::BridgeStateError,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeProjectDirectoryState {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeProjectDirectoryData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeProjectDirectoryData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeProjectDirectoryData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeProjectDirectoryData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeProjectDirectoryData {
    pub projects: Vec<ProjectDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeThreadDirectoryPage {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeThreadDirectoryPageData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeThreadDirectoryPageData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeThreadDirectoryPageData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeThreadDirectoryPageData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

/// Thread directory 分页窗口页数据（按 `updatedAt` 倒序、id 倒序的 keyset cursor）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeThreadDirectoryPageData {
    pub threads: Vec<BridgeThread>,
    /// `None` 表示没有更旧的页。
    pub next_cursor: Option<String>,
}

/// `listThreadsPage` 请求；`cursor` 为 `null` 时从最新一页开始。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeListThreadsPageRequest {
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeTaskDirectoryState {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeTaskDirectoryData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeTaskDirectoryData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeTaskDirectoryData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeTaskDirectoryData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeTaskDirectoryData {
    pub tasks: Vec<BridgeTaskDirectoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskDirectoryEntry {
    pub root_thread_id: String,
    pub task: BridgeTaskRuntimeDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeAgentDirectoryState {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeAgentDirectoryData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeAgentDirectoryData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeAgentDirectoryData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeAgentDirectoryData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeAgentDirectoryData {
    pub agents: Vec<BridgeAgentDirectoryEntryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeSettingsStateSnapshot {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeSettingsStateData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeSettingsStateData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeSettingsStateData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeSettingsStateData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeSettingsStateData {
    pub settings: BridgeStudioSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeRecoveryStateSnapshot {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeRecoveryStateData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeRecoveryStateData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeRecoveryStateData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeRecoveryStateData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeRecoveryStateData {
    pub issues: Vec<BridgeStudioRecoveryIssueDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeMcpStateSnapshot {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeMcpStateData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeMcpStateData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeMcpStateData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeMcpStateData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeMcpStateData {
    pub desired_config_fingerprint: String,
    pub applied_config_fingerprint: String,
    pub health: BridgeMcpHealthDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeLspStateSnapshot {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeLspStateData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeLspStateData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeLspStateData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeLspStateData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeLspStateData {
    pub health: BridgeLspHealthDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSkillsStateSnapshot {
    pub project_id: String,
    pub state: BridgeSkillsResourceState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeSkillsResourceState {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeSkillsStateData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeSkillsStateData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeSkillsStateData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeSkillsStateData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeSkillsStateData {
    pub config_fingerprint: String,
    pub catalog_revision: u64,
    pub skills: Vec<SkillSummaryDto>,
    pub warnings: Vec<String>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeProviderUsageStateSnapshot {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready {
        resource: BridgeReadyResource,
        value: BridgeProviderUsageStateData,
    },
    Refreshing {
        resource: BridgeRefreshingResource,
        value: BridgeProviderUsageStateData,
    },
    Stale {
        resource: BridgeStaleResource,
        value: BridgeProviderUsageStateData,
    },
    Degraded {
        resource: BridgeDegradedResource,
        value: BridgeProviderUsageStateData,
    },
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeProviderUsageStateData {
    pub config_fingerprint: String,
    pub usages: Vec<ProviderUsageDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub ssh_server_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartTurnResponse {
    pub thread_id: String,
    pub turn_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartNewThreadResponse {
    pub thread: BridgeThread,
    pub receipt: StartTurnResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveThreadResult {
    pub archived_root_id: String,
    pub removed_thread_ids: Vec<String>,
    pub next_root: Option<BridgeThread>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InterruptTurnResponse {
    pub thread_id: String,
    pub turn_id: String,
    pub interrupted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SteerTurnResponse {
    pub thread_id: String,
    pub turn_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsagesResponse {
    pub usages: Vec<ProviderUsageDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageDto {
    pub provider_id: String,
    pub revision: u64,
    pub updated_at: i64,
    pub state: BridgeProviderUsageState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeProviderUsageState {
    Unsupported,
    MissingCredential {
        message: String,
    },
    Ready {
        data: BridgeProviderUsageData,
    },
    Failed {
        error: super::runtime::BridgeStateError,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeProviderUsageData {
    DeepSeekBalance(DeepSeekBalanceDto),
    ZhipuCodingPlan(ZhipuCodingPlanUsageDto),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekBalanceDto {
    pub is_available: bool,
    pub balances: Vec<DeepSeekBalanceInfoDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekBalanceInfoDto {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuCodingPlanUsageDto {
    pub level: Option<String>,
    pub limits: Vec<ZhipuQuotaLimitDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuQuotaLimitDto {
    pub window: String,
    pub label: String,
    pub percentage: f64,
    pub current_value: Option<f64>,
    pub total: Option<f64>,
    pub remaining: Option<f64>,
    pub next_reset_at: Option<i64>,
    pub usage_details: Vec<ZhipuToolUsageDetailDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuToolUsageDetailDto {
    pub name: String,
    pub current_value: Option<f64>,
    pub total: Option<f64>,
    pub percentage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillsResponse {
    pub skills: Vec<SkillSummaryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummaryDto {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub platforms: Vec<String>,
    pub source: String,
    pub provider_id: String,
    pub invocation: SkillInvocationPolicyDto,
    pub resource_base: SkillResourceBaseDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInvocationPolicyDto {
    pub model_invocable: bool,
    pub user_invocable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SkillResourceBaseDto {
    Directory { path: String },
    Url { url: String },
    Opaque { description: String },
}
