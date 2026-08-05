use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use pl_model::TokenUsage;
use pl_protocol::{TurnBillingRecord, TurnFailure};
use serde::{Deserialize, Serialize};

use crate::{AgentRoleId, AgentSession};

use super::{AgentId, ThreadId, TurnId};

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
    Cancelling,
}

/// mailbox envelope 与模型上下文 checkpoint 的持久投递状态。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "state"
)]
pub enum MailboxDeliveryState {
    #[default]
    Pending,
    Claimed {
        turn_id: TurnId,
        checkpoint_seq: u64,
    },
    Consumed {
        turn_id: TurnId,
        checkpoint_seq: u64,
    },
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
    pub thread_id: ThreadId,
    pub kind: TurnOutcomeKind,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<TurnFailure>,
    pub usage: TokenUsage,
    pub finished_at: i64,
}

/// agent 最新进度阶段；`ReadyForReview` 仅由产品的 durable completion 路径提升。
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

/// agent 最新的显式进度 checkpoint。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentProgressCheckpoint {
    pub stage: AgentProgressStage,
    pub summary: String,
    pub next_step: String,
    pub revision: u64,
    pub updated_at: i64,
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

/// `wait_agents` 的 canonical 结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDirectoryWaitResult {
    pub reason: AgentDirectoryWaitReason,
    pub agents: Vec<AgentSnapshot>,
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

/// 决定 mailbox 输入是否以及如何投影到用户可见 Timeline。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum MailboxPresentation {
    #[default]
    User,
    Hidden,
}

/// 已分配 turn id、可持久化和恢复的 mailbox envelope。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DurableMailboxEnvelope {
    #[serde(default)]
    pub mail_id: String,
    pub turn_id: TurnId,
    pub thread_id: ThreadId,
    pub message: String,
    #[serde(default)]
    pub presentation: MailboxPresentation,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub delivery_state: MailboxDeliveryState,
    pub queued_at: i64,
}

impl DurableMailboxEnvelope {
    pub(crate) fn claim(&mut self, turn_id: TurnId) {
        self.delivery_state = MailboxDeliveryState::Claimed {
            turn_id,
            checkpoint_seq: 0,
        };
    }

    pub(crate) fn consume(&mut self, checkpoint_seq: u64) {
        self.delivery_state = MailboxDeliveryState::Consumed {
            turn_id: self.turn_id.clone(),
            checkpoint_seq,
        };
    }
}

/// 产品提交给 runtime 的输入请求。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentSubmitRequest {
    pub thread_id: ThreadId,
    pub message: String,
    pub presentation: MailboxPresentation,
    pub metadata: serde_json::Value,
    pub mail_id: Option<String>,
    pub turn_policy: AgentTurnSubmitPolicy,
}

/// 限定一次输入必须启动新 Turn、steer 活动 Turn，或由 actor 自动选择。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentTurnSubmitPolicy {
    #[default]
    StartOrSteer,
    StartOnly,
    SteerOnly,
}

impl AgentSubmitRequest {
    /// 创建可立即启动或排队的普通输入。
    pub fn start(thread_id: ThreadId, message: impl Into<String>) -> Self {
        Self {
            thread_id,
            message: message.into(),
            presentation: MailboxPresentation::User,
            metadata: serde_json::Value::Null,
            mail_id: None,
            turn_policy: AgentTurnSubmitPolicy::StartOrSteer,
        }
    }

    /// 设置产品自定义、可持久化的输入元数据。
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// 设置此输入在 Timeline 中的展示语义。
    pub fn with_presentation(mut self, presentation: MailboxPresentation) -> Self {
        self.presentation = presentation;
        self
    }

    /// 指定传输重试使用的稳定 mailbox id；不会被模型看到。
    pub fn with_mail_id(mut self, mail_id: impl Into<String>) -> Self {
        self.mail_id = Some(mail_id.into());
        self
    }

    /// 要求 actor 以指定 Turn 语义原子接收输入。
    pub fn with_turn_policy(mut self, turn_policy: AgentTurnSubmitPolicy) -> Self {
        self.turn_policy = turn_policy;
        self
    }
}

/// 提交到目标 agent 当前 session 的输入；session 身份只能由 runtime resolver 填充。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentCurrentSessionSubmitRequest {
    pub message: String,
    pub presentation: MailboxPresentation,
    pub metadata: serde_json::Value,
    pub mail_id: Option<String>,
}

