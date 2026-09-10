//! Persisted Studio execution policy.
use pl_model::completion::OpenAiCompactionMode;
use serde::{Deserialize, Serialize};

use crate::approval::PermissionMode;
use pl_tool::workspace::ToolCapabilityConfig;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    #[serde(default, skip_serializing_if = "PermissionMode::is_default")]
    pub permission_mode: PermissionMode,
    #[serde(default, skip_serializing_if = "ToolCapabilityConfig::is_default")]
    pub tool_capabilities: ToolCapabilityConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_mcp_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "OpenAiCompactionMode::is_default")]
    pub openai_compaction_mode: OpenAiCompactionMode,
}

impl RuntimeConfig {
    pub fn is_empty(&self) -> bool {
        self.permission_mode.is_default()
            && self.tool_capabilities.is_default()
            && self.active_skills.is_empty()
            && self.active_mcp_servers.is_empty()
            && self.openai_compaction_mode.is_default()
    }
}
