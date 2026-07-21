use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    #[serde(default)]
    pub is_other: bool,
    #[serde(default)]
    pub is_secret: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<UserQuestionOption>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserInputRequest {
    pub request_id: String,
    pub tool_id: String,
    pub questions: Vec<UserQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserInputAnswer {
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserInputResponse {
    pub answers: HashMap<String, UserInputAnswer>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlanLifecycleState {
    PendingConfirmation,
    Accepted,
    Implementing,
    Implemented,
    ImplementationFailed,
    ContinuedPlanning,
    Dismissed,
    Cancelled,
}

impl PlanLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PendingConfirmation => "pendingConfirmation",
            Self::Accepted => "accepted",
            Self::Implementing => "implementing",
            Self::Implemented => "implemented",
            Self::ImplementationFailed => "implementationFailed",
            Self::ContinuedPlanning => "continuedPlanning",
            Self::Dismissed => "dismissed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanLifecycleEvent {
    pub plan_id: String,
    pub state: PlanLifecycleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubAgentActivityKind {
    Spawned,
    MessageQueued,
    FollowupStarted,
    WaitCompleted,
    Closed,
}

impl SubAgentActivityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spawned => "spawned",
            Self::MessageQueued => "messageQueued",
            Self::FollowupStarted => "followupStarted",
            Self::WaitCompleted => "waitCompleted",
            Self::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

impl TodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "inProgress",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub step: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TodoListSnapshot {
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    pub items: Vec<TodoItem>,
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

/// Estimated runtime cost in a single currency.
///
/// Costs are local estimates derived from configured per-million-token prices.
/// Different currencies must remain separate and are never converted or summed
/// together.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCostAmount {
    pub currency: String,
    pub amount: f64,
}

/// Cumulative runtime usage snapshot.
///
/// Used by product DTOs to expose the current usage total for either a session
/// or a single agent. `estimated_costs` is grouped by currency.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUsageSnapshot {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default)]
    pub has_unpriced_usage: bool,
    pub updated_at: i64,
}

/// Per-inference runtime usage attributed to a root or child agent.
///
/// `inference_id` is stable for a model call and is used by product persistence
/// as an idempotency key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeDelta {
    pub inference_id: String,
    pub agent_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_path: Option<String>,
    pub role: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    pub usage: TokenUsageSnapshot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default)]
    pub has_unpriced_usage: bool,
    pub updated_at: i64,
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

/// Token usage snapshot shared by product projection and internal trace mapping.
///
/// Lightweight copy of `pl_model::TokenUsage` to avoid coupling public protocol
/// DTOs to `pl-model`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
}

/// Successful skill activation fact for a session.
///
/// Emitted when `skill_view` successfully reads a skill document or support
/// file and that content has entered the model-visible context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillActivation {
    pub name: String,
    pub source: String,
    pub path: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub activated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn user_input_defaults_and_response_shape_match_codex_style() {
        let question: UserQuestion = serde_json::from_value(serde_json::json!({
            "id": "notes",
            "header": "Notes",
            "question": "Anything else?"
        }))
        .unwrap();
        let response = UserInputResponse {
            answers: HashMap::from([(
                "notes".to_string(),
                UserInputAnswer {
                    answers: vec!["Ship it".to_string()],
                },
            )]),
        };

        assert_eq!(
            question,
            UserQuestion {
                id: "notes".to_string(),
                header: "Notes".to_string(),
                question: "Anything else?".to_string(),
                is_other: false,
                is_secret: false,
                options: None,
            }
        );
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({
                "answers": {
                    "notes": {
                        "answers": ["Ship it"]
                    }
                }
            })
        );
    }
}
