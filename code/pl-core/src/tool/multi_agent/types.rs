use std::path::PathBuf;

use pl_model::SharedModelProvider;
use pl_protocol::AgentStatus;
use serde::{Deserialize, Serialize};

use crate::agent::AgentRecord;
use crate::config::{PureConfig, ReasoningEffort};
use crate::tool::recoverable::RecoverableSubagentFailure;
use crate::turn::{CompileMode, TurnBudget};

pub(super) const DEFAULT_WAIT_TIMEOUT_MS: i64 = 30_000;

#[derive(Debug, Clone)]
pub struct SpawnAgentTool {
    pub provider: SharedModelProvider,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub config: Option<PureConfig>,
    pub workspace_instructions: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WaitAgentTool;

#[derive(Debug, Clone)]
pub struct ListAgentsTool;

#[derive(Debug, Clone)]
pub struct SendMessageTool;

#[derive(Debug, Clone)]
pub struct FollowupTaskTool {
    pub provider: SharedModelProvider,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub config: Option<PureConfig>,
    pub workspace_instructions: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CloseAgentTool;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
pub(super) struct SpawnAgentArgs {
    pub task_name: String,
    pub message: String,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub fork_turns: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WaitAgentArgs {
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CloseAgentArgs {
    pub target: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpawnAgentResult {
    pub agent_id: String,
    pub task_name: String,
    pub path: String,
    pub status: AgentStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WaitAgentResult {
    pub message: String,
    pub timed_out: bool,
    pub recoverable_failures: Vec<RecoverableSubagentFailure>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ListAgentsResult {
    pub agents: Vec<AgentRecord>,
    pub recoverable_failures: Vec<RecoverableSubagentFailure>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MessageResult {
    pub target: String,
    pub status: AgentStatus,
}

pub(crate) struct AgentRunConfig {
    pub provider: SharedModelProvider,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub config: Option<PureConfig>,
    pub workspace_instructions: Option<String>,
    pub workspace_root: PathBuf,
    pub options: crate::TurnOptions,
    pub agent_control: crate::AgentControl,
    pub event_tx: pl_protocol::AgentEventSender,
    pub agent_id: String,
    pub agent_path: String,
    pub role: String,
    pub message: String,
    pub mode: CompileMode,
    pub budget: TurnBudget,
}
