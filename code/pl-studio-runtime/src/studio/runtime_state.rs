use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::studio::ids::unix_seconds;

/// Studio runtime 对 UI 暴露的生命周期状态。
///
/// 状态机只描述 runtime 服务自身是否可接受请求；单个 turn 的运行阶段由公共
/// Thread 事件表达。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioRuntimeStatus {
    Uninitialized,
    Initializing,
    Ready,
    ShuttingDown,
    Stopped,
    Failed,
}

impl StudioRuntimeStatus {
    fn can_transition_to(self, target: Self) -> bool {
        if self == target {
            return true;
        }
        match (self, target) {
            (Self::Uninitialized, Self::Initializing)
            | (Self::Uninitialized, Self::Stopped)
            | (Self::Initializing, Self::Ready)
            | (Self::Initializing, Self::Failed)
            | (Self::Ready, Self::ShuttingDown)
            | (Self::Ready, Self::Failed)
            | (Self::ShuttingDown, Self::Stopped)
            | (Self::ShuttingDown, Self::Failed)
            | (Self::Stopped, Self::Initializing)
            | (Self::Failed, Self::ShuttingDown)
            | (Self::Failed, Self::Initializing) => true,
            (Self::Uninitialized, _)
            | (Self::Initializing, _)
            | (Self::Ready, _)
            | (Self::ShuttingDown, _)
            | (Self::Stopped, _)
            | (Self::Failed, _) => false,
        }
    }
}

/// Studio runtime 当前活动 turn。
///
/// 每个 Thread 同一时间最多暴露一个活动 turn；更细的 phase 由 turn event 表达。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioActiveTurn {
    pub thread_id: String,
    pub turn_id: String,
}

/// 恢复问题影响的最小 UI 范围。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioRecoveryIssueScope {
    Application,
    Project,
    Thread,
}

/// 恢复问题的稳定类别。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioRecoveryIssueCategory {
    ProcessLease,
    AgentState,
    Worktree,
    Repository,
    Merge,
    Conflict,
}

/// UI 可执行的恢复动作。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioRecoveryIssueAction {
    Retry,
    CleanupThread,
    RemoveProject,
}

/// 单个项目或 Thread 的可隔离恢复问题。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRecoveryIssue {
    pub id: String,
    pub scope: StudioRecoveryIssueScope,
    pub category: StudioRecoveryIssueCategory,
    pub action: StudioRecoveryIssueAction,
    pub project_id: Option<String>,
    pub thread_id: Option<String>,
    pub task_run_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioRecoveryResourcePresence {
    Absent,
    Complete,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRecoveryCleanupResource {
    pub work_unit_id: String,
    pub path: String,
    pub branch: String,
    pub presence: StudioRecoveryResourcePresence,
    pub registration_exists: bool,
    pub path_exists: bool,
    pub branch_exists: bool,
    pub branch_head: Option<String>,
    pub dirty: bool,
    pub ahead_by: u32,
    pub changed_file_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRecoveryCleanupPreview {
    pub issue_id: String,
    pub expected_revision: String,
    pub scope: StudioRecoveryIssueScope,
    pub project_id: Option<String>,
    pub thread_id: Option<String>,
    pub message: String,
    pub resources: Vec<StudioRecoveryCleanupResource>,
}

/// Task conversation recovery 可选择的目标角色。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskRecoveryTargetKind {
    Planner,
    Executor,
}

/// Preview/Apply 之间必须保持完全一致的 Git/工作区指纹。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskGitFingerprint {
    pub workspace_root: String,
    pub git_common_dir: String,
    pub branch: String,
    pub head: String,
    pub base_commit: String,
    pub expected_head: String,
    pub operation: String,
    pub index_diff_hash: String,
    pub working_tree_diff_hash: String,
    pub untracked_content_hash: String,
}

/// 可选的完整 terminal Turn。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRecoveryTurn {
    pub turn_id: String,
    pub status: String,
    pub updated_at: i64,
    pub item_count: u64,
    pub input_count: u64,
    pub tool_count: u64,
    pub tool_summaries: Vec<String>,
}

