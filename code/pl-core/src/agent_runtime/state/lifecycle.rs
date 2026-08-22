use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::AgentRoleId;
use crate::agent_runtime::{ThreadId, TurnId};

use super::AgentState;
use super::{mailbox::*, snapshot::*};

/// runtime event 转换为 AgentCommand 时使用的瞬时活动更新。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum AgentActivityUpdate {
    Running,
    WaitingTool,
    WaitingInteraction { interaction_id: String },
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
            .position(|input| input.delivery_state.is_pending())
    }

    pub(crate) fn triggering_turn_id(&self) -> Option<TurnId> {
        let first = self.triggering_input_position()?;
        let input = &self.pending_inputs[first];
        let Some(key) = input.queue_coalescing_key.as_deref() else {
            return Some(input.turn_id.clone());
        };
        Some(
            self.pending_inputs
                .iter()
                .skip(first)
                .take_while(|candidate| {
                    candidate.delivery_state.is_pending()
                        && candidate.queue_coalescing_key.as_deref() == Some(key)
                })
                .last()
                .expect("the triggering input starts its coalescing group")
                .turn_id
                .clone(),
        )
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
                state: AgentState::idle(),
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
