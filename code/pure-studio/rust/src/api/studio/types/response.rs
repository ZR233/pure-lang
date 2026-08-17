use super::runtime::{
    BridgeAgentDirectoryEntryDto, BridgeLspHealthDto, BridgeMcpHealthDto, BridgeObservedStateMeta,
    BridgeStudioRecoveryIssueDto, BridgeTaskRuntimeDto, RuntimeSnapshot,
};
use super::settings::BridgeStudioSettingsDto;
use super::thread_stream::BridgeThread;
use serde::{Deserialize, Serialize};
// ── Response types ──

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
    pub updater: BridgeUpdaterStateSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeProjectDirectoryState {
    pub meta: BridgeObservedStateMeta,
    pub projects: Vec<ProjectDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeThreadDirectoryState {
    pub meta: BridgeObservedStateMeta,
    pub threads: Vec<BridgeThread>,
}

/// Thread directory 分页窗口页（按 `updatedAt` 倒序、id 倒序的 keyset cursor）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeThreadDirectoryPage {
    pub meta: BridgeObservedStateMeta,
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
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskDirectoryState {
    pub meta: BridgeObservedStateMeta,
    pub tasks: Vec<BridgeTaskDirectoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskDirectoryEntry {
    pub root_thread_id: String,
    pub task: BridgeTaskRuntimeDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentDirectoryState {
    pub meta: BridgeObservedStateMeta,
    pub agents: Vec<BridgeAgentDirectoryEntryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSettingsStateSnapshot {
    pub meta: BridgeObservedStateMeta,
    pub settings: BridgeStudioSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRecoveryStateSnapshot {
    pub meta: BridgeObservedStateMeta,
    pub issues: Vec<BridgeStudioRecoveryIssueDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpStateSnapshot {
    pub meta: BridgeObservedStateMeta,
    pub desired_config_fingerprint: String,
    pub applied_config_fingerprint: String,
    pub health: BridgeMcpHealthDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLspStateSnapshot {
    pub meta: BridgeObservedStateMeta,
    pub health: BridgeLspHealthDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSkillsStateSnapshot {
    pub meta: BridgeObservedStateMeta,
    pub project_id: String,
    pub config_fingerprint: String,
    pub catalog_revision: u64,
    pub skills: Vec<SkillSummaryDto>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeProviderUsageStateSnapshot {
    pub meta: BridgeObservedStateMeta,
    pub config_fingerprint: String,
    pub usages: Vec<ProviderUsageDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeUpdaterStateSnapshot {
    pub meta: BridgeObservedStateMeta,
    pub update: Option<BridgeVerifiedUpdateSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeVerifiedUpdateSummary {
    pub version: String,
    pub published_at: i64,
    pub notes_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub path: String,
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
    pub updated_at: i64,
    pub status: String,
    pub usage_kind: String,
    pub message: Option<String>,
    pub balance: Option<DeepSeekBalanceDto>,
    pub coding_plan: Option<ZhipuCodingPlanUsageDto>,
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
}
