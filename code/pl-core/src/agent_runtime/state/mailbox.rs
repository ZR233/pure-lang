use pl_protocol::{
    ConversationRecoveryMode, ConversationRecoveryRecord, InteractionRequest, ThreadAttachment,
};
use serde::{Deserialize, Serialize};

use crate::agent_runtime::{ThreadId, TurnId};

use super::{MailboxCommand, MailboxDeliveryState, MailboxTransitionError};

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

/// mailbox 输入对目标 Turn 预算的影响。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MailboxBudgetAction {
    #[default]
    Preserve,
    Refresh,
}

impl MailboxBudgetAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Refresh => "refresh",
        }
    }

    pub fn from_persisted_str(value: &str) -> Option<Self> {
        match value {
            "preserve" => Some(Self::Preserve),
            "refresh" => Some(Self::Refresh),
            _ => None,
        }
    }

    pub const fn is_preserve(&self) -> bool {
        matches!(self, Self::Preserve)
    }
}

/// mailbox 请求与持久化 envelope 共享的输入内容。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MailboxInputPayload {
    pub message: String,
    pub attachments: Vec<ThreadAttachment>,
    #[serde(default)]
    pub presentation: MailboxPresentation,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl MailboxInputPayload {
    pub fn user(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            attachments: Vec::new(),
            presentation: MailboxPresentation::User,
            metadata: serde_json::Value::Null,
        }
    }
}

/// 已分配 turn id、可持久化和恢复的 mailbox envelope。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DurableMailboxEnvelope {
    #[serde(default)]
    pub mail_id: String,
    pub turn_id: TurnId,
    pub thread_id: ThreadId,
    #[serde(flatten)]
    pub payload: MailboxInputPayload,
    /// 只合并队首连续 pending 输入的通用 key；不会进入模型提示词。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_coalescing_key: Option<String>,
    #[serde(default, skip_serializing_if = "MailboxBudgetAction::is_preserve")]
    pub budget_action: MailboxBudgetAction,
    #[serde(default)]
    pub delivery_state: MailboxDeliveryState,
    pub queued_at: i64,
}

impl DurableMailboxEnvelope {
    pub(crate) fn claim(&mut self, turn_id: TurnId) -> Result<bool, MailboxTransitionError> {
        let decision = self
            .delivery_state
            .clone()
            .decide(MailboxCommand::Claim { turn_id })?;
        self.delivery_state = decision.next_state;
        Ok(decision.changed)
    }

    pub(crate) fn consume(&mut self, checkpoint_seq: u64) -> Result<bool, MailboxTransitionError> {
        let decision = self
            .delivery_state
            .clone()
            .decide(MailboxCommand::Consume {
                turn_id: self.turn_id.clone(),
                checkpoint_seq,
            })?;
        self.delivery_state = decision.next_state;
        Ok(decision.changed)
    }

    pub(crate) fn requeue(&mut self, turn_id: TurnId) -> Result<bool, MailboxTransitionError> {
        let decision = self
            .delivery_state
            .clone()
            .decide(MailboxCommand::Requeue)?;
        self.delivery_state = decision.next_state;
        self.turn_id = turn_id;
        Ok(decision.changed)
    }
}

/// 产品提交给 runtime 的输入请求。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentSubmitRequest {
    pub thread_id: ThreadId,
    pub payload: MailboxInputPayload,
    pub queue_coalescing_key: Option<String>,
    pub mail_id: Option<String>,
    pub turn_policy: AgentTurnSubmitPolicy,
    pub budget_action: MailboxBudgetAction,
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
    #[serde(flatten)]
    pub facts: ConversationRecoveryFacts,
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

/// conversation recovery 预览与提交结果共享的稳定 transcript 事实。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecoveryFacts {
    pub recovery_revision: u64,
    pub before_transcript_hash: String,
    pub after_transcript_hash: String,
}

