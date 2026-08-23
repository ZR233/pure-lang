//! Studio Task 会话恢复的 preview/apply 协议事实集。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Task conversation recovery target role.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskRecoveryTargetKind {
    Planner,
    Executor,
}

/// Terminal Turn available for conversation recovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioTaskRecoveryTurn {
    pub turn_id: String,
    pub state: StudioTaskRecoveryTurnState,
    pub updated_at: i64,
    pub item_count: u64,
    pub input_count: u64,
    pub tool_count: u64,
    pub tool_summaries: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskRecoveryTurnState {
    Completed,
    Cancelled,
    Failed,
    BudgetLimited,
}

/// One Planner or Executor recovery target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioTaskRecoveryTarget {
    pub thread_id: String,
    pub kind: StudioTaskRecoveryTargetKind,
    pub work_unit_id: Option<String>,
    pub attempt: Option<u32>,
    pub continuation_revision: Option<u64>,
    pub expected_runtime_revision: u64,
    pub expected_thread_revision: u64,
    pub branch: String,
    pub worktree_path: String,
    pub base_commit: Option<String>,
    pub turns: Vec<StudioTaskRecoveryTurn>,
    pub default_turn_ids: Vec<String>,
    pub available_modes: Vec<crate::ConversationRecoveryMode>,
}

/// Stateless recovery preview used as the apply CAS token and fact set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioTaskRecoveryPreview {
    pub preview_token: String,
    pub root_thread_id: String,
    pub run_id: String,
    pub revision: u64,
    pub task_generation: u64,
    pub state: StudioTaskRecoveryState,
    pub recommended_thread_id: String,
    pub targets: Vec<StudioTaskRecoveryTarget>,
    pub completion_revision_fingerprint: String,
    pub review_revision_fingerprint: String,
    pub merge_revision_fingerprint: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StudioTaskRecoveryState {
    Planning,
    PendingConfirmation,
    EditingDocuments,
    Working,
    Reviewing,
    Completed,
}

/// Applies a previously generated Task recovery preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioTaskRecoveryRequest {
    pub recovery_id: String,
    pub root_thread_id: String,
    pub target_thread_id: String,
    pub mode: crate::ConversationRecoveryMode,
    pub turn_ids: Vec<String>,
    pub preview: StudioTaskRecoveryPreview,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_turn_uses_typed_terminal_state_and_rejects_legacy_status() {
        let turn = StudioTaskRecoveryTurn {
            turn_id: "turn-1".to_string(),
            state: StudioTaskRecoveryTurnState::BudgetLimited,
            updated_at: 10,
            item_count: 2,
            input_count: 1,
            tool_count: 1,
            tool_summaries: vec!["shell".to_string()],
        };
        let json = serde_json::to_string(&turn).expect("serialize recovery Turn");
        let restored = serde_json::from_str(&json).expect("deserialize recovery Turn");
        assert_eq!(turn, restored);

        let legacy = serde_json::json!({
            "turnId": "turn-1",
            "status": "interrupted",
            "updatedAt": 10,
            "itemCount": 2,
            "inputCount": 1,
            "toolCount": 1,
            "toolSummaries": ["shell"]
        });
        assert!(serde_json::from_value::<StudioTaskRecoveryTurn>(legacy).is_err());
    }
}

/// Durable result of applying a Task conversation recovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRecoveryResult {
    pub recovery_id: String,
    pub run_id: String,
    pub work_unit_id: Option<String>,
    pub root_thread_id: String,
    pub target_thread_id: String,
    pub mode: crate::ConversationRecoveryMode,
    pub recovery_revision: u64,
    pub runtime_revision: u64,
    pub thread_revision: u64,
    pub before_transcript_hash: String,
    pub after_transcript_hash: String,
    pub removed_item_count: u64,
    pub removed_input_count: u64,
    pub resume_turn_id: String,
}
