use serde::{Deserialize, Serialize};

use crate::{AgentRoleId, ThreadId, TokenUsage, TurnId, TurnOutcome};

use super::AgentState;

/// Agent 在 runtime 内的稳定身份。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentIdentity {
    pub id: ThreadId,
    pub parent_id: Option<ThreadId>,
    pub role: AgentRoleId,
    pub depth: u32,
}

/// 最近一次 Turn 的结构化结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTurnOutcome {
    pub turn_id: TurnId,
    pub thread_id: ThreadId,
    pub outcome: TurnOutcome,
    pub usage: TokenUsage,
    pub started_at: Option<i64>,
    pub finished_at: i64,
}

/// runtime event 转换为 Agent 命令时使用的瞬时活动更新。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum AgentActivityUpdate {
    Running,
    WaitingTool,
    WaitingInteraction { interaction_id: String },
}

/// Agent 最新进度阶段；`ReadyForReview` 仅由产品的 durable completion 路径提升。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentProgressStage {
    Exploring,
    Implementing,
    Verifying,
    Blocked,
    ReadyForCompletion,
    ReadyForReview,
}

/// checkpoint 与 durable submission 共享的进度内容。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProgressReport {
    pub stage: AgentProgressStage,
    pub summary: String,
    pub next_step: String,
    pub revision: u64,
}

/// Agent 最新的显式进度 checkpoint。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProgressCheckpoint {
    #[serde(flatten)]
    pub report: AgentProgressReport,
    pub updated_at: i64,
}

/// 一次 `report_progress` 追加到 durable 提交日志的载荷。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSubmissionRecord {
    #[serde(flatten)]
    pub report: AgentProgressReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub created_at: i64,
}

/// `read_agent_submissions` 的分页结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSubmissionPage {
    pub items: Vec<AgentSubmissionRecord>,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub has_more: bool,
}

/// `read_agent_session` 可返回的公开消息角色。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionDigestRole {
    User,
    Assistant,
}

/// `read_agent_session` 的单条有界文本。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSessionDigestMessage {
    pub role: AgentSessionDigestRole,
    pub text: String,
}

/// `read_agent_session` 的过滤结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSessionDigest {
    pub through_sequence: u64,
    pub truncated: bool,
    pub messages: Vec<AgentSessionDigestMessage>,
    pub tool_names: Vec<String>,
}

/// `wait_agents` 返回的真实 directory 变化原因。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentDirectoryWaitReason {
    Progress,
    Interaction,
    Terminal,
}

/// `wait_agents` 返回的单个最新增量消息。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentDirectoryWaitMessage {
    pub identity: AgentIdentity,
    pub state: AgentState,
    pub message: Option<AgentProgressCheckpoint>,
    pub last_turn_outcome: Option<AgentTurnOutcome>,
}

/// `wait_agents` 的 canonical 增量结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentDirectoryWaitResult {
    pub reason: AgentDirectoryWaitReason,
    pub messages: Vec<AgentDirectoryWaitMessage>,
}

/// 可由所有产品直接发布和持久化的 Agent latest snapshot。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSnapshot {
    pub identity: AgentIdentity,
    pub state: AgentState,
    pub pending_inputs: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<AgentProgressCheckpoint>,
    pub last_turn: Option<AgentTurnOutcome>,
    pub revision: u64,
    pub event_sequence: u64,
    pub updated_at: i64,
}

impl AgentSnapshot {
    /// 返回 active、queued 或诊断 Turn。
    pub fn active_turn_id(&self) -> Option<&TurnId> {
        self.state.turn_id()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    #[test]
    fn progress_checkpoint_flattens_shared_report_fields() {
        let checkpoint = AgentProgressCheckpoint {
            report: AgentProgressReport {
                stage: AgentProgressStage::Verifying,
                summary: "tests passed".to_string(),
                next_step: "ship".to_string(),
                revision: 3,
            },
            updated_at: 42,
        };

        assert_eq!(
            serde_json::to_value(checkpoint).expect("serialize progress"),
            json!({
                "stage": "verifying",
                "summary": "tests passed",
                "nextStep": "ship",
                "revision": 3,
                "updatedAt": 42,
            })
        );
    }
}