/// 已提交 conversation recovery 的稳定结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecoveryResult {
    pub recovery_id: String,
    pub mode: ConversationRecoveryMode,
    #[serde(flatten)]
    pub facts: ConversationRecoveryFacts,
    pub runtime_revision: u64,
    pub thread_revision: u64,
    pub removed_item_count: u64,
    pub removed_input_count: u64,
}

impl From<ConversationRecoveryRecord> for ConversationRecoveryResult {
    fn from(record: ConversationRecoveryRecord) -> Self {
        Self {
            recovery_id: record.recovery_id,
            mode: record.mode,
            facts: ConversationRecoveryFacts {
                recovery_revision: record.revision,
                before_transcript_hash: record.before_transcript_hash,
                after_transcript_hash: record.after_transcript_hash,
            },
            runtime_revision: record.runtime_revision,
            thread_revision: record.thread_revision,
            removed_item_count: record.removed_item_count,
            removed_input_count: record.removed_input_count,
        }
    }
}

impl From<&ConversationRecoveryRecord> for ConversationRecoveryResult {
    fn from(record: &ConversationRecoveryRecord) -> Self {
        Self {
            recovery_id: record.recovery_id.clone(),
            mode: record.mode,
            facts: ConversationRecoveryFacts {
                recovery_revision: record.revision,
                before_transcript_hash: record.before_transcript_hash.clone(),
                after_transcript_hash: record.after_transcript_hash.clone(),
            },
            runtime_revision: record.runtime_revision,
            thread_revision: record.thread_revision,
            removed_item_count: record.removed_item_count,
            removed_input_count: record.removed_input_count,
        }
    }
}

impl AgentSubmitRequest {
    /// 创建可立即启动或排队的普通输入。
    pub fn start(thread_id: ThreadId, message: impl Into<String>) -> Self {
        Self {
            thread_id,
            payload: MailboxInputPayload::user(message),
            queue_coalescing_key: None,
            mail_id: None,
            turn_policy: AgentTurnSubmitPolicy::StartOrSteer,
            budget_action: MailboxBudgetAction::Preserve,
        }
    }

    /// 设置产品自定义、可持久化的输入元数据。
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.payload.metadata = metadata;
        self
    }

    /// 设置随输入持久化并投影到 Timeline 的 typed 附件清单。
    pub fn with_attachments(mut self, attachments: Vec<ThreadAttachment>) -> Self {
        self.payload.attachments = attachments;
        self
    }

    /// 合并队首连续、key 相同的 queued 输入，并在同一 Turn 中按顺序消费。
    pub fn with_queue_coalescing_key(mut self, key: impl Into<String>) -> Self {
        self.queue_coalescing_key = Some(key.into());
        self
    }

    /// 设置此输入在 Timeline 中的展示语义。
    pub fn with_presentation(mut self, presentation: MailboxPresentation) -> Self {
        self.payload.presentation = presentation;
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

    /// 设置输入被 runtime 接受时如何处理目标 Turn 的预算。
    pub fn with_budget_action(mut self, budget_action: MailboxBudgetAction) -> Self {
        self.budget_action = budget_action;
        self
    }
}

