//! Product timeline projections read core facts and never drive models, tools or persistence reducers.
mod compactions;
mod completions;
mod content;
pub(in crate::studio) use content::{
    prompt_request_digest, referenced_attachment_ids, saved_prompt_request_digest,
};
mod effect;
pub(in crate::studio) use effect::project_effect_items;
mod live;
pub(in crate::studio) use live::{LiveEvent, LiveProjection, TurnEvent};
mod runtime;
pub(in crate::studio) use runtime::fold_effect_accounting;
mod snapshot;
pub(in crate::studio) use snapshot::{project_history_items, project_snapshot, saved_mode, status};
pub(in crate::studio) mod order;
mod tool_media;
pub(crate) use tool_media::{delivery_attachments, read_persisted_media};
mod tools;
pub(in crate::studio) use tools::project_tools;
mod responses;
pub(in crate::studio) use responses::project_responses;
mod inputs;
mod messages;
pub(in crate::studio) use inputs::project_inputs;
mod turns;
pub(in crate::studio) use turns::{project_active_turn, project_turns};

/// Identity of one streaming channel item (`reasoning` or `text`) for an attempt.
///
/// Both the durable writer and the live projection derive the finalize set from the same identity,
/// so a failure never finalizes a channel the attempt never started.
pub(in crate::studio) fn attempt_channel_id(attempt_id: &str, channel: &str) -> String {
    order::response_id(attempt_id, channel)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProjectionError {
    #[error("saved product state cannot be projected")]
    Product(#[from] pl_protocol::PureError),
    #[error("saved workflow state cannot be projected")]
    Workflow(#[from] pl_core::tool::opaque::ToolError),
    #[error("saved tool media receipt cannot be decoded")]
    ToolMedia(pl_core::tool::opaque::ToolError),
    #[error("projection journal is incomplete or mixes Thread identities")]
    JournalOrder,
    #[error("durable history identity lookup failed: {0}")]
    History(String),
    #[error("timeline item {0} has no unique admission position")]
    ItemOrder(String),
    #[error("historical content metadata could not be encoded")]
    Encoding(#[from] serde_json::Error),
    #[error("duplicate or contradictory saved tool call {0}")]
    DuplicateCall(String),
    #[error("terminal tool call {0} has no saved result")]
    MissingToolResult(String),
    #[error("saved model receipt cannot be decoded")]
    Model(#[from] pl_core::model::ModelError),
    #[error("missing committed metadata for model attempt {0}")]
    MissingAttempt(String),
    #[error("unsupported saved model content: {0}")]
    UnsupportedOutput(String),
    #[error("missing committed metadata for input {0}")]
    MissingInput(String),
    #[error("durable history is missing the committed input item(s) {0}")]
    MissingDurableInput(String),
    #[error("missing committed consumption for parent message {0}")]
    MissingMessage(String),
    #[error("missing committed metadata for Turn {0}")]
    MissingTurn(String),
    #[error("timeline count exceeds the product representation")]
    Count,
    #[error("Turn {0} has no measured final duration")]
    MissingDuration(String),
}

/// Combines product projections with one order derived from their original admission facts.
///
/// This is the one-way legacy-history projection: it consumes one committed effect window
/// (`pl_core::thread::ThreadEffectBatch`, the bounded batch the owner publishes per commit) together
/// with the durable session state a restart or the migration export reconstructed. The only caller
/// that supplies a complete window is the one-way migration export; live and durable product reads
/// use `project_effect_items` plus the durable history database instead, so no normal path depends on
/// replaying a complete journal.
pub(crate) fn project_items(
    thread_id: &str,
    parent_id: Option<&str>,
    snapshot: &pl_core::thread::ThreadSnapshot,
    journal: &[std::sync::Arc<pl_core::thread::ThreadEffectBatch>],
) -> Result<Vec<pl_protocol::ThreadItem>, ProjectionError> {
    let positions = order::positions(journal, snapshot.commit_sequence)?;
    let mut items = project_inputs(thread_id, snapshot, journal)?;
    items.extend(messages::project_messages(
        thread_id,
        parent_id,
        journal,
        snapshot.commit_sequence,
    )?);
    items.extend(project_responses(thread_id, snapshot, journal)?);
    items.extend(project_tools(thread_id, snapshot, journal)?);
    items.extend(completions::project_completions(
        thread_id, snapshot, journal,
    ));
    items.extend(compactions::project_compactions(
        thread_id, snapshot, journal,
    )?);
    for turn in project_turns(thread_id, snapshot, journal)? {
        items.push(pl_protocol::ThreadItem::new(
            order::turn_id(&turn.id),
            thread_id.into(),
            turn.id.clone(),
            0,
            turn.revision,
            turn.state.started_at().unwrap_or(turn.updated_at),
            turn.updated_at,
            pl_protocol::ThreadItemState::Turn(
                pl_protocol::ThreadTurnItem::new(turn.state).with_input_id(turn.input_id),
            ),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for item in &mut items {
        let position = positions
            .get(&item.id)
            .ok_or_else(|| ProjectionError::ItemOrder(item.id.clone()))?;
        if !seen.insert(item.id.clone()) {
            return Err(ProjectionError::ItemOrder(item.id.clone()));
        }
        item.ordinal = position.ordinal;
        item.created_at = position.created_at;
    }
    items.sort_by_key(|item| item.ordinal);
    Ok(items)
}

pub(super) fn raw_payload(
    payload: &pl_core::context::OpaquePayload,
) -> pl_protocol::ThreadRawPayload {
    pl_protocol::ThreadRawPayload {
        format: payload.format().into(),
        version: payload.version(),
        content: payload.content().into(),
    }
}
