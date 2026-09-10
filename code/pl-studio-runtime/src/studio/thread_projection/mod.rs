//! Product timeline projections read core facts and never drive models, tools or persistence reducers.
mod compactions;
mod completions;
mod content;
mod runtime;
mod snapshot;
pub(in crate::studio) use snapshot::{project_snapshot, saved_mode, status};
mod order;
mod tools;
pub(in crate::studio) use tools::project_tools;
mod responses;
pub(in crate::studio) use responses::project_responses;
mod inputs;
pub(in crate::studio) use inputs::project_inputs;
mod turns;
pub(in crate::studio) use turns::project_turns;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProjectionError {
    #[error("saved product state cannot be projected")]
    Product(#[from] pl_protocol::PureError),
    #[error("saved workflow state cannot be projected")]
    Workflow(#[from] pl_core::tool::opaque::ToolError),
    #[error("projection journal is incomplete or mixes Thread identities")]
    JournalOrder,
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
    #[error("missing committed metadata for Turn {0}")]
    MissingTurn(String),
    #[error("timeline count exceeds the product representation")]
    Count,
    #[error("Turn {0} has no measured final duration")]
    MissingDuration(String),
}

/// Combines product projections with one order derived from their original admission facts.
pub(crate) fn project_items(
    thread_id: &str,
    snapshot: &pl_core::thread::ThreadSnapshot,
    journal: &[std::sync::Arc<pl_core::thread::journal::ThreadCommit>],
) -> Result<Vec<pl_protocol::ThreadItem>, ProjectionError> {
    let positions = order::positions(journal, snapshot.commit_sequence)?;
    let mut items = project_inputs(thread_id, snapshot, journal)?;
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
