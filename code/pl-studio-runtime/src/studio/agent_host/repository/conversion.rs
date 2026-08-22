use pl_core::{
    AgentProgressReport, AgentSubmissionRecord, AgentTurnOutcome, DurableMailboxEnvelope,
    MailboxDeliveryState, MailboxInputPayload, ThreadId, TurnId, TurnOutcomeKind,
};
use pl_protocol::{Thread as ThreadRecord, Turn, TurnState};

use crate::PureError;
use crate::studio::entity::{thread, thread_input, thread_submission, turn};

use super::input_metadata::deserialize_input_metadata;
use super::labels::{
    presentation_from_label, thread_mode_from_label, thread_status_from_label,
    turn_phase_from_label,
};
use super::{store_error, u64_from_i64};

impl TryFrom<thread_input::Model> for DurableMailboxEnvelope {
    type Error = PureError;

    fn try_from(model: thread_input::Model) -> Result<Self, Self::Error> {
        let delivery_state = match model.state.as_str() {
            "queued" => MailboxDeliveryState::Pending,
            "claimed" | "active" => MailboxDeliveryState::Claimed {
                turn_id: TurnId::new(
                    model
                        .claimed_turn_id
                        .clone()
                        .unwrap_or_else(|| model.turn_id.clone()),
                )?,
                checkpoint_seq: model
                    .checkpoint_seq
                    .map(u64_from_i64)
                    .transpose()?
                    .unwrap_or(0),
            },
            other => return Err(store_error(format!("cannot restore input state {other}"))),
        };
        let (metadata, queue_coalescing_key, budget_action) =
            deserialize_input_metadata(&model.metadata_json)?;
        Ok(Self {
            mail_id: model.mail_id,
            turn_id: TurnId::new(model.turn_id)?,
            thread_id: ThreadId::new(model.thread_id)?,
            payload: MailboxInputPayload {
                message: model.content,
                presentation: presentation_from_label(&model.presentation)?,
                metadata,
            },
            queue_coalescing_key,
            budget_action,
            delivery_state,
            queued_at: model.queued_at,
        })
    }
}

impl TryFrom<thread_submission::Model> for AgentSubmissionRecord {
    type Error = PureError;

    fn try_from(model: thread_submission::Model) -> Result<Self, Self::Error> {
        Ok(Self {
            report: AgentProgressReport {
                stage: crate::studio::agent_host::events::progress_stage_from_label(&model.stage),
                summary: model.summary,
                next_step: model.next_step,
                revision: u64_from_i64(model.revision)?,
            },
            detail: model.detail,
            created_at: model.created_at,
        })
    }
}

impl TryFrom<turn::Model> for AgentTurnOutcome {
    type Error = PureError;

    fn try_from(model: turn::Model) -> Result<Self, Self::Error> {
        let budget_limit = model
            .budget_limit_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        let kind = match model.status.as_str() {
            "completed" => TurnOutcomeKind::Completed,
            "failed" => TurnOutcomeKind::Failed,
            "interrupted" if budget_limit.is_some() => TurnOutcomeKind::BudgetLimited,
            "interrupted" => TurnOutcomeKind::Cancelled,
            other => return Err(store_error(format!("Turn {other} is not terminal"))),
        };
        Ok(Self {
            turn_id: TurnId::new(model.id)?,
            thread_id: ThreadId::new(model.thread_id)?,
            kind,
            reason: model.reason,
            failure: model
                .failure_json
                .map(|value| serde_json::from_str(&value))
                .transpose()?,
            budget_limit,
            rollover_compacted: model.rollover_compacted != 0,
            rollover_compaction_error: model.rollover_compaction_error,
            usage: serde_json::from_str(&model.usage_json)?,
            finished_at: model.completed_at.unwrap_or(model.updated_at),
        })
    }
}

impl TryFrom<thread::Model> for ThreadRecord {
    type Error = PureError;

    fn try_from(model: thread::Model) -> Result<Self, Self::Error> {
        Ok(Self {
            id: model.id,
            project_id: model.project_id,
            title: model.title,
            mode: thread_mode_from_label(&model.mode)?,
            root_thread_id: model.root_thread_id,
            parent_thread_id: model.parent_thread_id,
            role: model.role,
            agent_path: model.agent_path,
            status: thread_status_from_label(&model.status)?,
            created_at: model.created_at,
            updated_at: model.updated_at,
            archived: model.archived != 0,
        })
    }
}

impl TryFrom<turn::Model> for Turn {
    type Error = PureError;

    fn try_from(model: turn::Model) -> Result<Self, Self::Error> {
        let failure = model
            .failure_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        // 老数据兼容：schema v1 可能把等待交互的 Turn 存成
        // status=inProgress + phase=waitingInteraction。新设计下这种 Turn 应是 completed
        // （pending Interaction 是 completion boundary），读回时降级。
        let state = if model.status.as_str() == "inProgress"
            && model.phase.as_deref() == Some("waitingInteraction")
        {
            TurnState::Completed
        } else {
            match model.status.as_str() {
                "queued" => TurnState::Queued,
                "inProgress" => TurnState::InProgress {
                    phase: turn_phase_from_label(model.phase.as_deref().unwrap_or("preparing"))?,
                },
                "completed" => TurnState::Completed,
                "failed" => TurnState::Failed {
                    reason: model.reason.clone().unwrap_or_default(),
                },
                "interrupted" => TurnState::Interrupted {
                    reason: model.reason.clone().unwrap_or_default(),
                },
                other => return Err(store_error(format!("unknown Turn status {other}"))),
            }
        };
        Ok(Self {
            id: model.id,
            thread_id: model.thread_id,
            state,
            failure,
            started_at: model.started_at,
            updated_at: model.updated_at,
            completed_at: model.completed_at,
        })
    }
}
