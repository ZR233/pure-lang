use pl_protocol::{ObservedResource, Thread, ThreadModeCatalogSnapshot};
use serde::{Deserialize, Serialize};

use crate::{
    PersistenceStateSnapshot, ProjectRecord, ProviderUsageStateSnapshot, SkillsStateSnapshot,
    StudioAgentDirectoryEntry, StudioLspHealth, StudioMcpHealth, StudioRecoveryIssue,
    StudioUpdateStateSnapshot,
};

/// Studio 产品级事件信封。
///
/// `sequence` 只检测 transport lag；消费者判断新旧必须使用 payload 自带的领域 revision。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioProductEventEnvelope {
    pub event_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: StudioProductEventKind,
}

/// Studio 全局产品事件。除 `ThreadDirectoryChanged` 携带增量 payload 外，
/// 每个变体都携带可直接替换的完整领域 snapshot。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StudioProductEventKind {
    ProjectDirectoryChanged(StudioProjectDirectoryState),
    ThreadDirectoryChanged(StudioThreadDirectoryDelta),
    AgentDirectoryChanged(StudioAgentDirectoryState),
    SettingsConfigStateChanged(Box<StudioSettingsConfigStateSnapshot>),
    ModelCatalogStateChanged(Box<StudioModelCatalogStateSnapshot>),
    RecoveryStateChanged(StudioRecoveryStateSnapshot),
    McpStateChanged(StudioMcpStateSnapshot),
    LspStateChanged(StudioLspStateSnapshot),
    SkillsStateChanged(StudioSkillsStateSnapshot),
    ThreadModeCatalogChanged(ThreadModeCatalogSnapshot),
    ProviderUsageStateChanged(ProviderUsageStateSnapshot),
    ModelPerformanceStateChanged(StudioModelPerformanceSnapshot),
    SessionCostsChanged(StudioSessionCostsState),
    UpdaterStateChanged(StudioUpdateStateSnapshot),
    PersistenceStateChanged(PersistenceStateSnapshot),
    PersistenceQueueStateChanged(StudioPersistenceQueueStateSnapshot),
    AgentProfilesStateChanged(Box<StudioAgentProfilesStateSnapshot>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioProjectDirectoryState {
    pub state: ObservedResource<StudioProjectDirectoryData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioProjectDirectoryData {
    pub projects: Vec<ProjectRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryState {
    pub state: ObservedResource<StudioThreadDirectoryData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryData {
    pub threads: Vec<Thread>,
}

/// Thread directory 增量事件 payload：由常驻内存目录索引派生，不再携带全量列表。
///
/// `upserted` 按线程身份原位替换，`removed` 携带已归档/删除的 Thread id；
/// 未加载进分页窗口的增量由消费端忽略。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryDelta {
    pub revision: u64,
    pub updated_at: i64,
    pub upserted: Vec<Thread>,
    pub removed: Vec<String>,
}

/// Thread directory 的 keyset 分页页（按 `updatedAt` 倒序、id 倒序）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryPage {
    pub state: ObservedResource<StudioThreadDirectoryPageData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioThreadDirectoryPageData {
    pub threads: Vec<Thread>,
    /// `None` 表示没有更旧的页。
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentDirectoryState {
    pub state: ObservedResource<StudioAgentDirectoryData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentDirectoryData {
    pub agents: Vec<StudioAgentDirectoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSettingsConfigStateSnapshot {
    pub state: ObservedResource<pl_protocol::studio::SettingsConfigSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioModelCatalogStateSnapshot {
    pub state: ObservedResource<pl_protocol::studio::ModelCatalogSnapshot>,
}

/// 全局模型性能快照：按模型汇总与最近历史窗口，不含按 root 会话费用。
///
/// 会话费用由 [`StudioSessionCostsState`] 按 root 作用域单独交付；两者由同一计费
/// owner 投影生成，不同 scope 数据互不覆盖。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioModelPerformanceSnapshot {
    pub revision: u64,
    pub updated_at: i64,
    #[serde(default)]
    pub statistics_pending: bool,
    #[serde(default)]
    pub statistics_gap: bool,
    #[serde(default)]
    pub read_failed: bool,
    pub summaries: Vec<StudioModelPerformanceSummary>,
    pub history: Vec<StudioModelPerformanceSample>,
}

/// 一个根会话的作用域费用事实；revision 按 root 独立单调。
///
/// `cost == None` 是显式清除（例如该 root 已归档），不是零费用；清除断言该 root
/// 不再向此 scope 的消费者提供费用事实。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionCostsState {
    pub root_thread_id: String,
    pub revision: u64,
    pub updated_at: i64,
    #[serde(default)]
    pub statistics_pending: bool,
    #[serde(default)]
    pub statistics_gap: bool,
    #[serde(default)]
    pub read_failed: bool,
    pub cost: Option<StudioSessionCostSnapshot>,
}

/// 进程级持久化队列快照：协调器真实观测值加上发布 revision 与时间戳基线。
///
/// `updated_at` 是本次发布的时间基线；本地展示"最老待保存年龄"由该基线与 payload
/// 携带的现有 age 字段共同表达，不驱动每秒事件或轮询。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioPersistenceQueueStateSnapshot {
    pub revision: u64,
    pub updated_at: i64,
    pub queue: pl_protocol::PersistenceQueueSnapshot,
}

/// 配置级 Agent Profiles 的 canonical 资源快照（完整配置与逐文件诊断）。
///
/// 由配置 owner 持有；与运行期 Agent directory 是两个领域，互不替代。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentProfilesStateSnapshot {
    pub state: ObservedResource<StudioAgentProfilesData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentProfilesData {
    pub profiles: Vec<pl_protocol::AgentProfileSnapshot>,
    pub diagnostics: Vec<StudioAgentProfileDiagnostic>,
}

/// 单个 Profile 文件的诊断；只排除对应 Profile，不阻断其余配置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentProfileDiagnostic {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionCostSnapshot {
    pub root_thread_id: String,
    pub purpose_costs: Vec<StudioPurposeCostSnapshot>,
    pub estimated_costs: Vec<pl_protocol::RuntimeCostAmount>,
    pub has_unpriced_usage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPurposeCostSnapshot {
    pub purpose: Option<String>,
    pub estimated_costs: Vec<pl_protocol::RuntimeCostAmount>,
    pub has_unpriced_usage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioModelPerformanceSummary {
    pub provider_instance_id: String,
    pub provider_display_name: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
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
pub struct StudioModelPerformanceSample {
    pub completed_at: i64,
    pub provider_instance_id: String,
    pub provider_display_name: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_model: Option<String>,
    #[serde(default)]
    pub model_match_state: pl_protocol::ModelMatchState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub completion_tokens: u64,
    pub ttft_millis: Option<u64>,
    pub decode_millis: Option<u64>,
    pub total_response_millis: Option<u64>,
    pub tokens_per_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRecoveryStateSnapshot {
    pub state: ObservedResource<Vec<StudioRecoveryIssue>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpStateSnapshot {
    pub state: ObservedResource<StudioMcpStateData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpStateData {
    pub desired_config_fingerprint: String,
    pub applied_config_fingerprint: String,
    pub health: StudioMcpHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspStateSnapshot {
    pub state: ObservedResource<StudioLspHealth>,
}

/// Transport-neutral published Skills state with an owned catalog payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSkillsStateSnapshot {
    pub project_id: String,
    pub state: ObservedResource<StudioSkillsStateData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSkillsStateData {
    pub config_fingerprint: String,
    pub catalog_revision: u64,
    pub catalog: pl_tool::skill::SkillCatalog,
}

impl From<SkillsStateSnapshot> for StudioSkillsStateSnapshot {
    fn from(value: SkillsStateSnapshot) -> Self {
        Self {
            project_id: value.project_id,
            state: value.state.map(|data| StudioSkillsStateData {
                config_fingerprint: data.config_fingerprint,
                catalog_revision: data.catalog_revision,
                catalog: data.catalog.snapshot().clone(),
            }),
        }
    }
}

/// Complete Studio query snapshot shared by FRB and HTTP adapters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioStateSnapshot {
    pub runtime: crate::StudioRuntimeSnapshot,
    pub project_directory: StudioProjectDirectoryState,
    pub thread_directory: StudioThreadDirectoryPage,
    pub agent_directory: StudioAgentDirectoryState,
    pub settings_config: StudioSettingsConfigStateSnapshot,
    pub model_catalog: StudioModelCatalogStateSnapshot,
    pub recovery: StudioRecoveryStateSnapshot,
    pub mcp: StudioMcpStateSnapshot,
    pub lsp: StudioLspStateSnapshot,
    pub skills_by_project: Vec<StudioSkillsStateSnapshot>,
    pub thread_mode_catalog: ThreadModeCatalogSnapshot,
    pub provider_usage: ProviderUsageStateSnapshot,
    pub model_performance: StudioModelPerformanceSnapshot,
    pub session_costs: Vec<StudioSessionCostsState>,
    pub updater: StudioUpdateStateSnapshot,
    pub persistence: PersistenceStateSnapshot,
    pub persistence_queue: StudioPersistenceQueueStateSnapshot,
    pub agent_profiles: StudioAgentProfilesStateSnapshot,
}
