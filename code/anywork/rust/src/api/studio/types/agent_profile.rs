use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeAgentWorkspaceMode {
    Unrestricted,
    Directory,
    Worktree,
}

impl From<pl_protocol::AgentWorkspaceMode> for BridgeAgentWorkspaceMode {
    fn from(value: pl_protocol::AgentWorkspaceMode) -> Self {
        match value {
            pl_protocol::AgentWorkspaceMode::Unrestricted => Self::Unrestricted,
            pl_protocol::AgentWorkspaceMode::Directory => Self::Directory,
            pl_protocol::AgentWorkspaceMode::Worktree => Self::Worktree,
        }
    }
}

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
    pub workspace_mode: BridgeAgentWorkspaceMode,
}

impl From<pl_protocol::AgentProfileSnapshot> for BridgeAgentProfileDto {
    fn from(profile: pl_protocol::AgentProfileSnapshot) -> Self {
        Self {
            profile_id: profile.profile_id,
            display_name: profile.display_name,
            description: profile.description,
            when_to_use: profile.when_to_use,
            system_instructions: profile.system_instructions,
            provider_id: profile.provider_id,
            model: profile.model,
            effort: profile.effort,
            source: profile.source,
            revision: profile.revision,
            content_hash: profile.content_hash,
            system: profile.system,
            enabled: profile.enabled,
            workspace_mode: profile.workspace_mode.into(),
        }
    }
}
