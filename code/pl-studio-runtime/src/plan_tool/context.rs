//! Current Plan facts projected for the model by Studio.
use super::PLAN_CONTEXT_SECTION_ID;
use crate::plan_tool::state::AgentSessionPlanMachine;
use pl_core::context::content_hash as canonical_content_hash;
use pl_protocol::{
    AgentSessionPlanState, ContextSectionId, ModelContextSectionSnapshot, PureError,
};

/// 从当前 Plan 状态派生完整快照；相同版本由会话事实层去重。
///
/// # Errors
/// Rejects invalid Plan state or a failed context encoding.
pub fn plan_model_context_section(
    state: &AgentSessionPlanState,
) -> Result<ModelContextSectionSnapshot, PureError> {
    let machine = AgentSessionPlanMachine::new(state.clone())
        .map_err(|error| PureError::ConfigError(error.to_string()))?;
    let snapshot = machine.snapshot();
    let document = snapshot.document.as_ref().map(|document| {
        serde_json::json!({
            "version": document.version,
            "contentHash": document.content_hash,
            "markdown": document.markdown,
        })
    });
    let content = serde_json::to_string_pretty(&serde_json::json!({
        "revision": snapshot.revision,
        "state": snapshot.state,
        "document": document,
        "pendingInteractionId": snapshot.pending_interaction_id,
        "lastRevisionFeedback": snapshot.last_revision_feedback,
        "allowedTransitions": snapshot.allowed_transitions,
        "constraint": "Latest host state overrides outdated summaries. Approval survives compaction and restart. Continue implementing within the approved scope; ask only when the goal, scope or a key decision changes. Use plan_current only when this projection is missing or a revision conflict occurs.",
    }))?;
    Ok(ModelContextSectionSnapshot {
        id: ContextSectionId::new(PLAN_CONTEXT_SECTION_ID)
            .map_err(|error| PureError::ConfigError(error.to_string()))?,
        title: "AgentSession Plan State".to_string(),
        content_hash: canonical_content_hash(content.as_bytes()),
        content,
    })
}
