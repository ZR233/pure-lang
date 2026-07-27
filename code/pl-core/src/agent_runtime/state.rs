use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use pl_model::TokenUsage;
use pl_protocol::TurnFailure;
use serde::{Deserialize, Serialize};

use crate::{AgentRoleId, AgentSession};

use super::{AgentId, AgentWakeId, SessionId, TurnId};

/// agent 资源仍可执行工作的生命周期状态。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycleState {
    Active,
    Closing,
    Closed,
    Faulted,
}

/// agent 当前执行活动；与生命周期正交。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentActivityState {
    Idle,
    Queued,
    Running,
    WaitingTool,
    WaitingInteraction,
    WaitingAgents,
}

/// 子代理 runtime 终态如何参与父代理唤醒。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentWakePolicy {
    /// runtime turn 终态本身就是可执行的父代理事实。
    #[default]
    RuntimeTerminal,
    /// runtime 终态仅供产品收束合同；产品 durable signal 才可唤醒父代理。
    ProductGated,
}

/// 单轮执行结果，不用作 agent 生命周期。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnOutcomeKind {
    Completed,
    Cancelled,
    Failed,
    BudgetLimited,
}

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
    pub session_id: SessionId,
    pub kind: TurnOutcomeKind,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<TurnFailure>,
    pub usage: TokenUsage,
    pub finished_at: i64,
}

/// 可直接投影到产品协议的 agent latest snapshot。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSnapshot {
    pub identity: AgentIdentity,
    #[serde(default)]
    pub wake_policy: AgentWakePolicy,
    pub lifecycle: AgentLifecycleState,
    pub activity: AgentActivityState,
    pub active_turn_id: Option<TurnId>,
    pub active_session_id: Option<SessionId>,
    pub pending_inputs: usize,
    pub last_turn: Option<AgentTurnOutcome>,
    pub revision: u64,
    pub event_sequence: u64,
    pub updated_at: i64,
}

/// runtime 持有的 canonical session 及其统计。
#[derive(Debug, Clone)]
pub struct AgentSessionState {
    pub id: SessionId,
    /// 产品可持久化的 session 元数据，例如标题和展示属性；框架不解释其内容。
    pub metadata: serde_json::Value,
    pub session: AgentSession,
    pub usage: TokenUsage,
    pub last_context_tokens: Option<u64>,
    /// 当前 session 下一条 durable trace 的 sequence。
    pub trace_sequence: u64,
    /// 当前 session 已提交的 canonical UI event sequence。
    pub session_event_sequence: u64,
}

impl AgentSessionState {
    /// 创建空 session 状态。
    pub fn empty(id: SessionId) -> Self {
        Self {
            id,
            metadata: serde_json::Value::Null,
            session: AgentSession::new(),
            usage: TokenUsage::default(),
            last_context_tokens: None,
            trace_sequence: 0,
            session_event_sequence: 0,
        }
    }
}

/// 输入到达繁忙 agent 时的投递语义。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InputDelivery {
    QueueOnly,
    #[default]
    Start,
    InterruptThenStart,
}

/// 已分配 turn id、可持久化和恢复的输入。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PendingAgentInput {
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_id: Option<AgentWakeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wake_signal_ids: Vec<String>,
    pub session_id: SessionId,
    pub message: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub queued_at: i64,
}

/// runtime 已接受的订阅唤醒凭据；用于跨重启抑制同一事实的重复续轮。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedAgentWake {
    pub wake_id: AgentWakeId,
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signal_ids: Vec<String>,
    pub accepted_at: i64,
}

/// 产品提交给 runtime 的输入请求。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentSubmitRequest {
    pub session_id: SessionId,
    pub message: String,
    pub metadata: serde_json::Value,
    pub delivery: InputDelivery,
    pub wake_id: Option<AgentWakeId>,
    pub wake_signal_ids: Vec<String>,
}

impl AgentSubmitRequest {
    /// 创建可立即启动或排队的普通输入。
    pub fn start(session_id: SessionId, message: impl Into<String>) -> Self {
        Self {
            session_id,
            message: message.into(),
            metadata: serde_json::Value::Null,
            delivery: InputDelivery::Start,
            wake_id: None,
            wake_signal_ids: Vec::new(),
        }
    }

