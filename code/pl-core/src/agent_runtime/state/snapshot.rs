use std::collections::BTreeMap;

use pl_protocol::InferenceTokenUsage;
use pl_protocol::{
    AgentProgressReport, AgentSnapshot, AgentSubmissionRecord, AgentTurnOutcome, TurnBillingRecord,
};

use crate::AgentSession;

/// Runtime 热 session 的类型化展示元数据。
///
/// 该对象保持产品无关的 workspace/title 语义；存储适配器只在 worker 落库时
/// 序列化，不允许把预编码 JSON 带入热状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadContextMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

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
    /// 产品可持久化的类型化 session 展示元数据。
    pub metadata: ThreadContextMetadata,
    pub session: AgentSession,
    pub usage: InferenceTokenUsage,
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
            metadata: ThreadContextMetadata::default(),
            session: AgentSession::new(),
            usage: InferenceTokenUsage::default(),
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