impl AgentCurrentSessionSubmitRequest {
    /// 创建投递到当前 session 的普通输入。
    pub fn start(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            presentation: MailboxPresentation::User,
            metadata: serde_json::Value::Null,
            mail_id: None,
        }
    }

    /// 设置产品自定义、可持久化的输入元数据。
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// 设置此输入在 Timeline 中的展示语义。
    pub fn with_presentation(mut self, presentation: MailboxPresentation) -> Self {
        self.presentation = presentation;
        self
    }

    /// 指定传输重试使用的稳定 mailbox id；不会被模型看到。
    pub fn with_mail_id(mut self, mail_id: impl Into<String>) -> Self {
        self.mail_id = Some(mail_id.into());
        self
    }
}

/// repository 原子提交和恢复使用的 agent 全量 durable state。
#[derive(Debug, Clone)]
pub struct ThreadActorState {
    pub snapshot: AgentSnapshot,
    pub session: ThreadContextState,
    pub pending_inputs: VecDeque<DurableMailboxEnvelope>,
    pub active_input: Option<DurableMailboxEnvelope>,
}

impl ThreadActorState {
    pub(crate) fn has_triggering_input(&self) -> bool {
        self.triggering_input_position().is_some()
    }

    pub(crate) fn triggering_input_position(&self) -> Option<usize> {
        self.pending_inputs
            .iter()
            .position(|input| matches!(input.delivery_state, MailboxDeliveryState::Pending))
    }

    pub(crate) fn refresh_mailbox_snapshot(&mut self) {
        self.snapshot.pending_inputs = self.pending_inputs.len();
    }
}

/// 新 agent 注册输入；外部资源生命周期由产品或 spawn saga 准备。
#[derive(Debug, Clone)]
pub struct AgentRegistration {
    pub identity: AgentIdentity,
    pub session: ThreadContextState,
}

/// runtime 负责 lifecycle saga 的 child agent 创建请求。
#[derive(Debug, Clone)]
pub struct AgentSpawnRequest {
    pub thread_id: ThreadId,
    pub parent_id: AgentId,
    pub role: AgentRoleId,
    pub session: ThreadContextState,
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
    /// 为 identity 对应的 Thread 创建空运行上下文。
    pub fn new(identity: AgentIdentity) -> Self {
        Self {
            identity,
            session: ThreadContextState::empty(),
        }
    }

    pub(crate) fn into_durable_state(self) -> ThreadActorState {
        let now = unix_timestamp();
        ThreadActorState {
            snapshot: AgentSnapshot {
                identity: self.identity,
                lifecycle: AgentLifecycleState::Active,
                activity: AgentActivityState::Idle,
                active_turn_id: None,
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision: 1,
                event_sequence: 1,
                updated_at: now,
            },
            session: self.session,
            pending_inputs: VecDeque::new(),
            active_input: None,
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
        input: DurableMailboxEnvelope,
        snapshot: AgentSnapshot,
    },
    TurnStarted {
        turn_id: TurnId,
        thread_id: ThreadId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        claimed_inputs: Vec<DurableMailboxEnvelope>,
        snapshot: AgentSnapshot,
    },
    ThreadOpened {
        thread_id: ThreadId,
        snapshot: AgentSnapshot,
    },
    TurnFinished {
        outcome: AgentTurnOutcome,
        snapshot: AgentSnapshot,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finalized_with_tool: Option<String>,
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
    ThreadMismatch {
        agent_id: AgentId,
        expected: ThreadId,
        actual: ThreadId,
    },
    InvalidInput(String),
    Repository(String),
    RevisionConflict {
        expected: Option<u64>,
        actual: Option<u64>,
    },
    Lifecycle(String),
    ThreadEvents(String),
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
            Self::ThreadMismatch {
                agent_id,
                expected,
                actual,
            } => write!(
                formatter,
                "agent {agent_id} canonical thread mismatch: expected {expected}, got {actual}"
            ),
            Self::InvalidInput(reason) => write!(formatter, "invalid agent input: {reason}"),
            Self::Repository(error) => write!(formatter, "agent repository failed: {error}"),
            Self::RevisionConflict { expected, actual } => write!(
                formatter,
                "agent revision conflict: expected {expected:?}, actual {actual:?}"
            ),
            Self::Lifecycle(error) => write!(formatter, "agent lifecycle failed: {error}"),
            Self::ThreadEvents(error) => write!(formatter, "thread events failed: {error}"),
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
