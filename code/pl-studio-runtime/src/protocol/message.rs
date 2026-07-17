use serde::{Deserialize, Serialize};

use pl_protocol::{AgentStatus, TokenUsageSnapshot};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMessage {
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub role: StudioMessageRole,
    pub status: StudioMessageStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default = "empty_object", skip_serializing_if = "is_empty_object")]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioMessageRole {
    User,
    Assistant,
    System,
}

impl StudioMessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioMessageStatus {
    Queued,
    Streaming,
    Completed,
    Failed,
    Cancelled,
}

impl StudioMessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Streaming => "streaming",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPart {
    pub part_id: String,
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub part_type: StudioPartType,
    pub order: u64,
    #[serde(default)]
    pub revision: u64,
    pub status: StudioPartStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_channel: Option<StudioTextChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_group_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<StudioAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<StudioToolPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<StudioAgentPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<StudioInferencePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<StudioPlanPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<StudioFilePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsageSnapshot>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub synthetic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ignored: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioPartType {
    Text,
    Reasoning,
    Tool,
    Agent,
    Turn,
    Inference,
    Plan,
    File,
}

impl StudioPartType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Reasoning => "reasoning",
            Self::Tool => "tool",
            Self::Agent => "agent",
            Self::Turn => "turn",
            Self::Inference => "inference",
            Self::Plan => "plan",
            Self::File => "file",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioPartStatus {
    Started,
    Streaming,
    AwaitingApproval,
    Approved,
    Denied,
    Running,
    Completed,
    Failed,
    Interrupted,
    BudgetLimited,
}

impl StudioPartStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Streaming => "streaming",
            Self::AwaitingApproval => "awaitingApproval",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::BudgetLimited => "budgetLimited",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTextChannel {
    User,
    Commentary,
    Final,
}

impl StudioTextChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Commentary => "commentary",
            Self::Final => "final",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAttachment {
    pub id: String,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    pub byte_size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioToolPart {
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_item_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentPart {
    pub id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub depth: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioInferencePart {
    pub inference_id: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPlanPart {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioFilePart {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPartDelta {
    pub part_id: String,
    #[serde(default)]
    pub revision: u64,
    pub field: StudioPartDeltaField,
    pub delta: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioPartDeltaField {
    Text,
    #[serde(rename = "reasoning.summary")]
    ReasoningSummary,
    PlanContent,
    #[serde(rename = "tool.arguments")]
    ToolArguments,
    #[serde(rename = "tool.result")]
    ToolResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTurn {
    pub turn_id: String,
    pub session_id: String,
    pub status: StudioTurnStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTurnStatus {
    Queued,
    ContextLoading,
    WaitingForModel,
    Streaming,
    WaitingForInteraction,
    RunningTool,
    Persisting,
    Completed,
    Failed,
    Cancelled,
}

impl StudioTurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::ContextLoading => "contextLoading",
            Self::WaitingForModel => "waitingForModel",
            Self::Streaming => "streaming",
            Self::WaitingForInteraction => "waitingForInteraction",
            Self::RunningTool => "runningTool",
            Self::Persisting => "persisting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

fn empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn is_empty_object(value: &serde_json::Value) -> bool {
    value.as_object().is_some_and(serde_json::Map::is_empty)
}

fn is_false(value: &bool) -> bool {
    !*value
}
