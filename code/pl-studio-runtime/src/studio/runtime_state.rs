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

pub use pl_protocol::studio::{
    StudioTaskGitFingerprint, StudioTaskRecoveryPreview, StudioTaskRecoveryRequest,
    StudioTaskRecoveryResult, StudioTaskRecoveryTarget, StudioTaskRecoveryTargetKind,
    StudioTaskRecoveryTurn,
};
/// UI 可读取的 Studio runtime 快照。
///
/// 快照用于 Flutter/FRB 初始化、启动和关闭响应，也可用于状态栏展示 runtime
/// 服务级别错误。
///
/// 恢复问题不在此快照中：它们由 [`crate::studio::StudioRecoveryRegistry`] 独立
/// 持有，避免与频繁的生命周期转换竞争同一把锁。活动 turn 列表也不再持久存储：
/// [`StudioRuntime`] 在需要时从 agent framework 派生，避免与 turn 事件双写。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRuntimeSnapshot {
    pub status: StudioRuntimeStatus,
    pub active_turns: Vec<StudioActiveTurn>,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl StudioRuntimeSnapshot {
    /// 构造一个不含活动 turn 的最小快照。
    pub(super) fn from_status(
        status: StudioRuntimeStatus,
        updated_at: i64,
        error: Option<String>,
    ) -> Self {
        Self {
            status,
            active_turns: Vec::new(),
            updated_at,
            error,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StudioRuntimeState {
    inner: Arc<Mutex<StudioRuntimeStateInner>>,
}

#[derive(Debug)]
struct StudioRuntimeStateInner {
    status: StudioRuntimeStatus,
    updated_at: i64,
    error: Option<String>,
}

impl StudioRuntimeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StudioRuntimeStateInner {
                status: StudioRuntimeStatus::Uninitialized,
                updated_at: unix_seconds(),
                error: None,
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
        StudioRuntimeSnapshot::from_status(inner.status, inner.updated_at, inner.error.clone())
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
        Ok(StudioRuntimeSnapshot::from_status(
            inner.status,
            inner.updated_at,
            inner.error.clone(),
        ))
    }
}

impl Default for StudioRuntimeState {
    fn default() -> Self {
        Self::new()
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
    fn runtime_state_rejects_invalid_transition() {
        let state = StudioRuntimeState::ready();

        // Ready -> Stopped is not a legal direct transition.
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
        assert_eq!(stopped.status, StudioRuntimeStatus::Stopped);
    }
}
