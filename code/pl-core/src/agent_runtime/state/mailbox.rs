use pl_protocol::{ConversationRecoveryMode, InteractionRequest};
use serde::{Deserialize, Serialize};

use crate::agent_runtime::{ThreadId, TurnId};

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
    /// 只合并队首连续 pending 输入的通用 key；不会进入模型提示词。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_coalescing_key: Option<String>,
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
    pub queue_coalescing_key: Option<String>,
    pub mail_id: Option<String>,
    pub turn_policy: AgentTurnSubmitPolicy,
}

/// 限定一次输入必须启动新 Turn、steer 活动 Turn，或由 actor 自动选择。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentTurnSubmitPolicy {
    #[default]
    StartOrSteer,
    /// Idle 时启动新 Turn；已有活动 Turn 时持久排入下一 Turn，绝不 steer。
    StartOrQueue,
    StartOnly,
    SteerOnly,
}

/// 产品根据 durable Turn/input 事实构造的连续对话恢复目标。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecoveryTarget {
    pub mode: ConversationRecoveryMode,
    #[serde(default)]
    pub turn_ids: Vec<String>,
    /// 按 mailbox/Turn 顺序排列的 canonical user-message hash。
    #[serde(default)]
    pub input_hashes: Vec<String>,
}

/// 不产生服务端临时状态的 conversation recovery CAS 预览。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecoveryPreview {
    pub target: ConversationRecoveryTarget,
    pub expected_runtime_revision: u64,
    pub expected_thread_revision: u64,
    pub recovery_revision: u64,
    pub before_transcript_hash: String,
    pub after_transcript_hash: String,
    pub retained_item_count: u64,
    pub removed_item_count: u64,
    pub removed_input_count: u64,
}

/// 提交 conversation recovery 的幂等请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecoveryRequest {
    pub recovery_id: String,
    pub preview: ConversationRecoveryPreview,
}

/// 已提交 conversation recovery 的稳定结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecoveryResult {
    pub recovery_id: String,
    pub mode: ConversationRecoveryMode,
    pub recovery_revision: u64,
    pub runtime_revision: u64,
    pub thread_revision: u64,
    pub before_transcript_hash: String,
    pub after_transcript_hash: String,
    pub removed_item_count: u64,
    pub removed_input_count: u64,
}

impl AgentSubmitRequest {
    /// 创建可立即启动或排队的普通输入。
    pub fn start(thread_id: ThreadId, message: impl Into<String>) -> Self {
        Self {
            thread_id,
            message: message.into(),
            presentation: MailboxPresentation::User,
            metadata: serde_json::Value::Null,
            queue_coalescing_key: None,
            mail_id: None,
            turn_policy: AgentTurnSubmitPolicy::StartOrSteer,
        }
    }

    /// 设置产品自定义、可持久化的输入元数据。
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// 合并队首连续、key 相同的 queued 输入，并在同一 Turn 中按顺序消费。
    pub fn with_queue_coalescing_key(mut self, key: impl Into<String>) -> Self {
        self.queue_coalescing_key = Some(key.into());
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

/// 将 resolved Interaction 与后续 mailbox 输入作为一个原子 continuation 提交。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentInteractionContinuationRequest {
    pub interaction: InteractionRequest,
    pub input: AgentCurrentSessionSubmitRequest,
}

impl AgentInteractionContinuationRequest {
    pub fn new(interaction: InteractionRequest, input: AgentCurrentSessionSubmitRequest) -> Self {
        Self { interaction, input }
    }

    /// 返回 Interaction resolution 使用的稳定 mailbox ID。
    pub fn stable_mail_id(interaction_id: &str) -> String {
        format!("interaction-resolution:{interaction_id}")
    }
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
