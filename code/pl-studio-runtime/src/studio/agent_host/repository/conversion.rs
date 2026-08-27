use pl_core::{
    AgentProgressReport, AgentState, AgentSubmissionRecord, AgentTurnOutcome,
    DurableMailboxEnvelope, MailboxDeliveryState, MailboxInputPayload, ThreadId, TurnId,
};
use pl_protocol::{
    Thread as ThreadRecord, ThreadItem, ThreadItemState, Turn, TurnOutcome, TurnState,
};

use crate::PureError;
use crate::studio::entity::{item, thread, thread_input, thread_submission, turn};

use super::input_metadata::deserialize_input_metadata;
use super::labels::{
    agent_state_kind, item_kind_label, presentation_from_label, thread_mode_from_label,
    thread_status,
};
use super::{store_error, u64_from_i64};

impl TryFrom<thread_input::Model> for DurableMailboxEnvelope {
    type Error = PureError;

    fn try_from(model: thread_input::Model) -> Result<Self, Self::Error> {
        let delivery_state: MailboxDeliveryState = serde_json::from_str(&model.state_json)?;
        if mailbox_state_kind(&delivery_state) != model.state_kind {
            return Err(store_error(format!(
                "mailbox state discriminator mismatch: JSON is {}, generated column is {}",
                mailbox_state_kind(&delivery_state),
                model.state_kind
            )));
        }
        let (metadata, queue_coalescing_key, budget_action) =
            deserialize_input_metadata(&model.metadata_json)?;
        Ok(Self {
            mail_id: model.mail_id,
            turn_id: TurnId::new(model.turn_id)?,
            thread_id: ThreadId::new(model.thread_id)?,
            payload: MailboxInputPayload {
                message: model.content,
                attachments: serde_json::from_str(&model.attachments_json)?,
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
        let state: TurnState = serde_json::from_str(&model.state_json)?;
        if turn_state_kind(&state) != model.state_kind {
            return Err(store_error(format!(
                "Turn state discriminator mismatch: JSON is {}, generated column is {}",
                turn_state_kind(&state),
                model.state_kind
            )));
        }
        let started_at = state.started_at();
        let finished_at = state
            .completed_at()
            .ok_or_else(|| store_error(format!("Turn {} is not terminal", model.id)))?;
        let outcome = match state {
            TurnState::Completed(state) => TurnOutcome::completed(state.completion()),
            TurnState::Cancelled(state) => TurnOutcome::cancelled(state.cause().clone()),
            TurnState::Failed(state) => TurnOutcome::failed(state.failure().clone()),
            TurnState::BudgetLimited(state) => {
                TurnOutcome::budget_limited(*state.limit(), state.rollover().clone())
            }
            TurnState::Queued(_) | TurnState::Running(_) => {
                return Err(store_error(format!("Turn {} is not terminal", model.id)));
            }
        };
        Ok(Self {
            turn_id: TurnId::new(model.id)?,
            thread_id: ThreadId::new(model.thread_id)?,
            outcome,
            usage: serde_json::from_str(&model.usage_json)?,
            started_at,
            finished_at,
        })
    }
}

impl TryFrom<thread::Model> for ThreadRecord {
    type Error = PureError;

    fn try_from(model: thread::Model) -> Result<Self, Self::Error> {
        let state: AgentState = serde_json::from_str(&model.state_json)?;
        if agent_state_kind(&state) != model.state_kind {
            return Err(store_error(format!(
                "Agent state discriminator mismatch: JSON is {}, generated column is {}",
                agent_state_kind(&state),
                model.state_kind
            )));
        }
        Ok(Self {
            id: model.id,
            project_id: model.project_id,
            title: model.title,
            mode: thread_mode_from_label(&model.mode)?,
            root_thread_id: model.root_thread_id,
            parent_thread_id: model.parent_thread_id,
            role: model.role,
            agent_path: model.agent_path,
            status: thread_status(&state),
            created_at: model.created_at,
            updated_at: model.updated_at,
            archived: model.archived != 0,
        })
    }
}

impl TryFrom<turn::Model> for Turn {
    type Error = PureError;

    fn try_from(model: turn::Model) -> Result<Self, Self::Error> {
        let state: TurnState = serde_json::from_str(&model.state_json)?;
        if turn_state_kind(&state) != model.state_kind {
            return Err(store_error(format!(
                "Turn state discriminator mismatch: JSON is {}, generated column is {}",
                turn_state_kind(&state),
                model.state_kind
            )));
        }
        Ok(Self {
            id: model.id,
            thread_id: model.thread_id,
            revision: u64::try_from(model.revision).map_err(store_error)?,
            state,
            updated_at: model.updated_at,
        })
    }
}

impl TryFrom<item::Model> for ThreadItem {
    type Error = PureError;

    fn try_from(model: item::Model) -> Result<Self, Self::Error> {
        let state: ThreadItemState = serde_json::from_str(&model.state_json)?;
        if item_kind_label(&state) != model.state_kind {
            return Err(store_error(format!(
                "Thread Item state discriminator mismatch: JSON is {}, generated column is {}",
                item_kind_label(&state),
                model.state_kind
            )));
        }
        Ok(ThreadItem::new(
            model.id,
            model.thread_id,
            model.turn_id,
            u64_from_i64(model.ordinal)?,
            u64_from_i64(model.revision)?,
            model.created_at,
            model.updated_at,
            state,
        ))
    }
}

fn mailbox_state_kind(state: &MailboxDeliveryState) -> &'static str {
    match state {
        MailboxDeliveryState::Pending(_) => "pending",
        MailboxDeliveryState::Claimed(_) => "claimed",
        MailboxDeliveryState::Consumed(_) => "consumed",
    }
}

fn turn_state_kind(state: &TurnState) -> &'static str {
    match state {
        TurnState::Queued(_) => "queued",
        TurnState::Running(_) => "running",
        TurnState::Completed(_) => "completed",
        TurnState::Cancelled(_) => "cancelled",
        TurnState::Failed(_) => "failed",
        TurnState::BudgetLimited(_) => "budgetLimited",
    }
}
