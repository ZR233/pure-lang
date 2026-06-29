use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInteractionChangedDto {
    pub interaction_id: String,
    pub kind: String,
    pub status: String,
    pub session_id: String,
    pub turn_id: String,
    pub item_id: Option<String>,
    pub tool_id: Option<String>,
    pub agent_path: Option<String>,
    pub payload: BridgeInteractionPayloadDto,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeInteractionPayloadDto {
    UserInput {
        questions: Vec<BridgeUserQuestionDto>,
    },
    ToolApproval {
        name: String,
        arguments_json: String,
        working_directory: Option<String>,
        parent_agent_id: Option<String>,
    },
    PlanConfirmation {
        plan_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeUserQuestionDto {
    pub id: String,
    pub header: String,
    pub question: String,
    pub is_other: bool,
    pub is_secret: bool,
    pub options: Option<Vec<BridgeUserQuestionOptionDto>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeUserQuestionOptionDto {
    pub label: String,
    pub description: String,
}
