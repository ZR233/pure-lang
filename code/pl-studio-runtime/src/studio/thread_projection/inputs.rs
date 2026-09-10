//! Accepted input items retain stable product identity across queueing, steering and retries.
use super::{ProjectionError, content::input_content};
use pl_core::thread::{ThreadSnapshot, input::InputState, journal::ThreadCommit};
use pl_protocol::{
    MessagePresentation, ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel,
    ThreadTextItem,
};
use std::{collections::BTreeMap, sync::Arc};

pub(in crate::studio) fn project_inputs(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    journal: &[Arc<ThreadCommit>],
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let commits = journal
        .iter()
        .filter(|commit| commit.sequence <= snapshot.commit_sequence)
        .map(|commit| (commit.sequence, commit.as_ref()))
        .collect::<BTreeMap<_, _>>();
    let mut items = Vec::new();
    for input in snapshot.inputs.iter() {
        let content = input_content(input);
        if content
            .as_ref()
            .is_ok_and(|content| content.presentation == MessagePresentation::Hidden)
        {
            continue;
        }
        let accepted = commits
            .get(&input.accepted_sequence)
            .ok_or_else(|| ProjectionError::MissingInput(input.input.id.clone()))?;
        let turn_id = match &input.state {
            InputState::Consumed { turn_id, .. } => turn_id.clone(),
            InputState::Pending | InputState::Discarded => snapshot
                .turns
                .iter()
                .rev()
                .find(|turn| turn.input_id.as_deref() == Some(input.input.id.as_str()))
                .map(|turn| turn.turn_id.clone())
                .unwrap_or_default(),
        };
        let updated = journal
            .iter()
            .rev()
            .filter(|commit| commit.sequence <= snapshot.commit_sequence)
            .find(|commit| {
                commit.inputs.iter().any(|change| match change {
                    pl_core::thread::input::InputChange::Accepted(record) => {
                        record.input.id == input.input.id
                    }
                    pl_core::thread::input::InputChange::Transition { id, .. } => {
                        id == &input.input.id
                    }
                }) || commit
                    .turn
                    .as_ref()
                    .is_some_and(|turn| turn.input_id.as_deref() == Some(input.input.id.as_str()))
            })
            .ok_or_else(|| ProjectionError::MissingInput(input.input.id.clone()))?;
        let lifecycle = match input.state {
            InputState::Pending | InputState::Consumed { .. } => {
                ThreadContentLifecycle::completed(accepted.committed_at)
            }
            InputState::Discarded => ThreadContentLifecycle::cancelled(
                updated.committed_at,
                "Input discarded before model admission.".into(),
            ),
        };
        let state = match content {
            Ok(content) => ThreadItemState::Text(ThreadTextItem::new(
                ThreadTextChannel::User,
                content.text,
                content.attachments,
                lifecycle,
            )),
            Err(error) => ThreadItemState::Raw(pl_protocol::ThreadRawItem {
                payloads: vec![super::raw_payload(&input.input.payload)],
                notice: error.to_string(),
                recorded_at: updated.committed_at,
            }),
        };
        items.push(ThreadItem::new(
            input.input.id.clone(),
            thread_id.into(),
            turn_id,
            input.ordinal,
            updated.sequence,
            accepted.committed_at,
            updated.committed_at,
            state,
        ));
    }
    Ok(items)
}
