use serde::{Deserialize, Serialize};

/// Agent Profile 在子会话创建时冻结的可执行快照。
///
/// `profile_id` 同时作为运行时角色标识；provider/model/effort 与系统指令不再
/// 随磁盘配置变化，保证已启动 Agent 的行为可复现。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileSnapshot {
    pub profile_id: String,
    pub display_name: String,
    pub description: String,
    pub when_to_use: String,
    pub system_instructions: String,
    pub provider_id: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub source: String,
    pub revision: String,
    pub content_hash: String,
    #[serde(default)]
    pub system: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

const fn default_true() -> bool {
    true
}
