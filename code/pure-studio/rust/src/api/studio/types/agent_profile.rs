use serde::{Deserialize, Serialize};

/// Agent Profile 设置页使用的只读快照；系统 Profile 的 `system` 为 true。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentProfileDto {
    pub profile_id: String,
    pub display_name: String,
    pub description: String,
    pub when_to_use: String,
    pub system_instructions: String,
    pub provider_id: String,
    pub model: String,
    pub effort: Option<String>,
    pub source: String,
    pub revision: String,
    pub content_hash: String,
    pub system: bool,
    pub enabled: bool,
}
