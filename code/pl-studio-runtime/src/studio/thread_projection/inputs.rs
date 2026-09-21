//! Accepted input items retain stable product identity across queueing, steering and retries.
use super::{ProjectionError, content::input_content};
use pl_core::thread::{ThreadEffectBatch, ThreadSnapshot, input::InputState};
use pl_protocol::{
    MessagePresentation, ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel,
    ThreadTextItem,
};
use std::{collections::BTreeMap, sync::Arc};

pub(in crate::studio) fn project_inputs(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    journal: &[Arc<ThreadEffectBatch>],
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let commits = journal
        .iter()
        .filter(|commit| commit.sequence <= snapshot.commit_sequence)
        .map(|commit| (commit.sequence, commit.as_ref()))
        .collect::<BTreeMap<_, _>>();
    let mut items = Vec::new();
    for input in snapshot.inputs.iter() {
        let accepted = commits
            .get(&input.accepted_sequence)
            .ok_or_else(|| ProjectionError::MissingInput(input.input.id.clone()))?;
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
        if let Some(item) = project_input(
            thread_id,
            snapshot,
            input,
            input.ordinal,
            accepted.committed_at,
            updated.sequence,
            updated.committed_at,
        )? {
            items.push(item);
        }
    }
    Ok(items)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn project_input(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    input: &pl_core::thread::input::InputRecord,
    ordinal: u64,
    created_at: i64,
    revision: u64,
    updated_at: i64,
) -> Result<Option<ThreadItem>, ProjectionError> {
    let content = input_content(input);
    if content
        .as_ref()
        .is_ok_and(|content| content.presentation == MessagePresentation::Hidden)
    {
        return Ok(None);
    }
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
    let lifecycle = match input.state {
        InputState::Pending | InputState::Consumed { .. } => {
            ThreadContentLifecycle::completed(created_at)
        }
        InputState::Discarded => ThreadContentLifecycle::cancelled(
            updated_at,
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
            recorded_at: updated_at,
        }),
    };
    Ok(Some(ThreadItem::new(
        input.input.id.clone(),
        thread_id.into(),
        turn_id,
        ordinal,
        revision,
        created_at,
        updated_at,
        state,
    )))
}
