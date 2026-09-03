//! AgentSession 内固定 Plan 状态机的持久化协议。

use serde::{Deserialize, Serialize};

pub const AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID: &str = "plan_confirmation";

/// 一份 AgentSession Plan 的封闭生命周期状态。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionPlanPhase {
    #[default]
    Drafting,
    AwaitingConfirmation,
    RevisionRequested,
    Approved,
}

impl AgentSessionPlanPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Drafting => "drafting",
            Self::AwaitingConfirmation => "awaitingConfirmation",
            Self::RevisionRequested => "revisionRequested",
            Self::Approved => "approved",
        }
    }
}

/// 能触发 Plan 状态变化的穷尽操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionPlanOperation {
    Submit,
    Approve,
    RequestRevision,
    Restart,
}

impl AgentSessionPlanOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::Approve => "approve",
            Self::RequestRevision => "requestRevision",
            Self::Restart => "restart",
        }
    }
}

/// Plan 操作由模型工具还是用户 Interaction 触发。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionPlanTransitionActor {
    Agent,
    User,
}

/// 当前 Plan Markdown 文档。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPlanDocument {
    pub version: u64,
    pub markdown: String,
    pub content_hash: String,
}

/// 一次成功的固定 Plan 状态转换。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPlanTransitionRecord {
    pub revision: u64,
    pub operation: AgentSessionPlanOperation,
    pub source_state: AgentSessionPlanPhase,
    pub target_state: AgentSessionPlanPhase,
    pub operation_id: String,
    pub reason: String,
    pub transitioned_at: i64,
}

/// mutation 的有界幂等回执。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPlanOperationReceipt {
    pub operation_id: String,
    pub argument_hash: String,
    pub operation_revision: u64,
}

/// 与 Agent session 一起持久化的 Plan 热状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPlanState {
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub state: AgentSessionPlanPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<AgentSessionPlanDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_interaction_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_revision_feedback: Option<String>,
    #[serde(default)]
    pub history_tail: Vec<AgentSessionPlanTransitionRecord>,
    #[serde(default)]
    pub archived_transition_count: u64,
    #[serde(default)]
    pub archived_transition_digest: String,
    #[serde(default)]
    pub operation_receipts: Vec<AgentSessionPlanOperationReceipt>,
    #[serde(default)]
    pub updated_at: i64,
}

/// Plan confirmation Interaction 与状态机 submit 命令的 typed 绑定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPlanConfirmationPurpose {
    pub expected_revision: u64,
    pub operation_id: String,
    pub argument_hash: String,
    pub plan_hash: String,
}

/// 当前状态下的一条固定可用转换。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPlanAvailableTransition {
    pub operation: AgentSessionPlanOperation,
    pub target_state: AgentSessionPlanPhase,
    pub actor: AgentSessionPlanTransitionActor,
    pub condition: String,
    pub action: String,
}

/// 工具和错误共用的 canonical Plan 投影。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPlanSnapshot {
    pub revision: u64,
    pub state: AgentSessionPlanPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<AgentSessionPlanDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_interaction_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_revision_feedback: Option<String>,
    pub updated_at: i64,
    pub allowed_transitions: Vec<AgentSessionPlanAvailableTransition>,
}

/// Plan mutation 的稳定结果码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionPlanResultCode {
    Submitted,
    Restarted,
    Approved,
    RevisionRequested,
    AlreadyApplied,
    InvalidState,
    StaleRevision,
    OperationIdentityConflict,
    InvalidInteraction,
    InvalidResolution,
}

/// 状态拒绝时给模型的完整诊断，而不是只返回一条自然语言错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPlanTransitionError {
    pub code: AgentSessionPlanResultCode,
    pub message: String,
    pub attempted_operation: AgentSessionPlanOperation,
    pub current_state: AgentSessionPlanPhase,
    pub current_revision: u64,
    pub allowed_transitions: Vec<AgentSessionPlanAvailableTransition>,
    pub failed_condition: String,
    pub recovery_actions: Vec<String>,
}

/// Plan mutation 的统一工具响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPlanMutationResponse {
    pub accepted: bool,
    pub code: AgentSessionPlanResultCode,
    pub operation: AgentSessionPlanOperation,
    pub operation_revision: u64,
    pub snapshot: AgentSessionPlanSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AgentSessionPlanTransitionError>,
}
