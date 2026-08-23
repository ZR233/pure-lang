//! fingerprint、恢复状态与 preview token 的纯投影 helper。

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use pl_protocol::{
    ConversationRecoveryMode, ThreadToolFailureKind, ThreadToolItem, ThreadToolState,
};

use crate::studio::task_coordinator::TaskRunStateKind;
use crate::studio::{StudioTaskRecoveryPreview, StudioTaskRecoveryState};

pub(super) const fn recovery_state_from_task_kind(
    kind: TaskRunStateKind,
) -> StudioTaskRecoveryState {
    match kind {
        TaskRunStateKind::Planning => StudioTaskRecoveryState::Planning,
        TaskRunStateKind::PendingConfirmation => StudioTaskRecoveryState::PendingConfirmation,
        TaskRunStateKind::EditingDocuments => StudioTaskRecoveryState::EditingDocuments,
        TaskRunStateKind::Working => StudioTaskRecoveryState::Working,
        TaskRunStateKind::Reviewing => StudioTaskRecoveryState::Reviewing,
        TaskRunStateKind::Completed => StudioTaskRecoveryState::Completed,
    }
}
pub(super) fn tool_summary(tool: &ThreadToolItem) -> String {
    let outcome = match tool.state() {
        ThreadToolState::Started(_) => Some("started".to_string()),
        ThreadToolState::Streaming(_) => Some("streaming".to_string()),
        ThreadToolState::AwaitingApproval(_) => Some("awaiting approval".to_string()),
        ThreadToolState::Approved(_) => Some("approved".to_string()),
        ThreadToolState::Running(_) => Some("running".to_string()),
        ThreadToolState::Succeeded(state) => state
            .output()
            .exit_code()
            .map(|code| format!("exit {code}")),
        ThreadToolState::Failed(state) => Some(match state.failure().kind() {
            ThreadToolFailureKind::Execution => "failed".to_string(),
            ThreadToolFailureKind::TimedOut => "timed out".to_string(),
            ThreadToolFailureKind::BudgetLimited => "budget limited".to_string(),
        }),
        ThreadToolState::Denied(_) => Some("denied".to_string()),
        ThreadToolState::Cancelled(_) => Some("cancelled".to_string()),
    };
    match outcome {
        Some(outcome) => format!("{} ({outcome})", tool.invocation().name()),
        None => tool.invocation().name().to_string(),
    }
}
pub(super) async fn selected_input_hashes(
    store: &crate::studio::StudioStore,
    thread_id: &str,
    turn_ids: &[String],
    mode: ConversationRecoveryMode,
) -> Result<Vec<String>> {
    let inputs = store.conversation_turn_inputs(thread_id, turn_ids).await?;
    flatten_input_hashes(
        &inputs,
        turn_ids,
        matches!(mode, ConversationRecoveryMode::RewindTail),
    )
}

pub(super) fn flatten_input_hashes(
    inputs: &BTreeMap<String, crate::studio::store::conversation_recovery::ConversationTurnInputs>,
    turn_ids: &[String],
    require_every_turn: bool,
) -> Result<Vec<String>> {
    let mut hashes = Vec::new();
    for turn_id in turn_ids {
        let turn_inputs = inputs.get(turn_id);
        if require_every_turn && turn_inputs.is_none_or(|inputs| inputs.hashes.is_empty()) {
            bail!("Selected Turn has no precisely matched consumed mailbox input");
        }
        if let Some(turn_inputs) = turn_inputs {
            hashes.extend(turn_inputs.hashes.clone());
        }
    }
    Ok(hashes)
}
pub(super) fn record_fingerprint(value: &impl serde::Serialize) -> Result<String> {
    Ok(pl_core::canonical_json_hash(&serde_json::to_value(value)?))
}
pub(super) fn preview_token(preview: &StudioTaskRecoveryPreview) -> Result<String> {
    let mut value = serde_json::to_value(preview)?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "previewToken".to_string(),
            serde_json::Value::String(String::new()),
        );
    }
    Ok(pl_core::canonical_json_hash(&value))
}
