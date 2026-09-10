//! One deterministic timeline order, derived from immutable fact admission rather than current rendering.
use super::ProjectionError;
use pl_core::thread::{AttemptOutcome, input::InputChange, journal::ThreadCommit};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub(super) struct Position {
    pub ordinal: u64,
    pub created_at: i64,
}

pub(super) fn compaction_id(id: &str) -> String {
    format!("compaction:{}:{id}", id.len())
}

pub(super) fn turn_id(id: &str) -> String {
    format!("turn:{}:{id}", id.len())
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
pub(super) fn response_id(id: &str, kind: &str) -> String {
    format!("model:{}:{id}:{kind}", id.len())
}

/// Reserved slots include hidden/empty projections so later schema interpretation cannot shift history.
pub(super) fn positions(
    journal: &[Arc<ThreadCommit>],
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

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::OpaquePayload,
        thread::input::{InputDelivery, InputRecord, InputState, ThreadInput},
    };
    use pretty_assertions::assert_eq;

    fn commit(sequence: u64) -> ThreadCommit {
        ThreadCommit {
            committed_at: sequence as i64,
            thread_id: "thread".into(),
            sequence,
            permissions: Vec::new().into(),
            wake_messages_through: None,
            inputs: Vec::new().into(),
            tasks: Vec::new().into(),
            context: None,
            private_context: None,
            attempt: None,
            turn: None,
            discovered_tools: None,
            deliveries: Vec::new().into(),
            extensions: Vec::new().into(),
            inbox: Vec::new().into(),
            consumed_messages: None,
            interactions: Vec::new().into(),
            replacements: Vec::new().into(),
            runtime_facts: None,
            lifecycle: None,
        }
    }

    #[test]
    fn appended_inputs_never_change_existing_positions_and_hidden_content_still_reserves_its_slot()
    {
        let input = |id: &str, ordinal, accepted_sequence| {
            InputChange::Accepted(InputRecord {
                accepted_sequence,
                delivery: InputDelivery::NextTurn,
                ordinal,
                revision: 1,
                state: InputState::Pending,
                input: ThreadInput {
                    id: id.into(),
                    payload: OpaquePayload::text("opaque metadata"),
                    context: Vec::new(),
                },
            })
        };
        let mut first = commit(1);
        first.inputs = vec![input("hidden", 1, 1), input("visible", 2, 1)].into();
        let first = Arc::new(first);
        let previous = positions(std::slice::from_ref(&first), 1).unwrap();
        let mut second = commit(2);
        second.inputs = vec![input("later", 3, 2)].into();
        let current = positions(&[first, Arc::new(second)], 2).unwrap();
        assert_eq!(current["visible"].ordinal, previous["visible"].ordinal);
        assert_eq!(current["visible"].ordinal, 2);
        assert_eq!(current["later"].ordinal, 3);
        assert_eq!(current["visible"].created_at, 1);
    }

    #[test]
    fn truncated_or_cross_thread_journals_are_rejected() {
        assert!(positions(&[], 1).is_err());
        assert!(positions(&[Arc::new(commit(2))], 2).is_err());
        let first = Arc::new(commit(1));
        let mut second = commit(2);
        second.thread_id = "other".into();
        assert!(positions(&[first, Arc::new(second)], 2).is_err());
    }
}
