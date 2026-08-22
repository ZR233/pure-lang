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
    pub status: String,
    pub updated_at: i64,
    pub item_count: u64,
    pub input_count: u64,
    pub tool_count: u64,
    pub tool_summaries: Vec<String>,
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
    pub stop_requested: bool,
    pub project_lease_id: String,
    pub recommended_thread_id: String,
    pub targets: Vec<StudioTaskRecoveryTarget>,
    pub completion_revision_fingerprint: String,
    pub review_revision_fingerprint: String,
    pub merge_revision_fingerprint: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StudioTaskRecoveryState {
    DesignUpdating,
    Implementing,
    Merging,
    Reviewing,
    Reworking,
    Stopping,
    Blocked,
    Completed,
    Failed,
    Cancelled,
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
    pub stop_cleared: bool,
    pub resume_turn_id: String,
}
