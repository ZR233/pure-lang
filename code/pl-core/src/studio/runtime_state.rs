use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::studio::ids::unix_seconds;

/// Studio runtime 对 UI 暴露的生命周期状态。
///
/// 状态机只描述 runtime 服务自身是否可接受请求；单个 turn 的运行阶段仍由
/// `StudioTurnStatus` 和 session 事件表达。
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
/// 每个会话同一时间最多暴露一个活动 turn；更细的 phase 由 turn event 表达。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioActiveTurn {
    pub session_id: String,
    pub turn_id: String,
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
}

impl StudioRuntimeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StudioRuntimeStateInner {
                status: StudioRuntimeStatus::Uninitialized,
                active_turns: BTreeMap::new(),
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

    pub fn mark_active_turn(&self, session_id: String, turn_id: String) -> StudioRuntimeSnapshot {
        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        inner.active_turns.insert(session_id, turn_id);
        inner.updated_at = unix_seconds();
        snapshot_from_inner(&inner)
    }

    pub fn clear_active_turn(&self, session_id: &str) -> StudioRuntimeSnapshot {
        let mut inner = self.inner.lock().expect("runtime state mutex poisoned");
        inner.active_turns.remove(session_id);
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
            .map(|(session_id, turn_id)| StudioActiveTurn {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
            })
            .collect(),
        updated_at: inner.updated_at,
        error: inner.error.clone(),
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