/// 一个 Planner/Executor conversation recovery 候选。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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
    pub turns: Vec<StudioTaskRecoveryTurn>,
    pub default_turn_ids: Vec<String>,
    pub available_modes: Vec<pl_protocol::ConversationRecoveryMode>,
    pub git_fingerprint: StudioTaskGitFingerprint,
}

/// 无服务端临时状态的 Task recovery CAS preview。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRecoveryPreview {
    pub preview_token: String,
    pub root_thread_id: String,
    pub run_id: String,
    pub task_generation: u64,
    pub phase: String,
    pub expected_head: String,
    pub stop_requested: bool,
    pub branch_lease_id: String,
    pub branch_lease_branch: String,
    pub branch_lease_git_common_dir: String,
    pub branch_lease_expected_head: String,
    pub recommended_thread_id: String,
    pub targets: Vec<StudioTaskRecoveryTarget>,
    pub main_git_fingerprint: StudioTaskGitFingerprint,
    pub completion_revision_fingerprint: String,
    pub review_revision_fingerprint: String,
    pub merge_revision_fingerprint: String,
}

/// 用户确认 Task conversation recovery 的请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRecoveryRequest {
    pub recovery_id: String,
    pub root_thread_id: String,
    pub target_thread_id: String,
    pub mode: pl_protocol::ConversationRecoveryMode,
    pub turn_ids: Vec<String>,
    pub preview: StudioTaskRecoveryPreview,
}

/// conversation recovery、Stop 撤销与 resume mail 的 saga 结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRecoveryResult {
    pub recovery_id: String,
    pub run_id: String,
    pub work_unit_id: Option<String>,
    pub root_thread_id: String,
    pub target_thread_id: String,
    pub mode: pl_protocol::ConversationRecoveryMode,
    pub recovery_revision: u64,
    pub runtime_revision: u64,
    pub thread_revision: u64,
    pub before_transcript_hash: String,
    pub after_transcript_hash: String,
    pub removed_item_count: u64,
    pub removed_input_count: u64,
    pub stop_cleared: bool,
    pub resume_turn_id: String,
    pub git_fingerprint: StudioTaskGitFingerprint,
}

/// UI 可读取的 Studio runtime 快照。
///
/// 快照用于 Flutter/FRB 初始化、启动和关闭响应，也可用于状态栏展示 runtime
/// 服务级别错误。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRuntimeSnapshot {
    pub status: StudioRuntimeStatus,
    pub active_turns: Vec<StudioActiveTurn>,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recovery_issues: Vec<StudioRecoveryIssue>,
}

#[derive(Debug, Clone)]
pub struct StudioRuntimeState {
    inner: Arc<Mutex<StudioRuntimeStateInner>>,
}

#[derive(Debug)]
struct StudioRuntimeStateInner {
    status: StudioRuntimeStatus,
    active_turns: BTreeMap<String, String>,
    updated_at: i64,
    error: Option<String>,
    recovery_issues: Vec<StudioRecoveryIssue>,
}

