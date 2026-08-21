use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::AgentRoleId;
use crate::agent_runtime::{ThreadId, TurnId};

use super::{mailbox::*, snapshot::*};

/// agent 资源仍可执行工作的生命周期状态。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycleState {
    Active,
    Closing,
    Closed,
    Faulted,
}

/// active Turn 内部可展示的活动种类。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ActiveKind {
    Running,
    WaitingTool,
    WaitingInteraction,
}

/// agent 当前执行活动；只能从 lifecycle、active Turn 与 triggering input 派生。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentActivityState {
    Idle,
    Queued,
    Active(ActiveKind),
    Cancelling,
}

/// 单轮执行结果，不用作 agent 生命周期。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnOutcomeKind {
    Completed,
    Cancelled,
    Failed,
    BudgetLimited,
}

/// agent 最新进度阶段；`ReadyForReview` 仅由产品的 durable completion 路径提升。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentProgressStage {
    Exploring,
    Implementing,
    Verifying,
    Blocked,
    ReadyForCompletion,
    ReadyForReview,
}

/// AgentLoop 用来派生活动投影的 active Turn 输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveTurnActivity {
    pub(crate) kind: ActiveKind,
    pub(crate) cancelling: bool,
}

pub(crate) fn derive_activity(
    lifecycle: AgentLifecycleState,
    active: Option<ActiveTurnActivity>,
    has_triggering_input: bool,
) -> AgentActivityState {
    if lifecycle != AgentLifecycleState::Active {
        return AgentActivityState::Idle;
    }
    match active {
        Some(active) if active.cancelling => AgentActivityState::Cancelling,
        Some(active) => AgentActivityState::Active(active.kind),
        None if has_triggering_input => AgentActivityState::Queued,
        None => AgentActivityState::Idle,
    }
}

/// repository 原子提交和恢复使用的 agent 全量 durable state。
#[derive(Debug, Clone)]
pub struct ThreadActorState {
    pub snapshot: AgentSnapshot,
    pub session: ThreadContextState,
    pub pending_inputs: VecDeque<DurableMailboxEnvelope>,
    pub active_input: Option<DurableMailboxEnvelope>,
}

impl ThreadActorState {
    pub(crate) fn has_triggering_input(&self) -> bool {
        self.triggering_input_position().is_some()
    }

    pub(crate) fn triggering_input_position(&self) -> Option<usize> {
        self.pending_inputs
            .iter()
            .position(|input| matches!(input.delivery_state, MailboxDeliveryState::Pending))
    }

    pub(crate) fn refresh_mailbox_snapshot(&mut self) {
        self.snapshot.pending_inputs = self.pending_inputs.len();
    }
}

/// 新 agent 注册输入；外部资源生命周期由产品或 spawn saga 准备。
#[derive(Debug, Clone)]
pub struct AgentRegistration {
    pub identity: AgentIdentity,
    pub session: ThreadContextState,
    pub runtime_revision: u64,
    pub event_sequence: u64,
}

/// runtime 负责 lifecycle saga 的 child agent 创建请求。
#[derive(Debug, Clone)]
pub struct AgentSpawnRequest {
    pub thread_id: ThreadId,
    pub parent_id: ThreadId,
    pub role: AgentRoleId,
    pub session: ThreadContextState,
    /// 产品需要幂等重试时可提供稳定的首轮 id。
    pub initial_turn_id: Option<TurnId>,
    pub initial_message: Option<String>,
    pub metadata: serde_json::Value,
}

/// child agent 注册完成后的稳定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpawnResult {
    pub snapshot: AgentSnapshot,
    pub initial_turn_id: Option<TurnId>,
}

impl AgentRegistration {
    /// 为 identity 对应的 Thread 创建空运行上下文。
    pub fn new(identity: AgentIdentity) -> Self {
        Self {
            identity,
            session: ThreadContextState::empty(),
            runtime_revision: 1,
            event_sequence: 1,
        }
    }

    pub(crate) fn into_durable_state(self) -> ThreadActorState {
        let now = unix_timestamp();
        ThreadActorState {
            snapshot: AgentSnapshot {
                identity: self.identity,
                lifecycle: AgentLifecycleState::Active,
                activity: AgentActivityState::Idle,
                active_turn_id: None,
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision: self.runtime_revision,
                event_sequence: self.event_sequence,
                updated_at: now,
            },
            session: self.session,
            pending_inputs: VecDeque::new(),
            active_input: None,
        }
    }
}

pub(crate) fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_activity_from_lifecycle_active_turn_and_triggering_input() {
        let active_cases = [
            None,
            Some(ActiveTurnActivity {
                kind: ActiveKind::Running,
                cancelling: false,
            }),
            Some(ActiveTurnActivity {
                kind: ActiveKind::WaitingTool,
                cancelling: false,
            }),
            Some(ActiveTurnActivity {
                kind: ActiveKind::WaitingInteraction,
                cancelling: true,
            }),
        ];
        for lifecycle in [
            AgentLifecycleState::Active,
            AgentLifecycleState::Closing,
            AgentLifecycleState::Closed,
            AgentLifecycleState::Faulted,
        ] {
            for active in active_cases {
                for has_triggering_input in [false, true] {
                    let expected = if lifecycle != AgentLifecycleState::Active {
                        AgentActivityState::Idle
                    } else {
                        match active {
                            Some(active) if active.cancelling => AgentActivityState::Cancelling,
                            Some(active) => AgentActivityState::Active(active.kind),
                            None if has_triggering_input => AgentActivityState::Queued,
                            None => AgentActivityState::Idle,
                        }
                    };
                    assert_eq!(
                        derive_activity(lifecycle, active, has_triggering_input),
                        expected,
                        "lifecycle={lifecycle:?} active={active:?} pending={has_triggering_input}"
                    );
                }
            }
        }
    }
}
