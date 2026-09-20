//! Product timeline projections read core facts and never drive models, tools or persistence reducers.
mod compactions;
mod completions;
mod content;
pub(in crate::studio) mod engine;
pub(in crate::studio) mod panel;
mod runtime;
mod snapshot;
pub(in crate::studio) use snapshot::{project_snapshot, saved_mode, status};
mod order;
mod tool_media;
pub(crate) use tool_media::{delivery_attachments, read_persisted_media};
mod inputs;
mod messages;
mod responses;
mod tools;
mod turns;
pub(in crate::studio) use engine::ProjectionState;
pub(in crate::studio) use turns::project_turns;

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
    #[error("loaded projection fact {0} does not match its key")]
    FactIdentity(String),
    #[error("historical content metadata could not be encoded")]
    Encoding(#[from] serde_json::Error),
    #[error("duplicate or contradictory saved tool call {0}")]
    DuplicateCall(String),
    #[error("terminal tool call {0} has no saved result")]
    MissingToolResult(String),
    #[error("saved model receipt cannot be decoded")]
    Model(#[from] pl_core::model::ModelError),
    #[error("unsupported saved model content: {0}")]
    UnsupportedOutput(String),
    #[error("missing committed metadata for input {0}")]
    MissingInput(String),
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
/// This is the explicit reconstruction entry: it replays one canonical journal through the same
/// incremental slot builders the hot path uses, so there is exactly one projection algorithm.
pub(crate) fn project_items(
    thread_id: &str,
    parent_id: Option<&str>,
    snapshot: &pl_core::thread::ThreadSnapshot,
    journal: &[std::sync::Arc<pl_core::thread::journal::ThreadCommit>],
) -> Result<Vec<pl_protocol::ThreadItem>, ProjectionError> {
    Ok(ProjectionState::rebuild(thread_id, parent_id, snapshot, journal)?.materialize())
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
