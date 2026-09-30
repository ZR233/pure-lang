//! Product timeline projections read core facts and never drive models, tools or persistence reducers.
mod billing;
pub(crate) use billing::AuxiliaryBillingReceipt;
pub(in crate::studio) use billing::billing_facts;
mod activity;
pub(in crate::studio) use activity::{ActivityDetailRead, ActivityProjection, project_activity};
mod compactions;
mod completions;
mod content;
pub(in crate::studio) use content::{
    input_presentation, prompt_request_digest, referenced_attachment_ids,
    saved_prompt_request_digest,
};
mod effect;
pub(in crate::studio) use effect::project_effect_items;
mod live;
pub(in crate::studio) use live::{
    LiveEvent, LiveProjection, PreparedEffect, TurnEvent, TurnResult, retained_bytes,
    seed_repaired_targets,
};
// The retained-window bound is only read outside its own module by the storage fault test that has
// to commit more parts than the window holds, so it stays a test-only re-export.
#[cfg(test)]
pub(in crate::studio) use live::LIVE_ITEM_WINDOW;
mod runtime;
pub(in crate::studio) use runtime::{fold_effect_accounting, project_runtime};
mod snapshot;
pub(in crate::studio) use snapshot::{project_snapshot, saved_mode, status, storage_state};
pub(in crate::studio) mod order;
mod tool_media;
pub(crate) use tool_media::{delivery_attachments, read_persisted_media};
mod inputs;
mod responses;
mod tools;
mod turns;
pub(in crate::studio) use turns::project_active_turn;

/// The canonical terminal Turn already prepared for the reliable history transaction.
pub(in crate::studio) fn prepared_terminal_turn(
    prepared: &PreparedEffect,
) -> Option<pl_protocol::Turn> {
    prepared.items.iter().find_map(|item| {
        let pl_protocol::ThreadItemState::Turn(turn) = item.state() else {
            return None;
        };
        if !turn.state().is_terminal() {
            return None;
        }
        Some(pl_protocol::Turn {
            id: item.turn_id.clone(),
            thread_id: item.thread_id.clone(),
            input_id: turn.input_id().map(str::to_owned),
            state: turn.state().clone(),
            revision: prepared.sequence,
            updated_at: item.updated_at,
        })
    })
}

/// Identity of one streaming channel item (`reasoning` or `text`) for an attempt.
///
/// Both the durable writer and the live projection derive the finalize set from the same identity,
/// so a failure never finalizes a channel the attempt never started.
pub(in crate::studio) fn attempt_channel_id(attempt_id: &str, channel: &str) -> String {
    order::response_id(attempt_id, channel)
}

pub(in crate::studio) fn presentation_preview_prefix(attempt_id: &str) -> String {
    order::presentation_prefix(attempt_id)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProjectionError {
    #[error("saved product state cannot be projected")]
    Product(#[from] pl_protocol::PureError),
    #[error("saved workflow state cannot be projected")]
    Workflow(#[from] pl_core::tool::opaque::ToolError),
    #[error("saved tool media receipt cannot be decoded")]
    ToolMedia(pl_core::tool::opaque::ToolError),
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
    #[error("unsupported saved model content: {0}")]
    UnsupportedOutput(String),
    #[error("missing committed metadata for input {0}")]
    MissingInput(String),
    #[error("durable history is missing the committed input item(s) {0}")]
    MissingDurableInput(String),
    #[error("hidden input {0} also has a visible durable timeline item")]
    ContradictoryHiddenInput(String),
    #[error("durable history is missing the accepted tool call item(s) {0}")]
    MissingDurableToolCall(String),
    #[error("timeline count exceeds the product representation")]
    Count,
    #[error("Turn {0} has no measured final duration")]
    MissingDuration(String),
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