    /// 设置产品自定义、可持久化的输入元数据。
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// 设置明确的输入投递语义。
    pub fn with_delivery(mut self, delivery: InputDelivery) -> Self {
        self.delivery = delivery;
        self
    }

    /// 标记此输入为可跨重试去重的订阅唤醒。
    pub fn with_wake_id(mut self, wake_id: AgentWakeId) -> Self {
        self.wake_id = Some(wake_id);
        self
    }

    /// 记录组成该唤醒的 durable product signal，用于跨批次、跨重启去重。
    pub fn with_wake_signal_ids(mut self, signal_ids: Vec<String>) -> Self {
        self.wake_signal_ids = signal_ids;
        self
    }
}

/// 提交到目标 agent 当前 session 的输入；session 身份只能由 runtime resolver 填充。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentCurrentSessionSubmitRequest {
    pub message: String,
    pub metadata: serde_json::Value,
    pub delivery: InputDelivery,
    pub wake_id: Option<AgentWakeId>,
    pub wake_signal_ids: Vec<String>,
}

impl AgentCurrentSessionSubmitRequest {
    /// 创建投递到当前 session 的普通输入。
    pub fn start(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            metadata: serde_json::Value::Null,
            delivery: InputDelivery::Start,
            wake_id: None,
            wake_signal_ids: Vec::new(),
        }
    }

    /// 设置产品自定义、可持久化的输入元数据。
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// 设置明确的输入投递语义。
    pub fn with_delivery(mut self, delivery: InputDelivery) -> Self {
        self.delivery = delivery;
        self
    }

    /// 标记此输入为可跨重试去重的订阅唤醒。
    pub fn with_wake_id(mut self, wake_id: AgentWakeId) -> Self {
        self.wake_id = Some(wake_id);
        self
    }

    /// 记录组成该唤醒的 durable product signal，用于跨批次、跨重启去重。
    pub fn with_wake_signal_ids(mut self, signal_ids: Vec<String>) -> Self {
        self.wake_signal_ids = signal_ids;
        self
    }
}

/// actor 内部解析出的 owner-bound current session capability。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAgentSessionTarget {
    pub(crate) root_agent_id: AgentId,
    pub(crate) agent_id: AgentId,
    pub(crate) session_id: SessionId,
    pub(crate) owner_revision: u64,
}

/// repository 原子提交和恢复使用的 agent 全量 durable state。
#[derive(Debug, Clone)]
pub struct AgentDurableState {
    pub snapshot: AgentSnapshot,
    pub sessions: BTreeMap<SessionId, AgentSessionState>,
    pub pending_inputs: VecDeque<PendingAgentInput>,
    pub accepted_wakes: BTreeMap<AgentWakeId, AcceptedAgentWake>,
}

/// 新 agent 注册输入；外部资源生命周期由产品或 spawn saga 准备。
#[derive(Debug, Clone)]
pub struct AgentRegistration {
    pub identity: AgentIdentity,
    pub wake_policy: AgentWakePolicy,
    pub sessions: Vec<AgentSessionState>,
}

/// runtime 负责 lifecycle saga 的 child agent 创建请求。
#[derive(Debug, Clone)]
pub struct AgentSpawnRequest {
    pub parent_id: AgentId,
    pub role: AgentRoleId,
    pub wake_policy: AgentWakePolicy,
    pub session: AgentSessionState,
    pub initial_message: Option<String>,
    pub metadata: serde_json::Value,
}

/// child agent 注册完成后的稳定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpawnResult {
    pub snapshot: AgentSnapshot,
    pub initial_turn_id: Option<TurnId>,
}

impl AgentRegistration {
    /// 创建带一个空 session 的 agent 注册。
    pub fn with_session(identity: AgentIdentity, session_id: SessionId) -> Self {
        Self {
            identity,
            wake_policy: AgentWakePolicy::RuntimeTerminal,
            sessions: vec![AgentSessionState::empty(session_id)],
        }
    }

