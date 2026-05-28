use serde::Deserialize;
use serde::Serialize;
use tokio::sync::broadcast;

pub type AgentEventSender = broadcast::Sender<AgentEvent>;
pub type AgentEventReceiver = broadcast::Receiver<AgentEvent>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AgentEvent {
    TextDelta {
        content: String,
    },
    ThinkingDelta {
        content: String,
    },
    ToolCallDelta {
        id: String,
        name: String,
        arguments_delta: String,
    },
    ToolCallComplete {
        id: String,
        name: String,
        arguments: String,
    },
    ToolApprovalRequested {
        id: String,
        name: String,
        arguments: String,
        #[serde(rename = "workingDirectory")]
        working_directory: Option<String>,
    },
    ToolApprovalGranted {
        id: String,
        name: String,
    },
    ToolApprovalDenied {
        id: String,
        name: String,
        reason: String,
    },
    AgentStateChanged {
        id: String,
        path: String,
        #[serde(rename = "parentPath")]
        parent_path: Option<String>,
        role: String,
        task: String,
        status: AgentStatus,
        summary: Option<String>,
        depth: u32,
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        budget_limit_kind: Option<BudgetLimitKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        budget_usage: Option<BudgetUsage>,
        #[serde(rename = "updatedAt")]
        updated_at: i64,
    },
    TurnStarted,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorSeverity {
    Transient,
    Recoverable,
    Fatal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Queued,
    Running,
    Waiting,
    Completed,
    Errored,
    Interrupted,
    Shutdown,
    NotFound,
}

impl AgentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Errored => "errored",
            Self::Interrupted => "interrupted",
            Self::Shutdown => "shutdown",
            Self::NotFound => "notFound",
        }
    }

    pub fn is_final(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Errored | Self::Shutdown | Self::NotFound
        )
    }
}

/// Kind of runtime budget that stopped a turn or agent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BudgetLimitKind {
    ModelStep,
    ToolCall,
    Wait,
    WallClock,
    AgentCount,
    AgentDepth,
    Finalization,
}

impl BudgetLimitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModelStep => "modelStep",
            Self::ToolCall => "toolCall",
            Self::Wait => "wait",
            Self::WallClock => "wallClock",
            Self::AgentCount => "agentCount",
            Self::AgentDepth => "agentDepth",
            Self::Finalization => "finalization",
        }
    }
}

/// Snapshot of consumed turn budgets.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetUsage {
    pub model_steps: u32,
    pub tool_calls: u32,
    pub wait_calls: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PipelineStage {
    IntentAnalysis,
    Planning,
    CodeGeneration,
    Verification,
    Integration,
}

/// Token usage snapshot for trace events.
///
/// Lightweight copy of `pl_model::TokenUsage` to avoid coupling `pl-protocol` to `pl-model`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
}

/// Append-only trace event for structured session lifecycle tracking.
///
/// Each event belongs to a session and carries a monotonic sequence number
/// for causal ordering. The `kind` field discriminates the event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEvent {
    pub session_id: String,
    pub sequence: u64,
    pub timestamp: i64,
    pub kind: TraceEventKind,
}

