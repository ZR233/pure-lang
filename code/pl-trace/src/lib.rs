//! Agent 执行 trace 的 canonical typed 生命周期与事件。

mod part;
mod sink;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use pl_protocol::{
    AgentRuntimeDelta, BudgetLimitKind, BudgetUsage, ErrorSeverity, InteractionChangedEvent,
    SkillActivation, TodoListSnapshot,
};

pub use part::*;
pub use sink::*;

pub type AgentEventSender = broadcast::Sender<AgentEvent>;
pub type AgentEventReceiver = broadcast::Receiver<AgentEvent>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AgentEvent {
    TracePartStarted {
        item: TracePart,
    },
    TracePartDelta {
        event: TracePartDeltaEvent,
    },
    TracePartCompleted {
        item: TracePart,
    },
    TracePartFailed {
        item: TracePart,
    },
    InteractionChanged {
        event: InteractionChangedEvent,
    },
    AgentRuntimeUpdated {
        delta: AgentRuntimeDelta,
    },
    SkillActivated {
        activation: SkillActivation,
    },
    TodoListUpdated {
        snapshot: TodoListSnapshot,
    },
    TurnInterrupted {
        reason: String,
    },
    TurnBudgetLimited {
        reason: String,
        limit_kind: BudgetLimitKind,
        usage: BudgetUsage,
    },
    Done,
    Error {
        message: String,
        severity: ErrorSeverity,
    },
}

/// Append-only internal trace event for core diagnostics.
///
/// Studio UI may only receive these events after `pl-core` maps them into
/// durable message/part snapshots or live-only part deltas.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEvent {
    pub session_id: String,
    pub sequence: u64,
    pub timestamp: i64,
    pub kind: TraceEventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnabledToolsEvent {
    pub turn_id: String,
    pub step: u32,
    pub tools: Vec<String>,
    pub wire_fingerprint: String,
    pub execution_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum TraceEventKind {
    TracePartStarted { item: TracePart },
    TracePartDelta { event: TracePartDeltaEvent },
    TracePartCompleted { item: TracePart },
    TracePartFailed { item: TracePart },
    InteractionChanged { event: InteractionChangedEvent },
    SkillActivated { activation: SkillActivation },
    EnabledToolsRecorded { event: EnabledToolsEvent },
}
