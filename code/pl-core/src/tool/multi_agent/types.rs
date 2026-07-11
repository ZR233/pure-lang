use pl_model::SharedModelProvider;
use pl_protocol::AgentStatus;
use serde::{Deserialize, Serialize};

use crate::config::{PureConfig, ReasoningEffort};

pub(super) const DEFAULT_WAIT_TIMEOUT_MS: i64 = 30_000;

#[derive(Debug, Clone)]
pub(super) struct AgentToolRuntime {
    pub provider: SharedModelProvider,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub config: Option<PureConfig>,
    pub mcp_runtime: Option<crate::mcp::McpRuntimeRegistry>,
    pub lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
    pub workspace_instructions: Option<String>,
}

impl AgentToolRuntime {
    pub fn new(
        provider: SharedModelProvider,
        reasoning_effort: Option<ReasoningEffort>,
        config: Option<PureConfig>,
        mcp_runtime: Option<crate::mcp::McpRuntimeRegistry>,
        lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
        workspace_instructions: Option<String>,
    ) -> Self {
        Self {
            provider,
            reasoning_effort,
            config,
            mcp_runtime,
            lsp_runtime,
            workspace_instructions,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpawnAgentTool {
    pub(super) runtime: AgentToolRuntime,
}

#[derive(Debug, Clone)]
pub struct WaitAgentTool;

#[derive(Debug, Clone)]
pub struct ListAgentsTool;

#[derive(Debug, Clone)]
pub struct SendInputTool {
    pub(super) runtime: AgentToolRuntime,
}

#[derive(Debug, Clone)]
pub struct CloseAgentTool;

#[derive(Debug, Clone)]
pub struct ResumeAgentTool;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SpawnAgentArgs {
    pub task_name: String,
    pub message: String,
    pub agent_type: Option<String>,
    #[serde(rename = "model")]
    pub _model: Option<String>,
    #[serde(rename = "reasoningEffort")]
    pub _reasoning_effort: Option<String>,
    pub fork_turns: Option<String>,
    #[serde(default, rename = "skillMentions")]
    pub _skill_mentions: Vec<String>,
    #[serde(default)]
    pub owned_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WaitAgentArgs {
    #[serde(rename = "target")]
    pub _target: Option<String>,
    #[serde(default, rename = "targets")]
    pub _targets: Vec<String>,
    pub timeout_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ListAgentsArgs {
    pub path_prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AgentMessageArgs {
    pub target: String,
    pub message: String,
    #[serde(default)]
    pub trigger_turn: bool,
    #[serde(default)]
    pub interrupt: bool,
    #[serde(default, rename = "skillMentions")]
    pub _skill_mentions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CloseAgentArgs {
    pub target: String,
    #[serde(default)]
    pub merge: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpawnAgentResult {
    pub agent_id: String,
    pub task_name: String,
    pub path: String,
    pub status: AgentStatus,
    pub turn_id: Option<String>,
    pub worktree: Option<crate::agent::WorktreeRef>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WaitAgentResult {
    pub message: String,
    pub timed_out: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ListAgentsResult {
    pub agents: Vec<AgentToolRecord>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompactAgentRecord {
    pub path: String,
    pub status: AgentStatus,
    pub role: String,
    pub task: String,
    pub summary: Option<String>,
    pub error: Option<String>,
}

pub(super) type AgentToolRecord = CompactAgentRecord;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MessageResult {
    pub target: String,
    pub status: AgentStatus,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SendInputResult {
    pub target: String,
    pub status: AgentStatus,
    pub interrupt: bool,
    pub queued: bool,
    pub turn_id: Option<String>,
}
