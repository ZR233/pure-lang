use std::collections::BTreeMap;

use pl_model::TokenUsage;
use pl_protocol::{TurnBillingRecord, TurnFailure};
use serde::{Deserialize, Serialize};

use crate::{AgentRoleId, AgentSession};

use crate::agent_runtime::{AgentId, ThreadId, TurnId};

use super::lifecycle::*;

/// agent 在 runtime 内的稳定身份。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentity {
    pub id: AgentId,
    pub parent_id: Option<AgentId>,
    pub role: AgentRoleId,
    pub depth: u32,
}

/// 最近一次 turn 的结构化结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnOutcome {
    pub turn_id: TurnId,
    pub thread_id: ThreadId,
    pub kind: TurnOutcomeKind,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<TurnFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_limit: Option<pl_protocol::BudgetLimitSnapshot>,
    #[serde(default)]
    pub rollover_compacted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollover_compaction_error: Option<String>,
    pub usage: TokenUsage,
    pub finished_at: i64,
}

/// checkpoint 与 durable submission 共享的进度内容。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentProgressReport {
    pub stage: AgentProgressStage,
    pub summary: String,
    pub next_step: String,
    pub revision: u64,
}

#[cfg(test)]
mod tests {
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
            serde_json::to_value(checkpoint).unwrap(),
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

/// agent 最新的显式进度 checkpoint。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentProgressCheckpoint {
    #[serde(flatten)]
    pub report: AgentProgressReport,
    pub updated_at: i64,
}

/// 一次 `report_progress` 追加到 durable 提交日志的载荷。
///
/// 写入 `thread_submissions`；主代理通过 `read_agent_submissions` 主动拉取全历史，
/// 不依赖子代理 push。detail 承载实质报告内容（替代被移除的 send_message message 体），
/// 全文返回、分页不截断。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSubmissionRecord {
    #[serde(flatten)]
    pub report: AgentProgressReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub created_at: i64,
}

/// `read_agent_submissions` 的分页结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSubmissionPage {
    pub items: Vec<AgentSubmissionRecord>,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub has_more: bool,
}

/// 一次 commit 中需要原子追加到 durable 提交日志的 typed 载荷。
#[derive(Debug, Clone)]
pub struct ProgressSubmissionCommit {
    pub report: AgentProgressReport,
    pub detail: Option<String>,
    pub created_at: i64,
}

impl ProgressSubmissionCommit {
    pub fn to_record(&self) -> AgentSubmissionRecord {
        AgentSubmissionRecord {
            report: self.report.clone(),
            detail: self.detail.clone(),
            created_at: self.created_at,
        }
    }
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
#[serde(rename_all = "camelCase")]
pub struct AgentSessionDigestMessage {
    pub role: AgentSessionDigestRole,
    pub text: String,
}

/// `read_agent_session` 的过滤结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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
///
/// 该类型只保留模型处理本次 directory 变化所需的字段，不复制完整的
/// [`AgentSnapshot`]。历史工具结果不做兼容转换；协议变化时由调用方建立新会话。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDirectoryWaitMessage {
    pub identity: AgentIdentity,
    pub lifecycle: AgentLifecycleState,
    pub activity: AgentActivityState,
    pub message: Option<AgentProgressCheckpoint>,
    pub turn_outcome: Option<TurnOutcomeKind>,
}

/// `wait_agents` 的 canonical 增量结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDirectoryWaitResult {
    pub reason: AgentDirectoryWaitReason,
    pub messages: Vec<AgentDirectoryWaitMessage>,
}

/// 可直接投影到产品协议的 agent latest snapshot。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSnapshot {
    pub identity: AgentIdentity,
    pub lifecycle: AgentLifecycleState,
    pub activity: AgentActivityState,
    pub active_turn_id: Option<TurnId>,
    pub pending_inputs: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<AgentProgressCheckpoint>,
    pub last_turn: Option<AgentTurnOutcome>,
    pub revision: u64,
    pub event_sequence: u64,
    pub updated_at: i64,
}

/// runtime 持有的 canonical session 及其统计。
#[derive(Debug, Clone)]
pub struct ThreadContextState {
    /// 产品可持久化的 session 元数据，例如标题和展示属性；框架不解释其内容。
    pub metadata: serde_json::Value,
    pub session: AgentSession,
    pub usage: TokenUsage,
    /// 按 Turn 保存的 inference 计费快照；durable truth 位于 `turns.model_json`。
    pub billing_by_turn: BTreeMap<String, TurnBillingRecord>,
    pub last_context_tokens: Option<u64>,
    /// 当前 session 下一条 durable trace 的 sequence。
    pub trace_sequence: u64,
    /// 当前 session 已提交的 canonical UI event sequence。
    pub thread_revision: u64,
}

impl ThreadContextState {
    /// 创建空 session 状态。
    pub fn empty() -> Self {
        Self {
            metadata: serde_json::Value::Null,
            session: AgentSession::new(),
            usage: TokenUsage::default(),
            billing_by_turn: BTreeMap::new(),
            last_context_tokens: None,
            trace_sequence: 0,
            thread_revision: 0,
        }
    }
}

/// 等待 agent idle 时返回的稳定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWaitResult {
    pub snapshot: AgentSnapshot,
    pub last_turn: Option<AgentTurnOutcome>,
}
