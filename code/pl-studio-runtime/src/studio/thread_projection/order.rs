//! One deterministic timeline order, derived from immutable fact admission rather than current rendering.
use super::ProjectionError;
use pl_core::thread::{AttemptOutcome, ThreadEffectBatch, input::InputChange};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub(super) struct Position {
    pub ordinal: u64,
    pub created_at: i64,
}

pub(super) fn compaction_id(id: &str) -> String {
    format!("compaction:{}:{id}", id.len())
}

pub(in crate::studio) fn turn_id(id: &str) -> String {
    format!("turn:{}:{id}", id.len())
}
pub(in crate::studio) fn message_id(id: &str) -> String {
    format!("message:{}:{id}", id.len())
}
pub(super) fn skill_id(id: &str) -> String {
    format!("skill:{}:{id}", id.len())
}

pub(super) fn completion_id(id: &str) -> String {
    format!("completion:{}:{id}", id.len())
}

pub(super) fn tool_id(id: &str) -> String {
    format!("tool:{}:{id}", id.len())
}

/// Durable receipt identity of one terminal interaction/permission record.
///
/// The writer records the receipt in the same transaction as the effect that produced it and the
/// host reads it back by this identity, so both sides derive it from the same function instead of
/// duplicating a format.
pub(in crate::studio) fn receipt_id(kind: &str, id: &str) -> String {
    format!("{kind}:{}:{id}", id.len())
}

pub(super) fn response_id(id: &str, kind: &str) -> String {
    format!("model:{}:{id}:{kind}", id.len())
}

/// Reserved slots include hidden/empty projections so later schema interpretation cannot shift history.
pub(super) fn positions(
    journal: &[Arc<ThreadEffectBatch>],
    through: u64,
) -> Result<BTreeMap<String, Position>, ProjectionError> {
    let mut positions = BTreeMap::new();
    let mut ordinal = 0_u64;
    let mut next_sequence = 1;
    let mut owner_id = None;
    for commit in journal.iter().filter(|commit| commit.sequence <= through) {
        if commit.sequence != next_sequence || owner_id.is_some_and(|id| id != commit.thread_id) {
            return Err(ProjectionError::JournalOrder);
        }
        owner_id = Some(commit.thread_id.as_str());
        next_sequence = next_sequence.checked_add(1).ok_or(ProjectionError::Count)?;
        let mut insert = |id: String| -> Result<(), ProjectionError> {
            if let std::collections::btree_map::Entry::Vacant(entry) = positions.entry(id) {
                ordinal = ordinal.checked_add(1).ok_or(ProjectionError::Count)?;
                entry.insert(Position {
                    ordinal,
                    created_at: commit.committed_at,
                });
            }
            Ok(())
        };
        for input in commit.inputs.iter() {
            if let InputChange::Accepted(record) = input {
                insert(record.input.id.clone())?;
            }
        }
        for record in commit.inbox.iter() {
            insert(message_id(&record.message.id))?;
        }
        if let Some(turn) = &commit.turn {
            insert(turn_id(&turn.turn_id))?;
        }
        for change in commit.extensions.iter() {
            if let pl_core::thread::extensions::ExtensionChange::Put { id, record } = change
                && record.payload.format() == "pl.studio.compaction"
            {
                insert(compaction_id(id))?;
            }
        }
        for delivery in commit.deliveries.iter() {
            insert(skill_id(&delivery.call_id))?;
            insert(completion_id(&delivery.call_id))?;
        }
        if let Some(attempt) = &commit.attempt {
            insert(response_id(&attempt.attempt_id, "inference"))?;
            insert(response_id(&attempt.attempt_id, "reasoning"))?;
            insert(response_id(&attempt.attempt_id, "text"))?;
            if let AttemptOutcome::Committed(output) = &attempt.outcome {
                for call in &output.tool_calls {
                    insert(tool_id(&call.call_id))?;
                }
            }
        }
    }
    if through != next_sequence - 1 {
        return Err(ProjectionError::JournalOrder);
    }
    Ok(positions)
}