/// Trace event variants for turn, inference, and tool call lifecycle.
///
/// Grouped by correlation IDs:
/// - `turn_id` correlates events within a single turn
/// - `inference_id` correlates inference call events
/// - `tool_call_id` correlates tool lifecycle events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum TraceEventKind {
    TurnStarted {
        turn_id: String,
    },
    TurnCompleted {
        turn_id: String,
        content: String,
        model: String,
        usage: TokenUsageSnapshot,
    },
    TurnFailed {
        turn_id: String,
        error: String,
    },
    TurnInterrupted {
        turn_id: String,
        reason: String,
    },
    TurnBudgetLimited {
        turn_id: String,
        reason: String,
        limit_kind: BudgetLimitKind,
        usage: BudgetUsage,
        last_content: String,
    },
    InferenceStarted {
        turn_id: String,
        inference_id: String,
        model: String,
    },
    InferenceCompleted {
        turn_id: String,
        inference_id: String,
        usage: TokenUsageSnapshot,
    },
    ToolCallStarted {
        turn_id: String,
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    ToolCallApproved {
        tool_call_id: String,
    },
    ToolCallDenied {
        tool_call_id: String,
        reason: String,
    },
    ToolCallCompleted {
        tool_call_id: String,
        result: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        timed_out: bool,
    },
    ToolCallFailed {
        tool_call_id: String,
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn serializes_tool_approval_requested_as_camel_case() {
        let event = AgentEvent::ToolApprovalRequested {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: "{\"command\":\"echo hi\"}".to_string(),
            working_directory: Some("C:/project".to_string()),
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "toolApprovalRequested": {
                    "id": "call-1",
                    "name": "bash",
                    "arguments": "{\"command\":\"echo hi\"}",
                    "workingDirectory": "C:/project"
                }
            })
        );
    }

    #[test]
    fn serializes_tool_approval_decision_events_as_camel_case() {
        let granted = serde_json::to_value(AgentEvent::ToolApprovalGranted {
            id: "call-1".to_string(),
            name: "bash".to_string(),
        })
        .unwrap();
        let denied = serde_json::to_value(AgentEvent::ToolApprovalDenied {
            id: "call-2".to_string(),
            name: "subagent".to_string(),
            reason: "not now".to_string(),
        })
        .unwrap();

        assert_eq!(
            granted,
            serde_json::json!({
                "toolApprovalGranted": {
                    "id": "call-1",
                    "name": "bash"
                }
            })
        );
        assert_eq!(
            denied,
            serde_json::json!({
                "toolApprovalDenied": {
                    "id": "call-2",
                    "name": "subagent",
                    "reason": "not now"
                }
            })
        );
    }

    #[test]
    fn serializes_trace_event_turn_started_as_camel_case() {
        let event = TraceEvent {
            session_id: "sess-1".to_string(),
            sequence: 0,
            timestamp: 1_779_688_800,
            kind: TraceEventKind::TurnStarted {
                turn_id: "turn-1".to_string(),
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "sessionId": "sess-1",
                "sequence": 0,
                "timestamp": 1779688800,
                "kind": {
                    "type": "turnStarted",
                    "turnId": "turn-1"
                }
            })
        );
    }

    #[test]
    fn serializes_turn_interrupted_as_camel_case() {
        let event = AgentEvent::TurnInterrupted {
            reason: "stopped by user".to_string(),
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "turnInterrupted": {
                    "reason": "stopped by user"
                }
            })
        );
    }

    #[test]
    fn serializes_turn_budget_limited_as_camel_case() {
        let event = AgentEvent::TurnBudgetLimited {
            reason: "budget limited".to_string(),
            limit_kind: BudgetLimitKind::ToolCall,
            usage: BudgetUsage {
                model_steps: 3,
                tool_calls: 121,
                wait_calls: 2,
                elapsed_ms: 42,
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "turnBudgetLimited": {
                    "reason": "budget limited",
                    "limitKind": "toolCall",
                    "usage": {
                        "modelSteps": 3,
                        "toolCalls": 121,
                        "waitCalls": 2,
                        "elapsedMs": 42
                    }
                }
            })
        );
    }

    #[test]
    fn serializes_trace_event_turn_interrupted_as_camel_case() {
        let event = TraceEvent {
            session_id: "sess-1".to_string(),
            sequence: 2,
            timestamp: 1_779_688_820,
            kind: TraceEventKind::TurnInterrupted {
                turn_id: "turn-1".to_string(),
                reason: "stopped by user".to_string(),
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "sessionId": "sess-1",
                "sequence": 2,
                "timestamp": 1779688820,
                "kind": {
                    "type": "turnInterrupted",
                    "turnId": "turn-1",
                    "reason": "stopped by user"
                }
            })
        );
    }

    #[test]
    fn serializes_trace_event_turn_budget_limited_as_camel_case() {
        let event = TraceEvent {
            session_id: "sess-1".to_string(),
            sequence: 3,
            timestamp: 1_779_688_830,
            kind: TraceEventKind::TurnBudgetLimited {
                turn_id: "turn-1".to_string(),
                reason: "tool budget".to_string(),
                limit_kind: BudgetLimitKind::ToolCall,
                usage: BudgetUsage {
                    model_steps: 4,
                    tool_calls: 121,
                    wait_calls: 1,
                    elapsed_ms: 50,
                },
                last_content: "partial".to_string(),
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "sessionId": "sess-1",
                "sequence": 3,
                "timestamp": 1779688830,
                "kind": {
                    "type": "turnBudgetLimited",
                    "turnId": "turn-1",
                    "reason": "tool budget",
                    "limitKind": "toolCall",
                    "usage": {
                        "modelSteps": 4,
                        "toolCalls": 121,
                        "waitCalls": 1,
                        "elapsedMs": 50
                    },
                    "lastContent": "partial"
                }
            })
        );
    }

    #[test]
    fn serializes_trace_event_tool_call_completed_as_camel_case() {
        let event = TraceEvent {
            session_id: "sess-1".to_string(),
            sequence: 5,
            timestamp: 1_779_688_900,
            kind: TraceEventKind::ToolCallCompleted {
                tool_call_id: "call-1".to_string(),
                result: "ok".to_string(),
                exit_code: Some(0),
                timed_out: false,
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "sessionId": "sess-1",
                "sequence": 5,
                "timestamp": 1779688900,
                "kind": {
                    "type": "toolCallCompleted",
                    "toolCallId": "call-1",
                    "result": "ok",
                    "exitCode": 0,
                    "timedOut": false
                }
            })
        );
    }

    #[test]
    fn trace_event_tool_call_completed_omits_none_exit_code() {
        let event = TraceEvent {
            session_id: "sess-1".to_string(),
            sequence: 3,
            timestamp: 1_779_688_900,
            kind: TraceEventKind::ToolCallCompleted {
                tool_call_id: "call-1".to_string(),
                result: "done".to_string(),
                exit_code: None,
                timed_out: false,
            },
        };

        let json = serde_json::to_value(event).unwrap();
        let kind = json.get("kind").unwrap();

        assert!(kind.get("exitCode").is_none());
    }
}