impl StudioRuntimeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StudioRuntimeStateInner {
                status: StudioRuntimeStatus::Uninitialized,
                active_turns: BTreeMap::new(),
                updated_at: unix_seconds(),
                error: None,
                recovery_issues: Vec::new(),
            })),
        }
    }

    pub fn ready() -> Self {
        let state = Self::new();
        let _ = state.transition(StudioRuntimeStatus::Initializing, None);
        let _ = state.transition(StudioRuntimeStatus::Ready, None);
        state
    }

    pub fn snapshot(&self) -> StudioRuntimeSnapshot {
        let inner = self.inner.lock().expect("runtime state mutex poisoned");
        snapshot_from_inner(&inner)
    }

    pub fn transition(
        &self,
        target: StudioRuntimeStatus,
        error: Option<String>,
    ) -> Result<StudioRuntimeSnapshot> {
        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        if !inner.status.can_transition_to(target) {
            bail!(
                "invalid Studio runtime transition: {:?} -> {:?}",
                inner.status,
                target
            );
        }
        inner.status = target;
        inner.updated_at = unix_seconds();
        inner.error = error;
        if matches!(
            target,
            StudioRuntimeStatus::Uninitialized
                | StudioRuntimeStatus::Stopped
                | StudioRuntimeStatus::Failed
        ) {
            inner.active_turns.clear();
        }
        Ok(snapshot_from_inner(&inner))
    }

    pub fn mark_active_turn(&self, thread_id: String, turn_id: String) -> StudioRuntimeSnapshot {
        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        inner.active_turns.insert(thread_id, turn_id);
        inner.updated_at = unix_seconds();
        snapshot_from_inner(&inner)
    }

    pub(crate) fn replace_recovery_issues(
        &self,
        recovery_issues: Vec<StudioRecoveryIssue>,
    ) -> StudioRuntimeSnapshot {
        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        inner.recovery_issues = recovery_issues;
        inner.updated_at = unix_seconds();
        snapshot_from_inner(&inner)
    }

    pub(crate) fn recovery_issue(&self, issue_id: &str) -> Option<StudioRecoveryIssue> {
        self.inner
            .lock()
            .expect("runtime state mutex poisoned")
            .recovery_issues
            .iter()
            .find(|issue| issue.id == issue_id)
            .cloned()
    }

    pub(crate) fn remove_recovery_issue(&self, issue_id: &str) -> StudioRuntimeSnapshot {
        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        inner.recovery_issues.retain(|issue| issue.id != issue_id);
        inner.updated_at = unix_seconds();
        snapshot_from_inner(&inner)
    }

    pub(crate) fn remove_project_recovery_issues(&self, project_id: &str) -> StudioRuntimeSnapshot {
        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        inner
            .recovery_issues
            .retain(|issue| issue.project_id.as_deref() != Some(project_id));
        inner.updated_at = unix_seconds();
        snapshot_from_inner(&inner)
    }

    pub fn clear_active_turn(&self, thread_id: &str, turn_id: &str) -> StudioRuntimeSnapshot {
        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        if inner
            .active_turns
            .get(thread_id)
            .is_some_and(|active_turn_id| active_turn_id == turn_id)
        {
            inner.active_turns.remove(thread_id);
        }
        inner.updated_at = unix_seconds();
        snapshot_from_inner(&inner)
    }
}

impl Default for StudioRuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

fn snapshot_from_inner(inner: &StudioRuntimeStateInner) -> StudioRuntimeSnapshot {
    StudioRuntimeSnapshot {
        status: inner.status,
        active_turns: inner
            .active_turns
            .iter()
            .map(|(thread_id, turn_id)| StudioActiveTurn {
                thread_id: thread_id.clone(),
                turn_id: turn_id.clone(),
            })
            .collect(),
        updated_at: inner.updated_at,
        error: inner.error.clone(),
        recovery_issues: inner.recovery_issues.clone(),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{StudioRuntimeState, StudioRuntimeStatus};

    #[test]
    fn runtime_state_follows_lifecycle_transitions() {
        let state = StudioRuntimeState::new();

        assert_eq!(state.snapshot().status, StudioRuntimeStatus::Uninitialized);
        let initializing = state
            .transition(StudioRuntimeStatus::Initializing, None)
            .unwrap();
        assert_eq!(initializing.status, StudioRuntimeStatus::Initializing);
        let ready = state.transition(StudioRuntimeStatus::Ready, None).unwrap();
        assert_eq!(ready.status, StudioRuntimeStatus::Ready);
        let shutting_down = state
            .transition(StudioRuntimeStatus::ShuttingDown, None)
            .unwrap();
        assert_eq!(shutting_down.status, StudioRuntimeStatus::ShuttingDown);
        let stopped = state
            .transition(StudioRuntimeStatus::Stopped, None)
            .unwrap();
        assert_eq!(stopped.status, StudioRuntimeStatus::Stopped);
    }

    #[test]
    fn runtime_state_rejects_invalid_transition_and_clears_active_turns() {
        let state = StudioRuntimeState::ready();
        let snapshot = state.mark_active_turn("session-a".to_string(), "turn-a".to_string());

        assert_eq!(snapshot.active_turns.len(), 1);
        assert!(
            state
                .transition(StudioRuntimeStatus::Stopped, None)
                .is_err()
        );
        state
            .transition(StudioRuntimeStatus::ShuttingDown, None)
            .unwrap();
        let stopped = state
            .transition(StudioRuntimeStatus::Stopped, None)
            .unwrap();

        assert_eq!(stopped.active_turns, Vec::new());
    }
}