/// 提交到目标 agent 当前 session 的输入；session 身份只能由 runtime resolver 填充。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentCurrentSessionSubmitRequest {
    pub payload: MailboxInputPayload,
    pub mail_id: Option<String>,
    pub budget_action: MailboxBudgetAction,
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
            payload: MailboxInputPayload::user(message),
            mail_id: None,
            budget_action: MailboxBudgetAction::Preserve,
        }
    }

    /// 设置产品自定义、可持久化的输入元数据。
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.payload.metadata = metadata;
        self
    }

    /// 设置此输入在 Timeline 中的展示语义。
    pub fn with_presentation(mut self, presentation: MailboxPresentation) -> Self {
        self.payload.presentation = presentation;
        self
    }

    /// 指定传输重试使用的稳定 mailbox id；不会被模型看到。
    pub fn with_mail_id(mut self, mail_id: impl Into<String>) -> Self {
        self.mail_id = Some(mail_id.into());
        self
    }

    /// 设置输入被 runtime 接受时如何处理目标 Turn 的预算。
    pub fn with_budget_action(mut self, budget_action: MailboxBudgetAction) -> Self {
        self.budget_action = budget_action;
        self
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn shared_payload_and_recovery_facts_remain_flat_on_the_wire() {
        let envelope = DurableMailboxEnvelope {
            mail_id: "mail-1".to_string(),
            turn_id: TurnId::new("turn-1").unwrap(),
            thread_id: ThreadId::new("thread-1").unwrap(),
            payload: MailboxInputPayload {
                message: "hello".to_string(),
                attachments: Vec::new(),
                presentation: MailboxPresentation::Hidden,
                metadata: json!({"kind": "test"}),
            },
            queue_coalescing_key: None,
            budget_action: MailboxBudgetAction::Refresh,
            delivery_state: MailboxDeliveryState::default(),
            queued_at: 7,
        };
        let envelope_json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(envelope_json["message"], "hello");
        assert_eq!(envelope_json["attachments"], json!([]));
        assert_eq!(envelope_json["presentation"], json!({"type": "hidden"}));
        assert_eq!(envelope_json["metadata"], json!({"kind": "test"}));
        assert_eq!(envelope_json["budgetAction"], "refresh");
        assert!(envelope_json.get("payload").is_none());
        assert_eq!(
            serde_json::from_value::<DurableMailboxEnvelope>(envelope_json).unwrap(),
            envelope
        );

        let result = ConversationRecoveryResult {
            recovery_id: "recovery-1".to_string(),
            mode: ConversationRecoveryMode::RewindTail,
            facts: ConversationRecoveryFacts {
                recovery_revision: 3,
                before_transcript_hash: "before".to_string(),
                after_transcript_hash: "after".to_string(),
            },
            runtime_revision: 4,
            thread_revision: 5,
            removed_item_count: 6,
            removed_input_count: 1,
        };
        let result_json = serde_json::to_value(&result).unwrap();
        assert_eq!(result_json["recoveryRevision"], 3);
        assert_eq!(result_json["beforeTranscriptHash"], "before");
        assert_eq!(result_json["afterTranscriptHash"], "after");
        assert!(result_json.get("facts").is_none());
        assert_eq!(
            serde_json::from_value::<ConversationRecoveryResult>(result_json).unwrap(),
            result
        );
    }

    #[test]
    fn canonical_flat_mailbox_and_recovery_json_are_deserializable() {
        let envelope = serde_json::from_value::<DurableMailboxEnvelope>(json!({
            "mailId": "mail-1",
            "turnId": "turn-1",
            "threadId": "thread-1",
            "message": "hello",
            "attachments": [],
            "presentation": { "type": "hidden" },
            "metadata": { "kind": "test" },
            "deliveryState": { "kind": "pending", "data": null },
            "queuedAt": 7
        }))
        .unwrap();
        assert_eq!(envelope.payload.message, "hello");
        assert!(envelope.payload.attachments.is_empty());
        assert_eq!(envelope.payload.presentation, MailboxPresentation::Hidden);
        assert_eq!(envelope.payload.metadata, json!({"kind": "test"}));
        assert_eq!(envelope.budget_action, MailboxBudgetAction::Preserve);

        let result = serde_json::from_value::<ConversationRecoveryResult>(json!({
            "recoveryId": "recovery-1",
            "mode": "rewindTail",
            "recoveryRevision": 3,
            "beforeTranscriptHash": "before",
            "afterTranscriptHash": "after",
            "runtimeRevision": 4,
            "threadRevision": 5,
            "removedItemCount": 6,
            "removedInputCount": 1
        }))
        .unwrap();
        assert_eq!(result.facts.recovery_revision, 3);
        assert_eq!(result.facts.before_transcript_hash, "before");
        assert_eq!(result.facts.after_transcript_hash, "after");
    }
}
