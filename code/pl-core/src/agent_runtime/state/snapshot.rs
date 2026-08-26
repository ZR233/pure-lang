use std::collections::BTreeMap;

use pl_model::TokenUsage;
use pl_protocol::{
    AgentProgressReport, AgentSnapshot, AgentSubmissionRecord, AgentTurnOutcome, TurnBillingRecord,
};

use crate::AgentSession;

/// 一次 commit 中需要原子追加到 durable 提交日志的 typed 载荷。
#[derive(Debug, Clone)]
pub struct ProgressSubmissionCommit {
    pub report: AgentProgressReport,
    pub detail: Option<String>,
    pub created_at: i64,
}

impl From<&ProgressSubmissionCommit> for AgentSubmissionRecord {
    fn from(commit: &ProgressSubmissionCommit) -> Self {
        Self {
            report: commit.report.clone(),
            detail: commit.detail.clone(),
            created_at: commit.created_at,
        }
    }
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

/// 等待 Agent idle 时返回的稳定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWaitResult {
    pub snapshot: AgentSnapshot,
    pub last_turn: Option<AgentTurnOutcome>,
}