    pub(crate) fn into_durable_state(self) -> AgentDurableState {
        let now = unix_timestamp();
        AgentDurableState {
            snapshot: AgentSnapshot {
                identity: self.identity,
                wake_policy: self.wake_policy,
                lifecycle: AgentLifecycleState::Active,
                activity: AgentActivityState::Idle,
                active_turn_id: None,
                active_session_id: None,
                pending_inputs: 0,
                last_turn: None,
                revision: 1,
                event_sequence: 1,
                updated_at: now,
            },
            sessions: self
                .sessions
                .into_iter()
                .map(|session| (session.id.clone(), session))
                .collect(),
            pending_inputs: VecDeque::new(),
            accepted_wakes: BTreeMap::new(),
        }
    }
}

/// 提交后广播的 framework runtime 事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeEvent {
    pub agent_id: AgentId,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: AgentRuntimeEventKind,
}

/// runtime 事件的结构化类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum AgentRuntimeEventKind {
    Registered {
        snapshot: AgentSnapshot,
    },
    StateChanged {
        snapshot: AgentSnapshot,
    },
    TurnQueued {
        input: PendingAgentInput,
        snapshot: AgentSnapshot,
    },
    TurnStarted {
        turn_id: TurnId,
        session_id: SessionId,
        snapshot: AgentSnapshot,
    },
    SessionOpened {
        session_id: SessionId,
        snapshot: AgentSnapshot,
    },
    TurnFinished {
        outcome: AgentTurnOutcome,
        snapshot: AgentSnapshot,
    },
    RecoveryCancelledTurn {
        outcome: AgentTurnOutcome,
        snapshot: AgentSnapshot,
    },
    Faulted {
        reason: String,
        snapshot: AgentSnapshot,
    },
}

/// 等待 agent idle 时返回的稳定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWaitResult {
    pub snapshot: AgentSnapshot,
    pub last_turn: Option<AgentTurnOutcome>,
}

/// 非泛型 handle 使用的 runtime 错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRuntimeError {
    NotFound(AgentId),
    AlreadyExists(AgentId),
    NotActive(AgentId, AgentLifecycleState),
    NoActiveTurn(AgentId),
    TurnMismatch {
        expected: TurnId,
        actual: TurnId,
    },
    SessionNotOwned {
        agent_id: AgentId,
        session_id: SessionId,
    },
    CurrentSessionUnavailable {
        agent_id: AgentId,
        session_count: usize,
    },
    Repository(String),
    RevisionConflict {
        expected: Option<u64>,
        actual: Option<u64>,
    },
    Lifecycle(String),
    SessionEvents(String),
    ChannelClosed,
    TimedOut,
}

pub type AgentRuntimeResult<T> = std::result::Result<T, AgentRuntimeError>;

impl fmt::Display for AgentRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(id) => write!(formatter, "agent not found: {id}"),
            Self::AlreadyExists(id) => write!(formatter, "agent already exists: {id}"),
            Self::NotActive(id, state) => write!(formatter, "agent {id} is not active: {state:?}"),
            Self::NoActiveTurn(id) => write!(formatter, "agent has no active turn: {id}"),
            Self::TurnMismatch { expected, actual } => {
                write!(
                    formatter,
                    "active turn mismatch: expected {expected}, got {actual}"
                )
            }
            Self::SessionNotOwned {
                agent_id,
                session_id,
            } => write!(
                formatter,
                "agent {agent_id} does not own session {session_id}"
            ),
            Self::CurrentSessionUnavailable {
                agent_id,
                session_count,
            } => write!(
                formatter,
                "agent {agent_id} has no unambiguous current session ({session_count} owned)"
            ),
            Self::Repository(error) => write!(formatter, "agent repository failed: {error}"),
            Self::RevisionConflict { expected, actual } => write!(
                formatter,
                "agent revision conflict: expected {expected:?}, actual {actual:?}"
            ),
            Self::Lifecycle(error) => write!(formatter, "agent lifecycle failed: {error}"),
            Self::SessionEvents(error) => write!(formatter, "session events failed: {error}"),
            Self::ChannelClosed => formatter.write_str("agent runtime channel closed"),
            Self::TimedOut => formatter.write_str("agent wait timed out"),
        }
    }
}

impl std::error::Error for AgentRuntimeError {}

pub(crate) fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
