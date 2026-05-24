use serde::Deserialize;
use serde::Serialize;
use tokio::sync::broadcast;

pub type AgentEventSender = broadcast::Sender<AgentEvent>;
pub type AgentEventReceiver = broadcast::Receiver<AgentEvent>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    TurnStarted,
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
}
