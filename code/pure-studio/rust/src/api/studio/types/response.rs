use super::interaction::BridgeInteractionChangedDto;
use super::runtime::{BridgeStudioRecoveryIssueDto, BridgeTaskRuntimeDto};
use super::settings::BridgeStudioSettingsDto;
use serde::{Deserialize, Serialize};
// ── Response types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioSnapshotResponse {
    pub projects: Vec<ProjectDto>,
    pub selected_project_id: Option<String>,
    pub sessions: Vec<SessionDto>,
    pub selected_session_id: Option<String>,
    pub selected_session_task: Option<BridgeTaskRuntimeDto>,
    pub recovery_issues: Vec<BridgeStudioRecoveryIssueDto>,
    pub settings: BridgeStudioSettingsDto,
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
pub struct SessionDto {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub visibility: String,
    pub parent_session_id: Option<String>,
    pub root_session_id: String,
    pub session_kind: String,
    pub owner_agent_id: String,
    pub owner_role: String,
    pub agent_status: String,
    pub agent_summary: Option<String>,
    pub agent_error: Option<String>,
    pub agent_updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitPromptResponse {
    pub session_id: String,
    pub turn_id: String,
    pub cursor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StopPromptResponse {
    pub session_id: String,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolveInteractionResponse {
    pub session_id: String,
    pub interaction: BridgeInteractionChangedDto,
    pub sessions: Vec<SessionDto>,
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
