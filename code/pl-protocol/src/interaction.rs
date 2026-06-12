use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::event::{UserInputAnswer, UserQuestion};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InteractionKind {
    UserInput,
    ToolApproval,
    PlanConfirmation,
}

impl InteractionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserInput => "userInput",
            Self::ToolApproval => "toolApproval",
            Self::PlanConfirmation => "planConfirmation",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InteractionStatus {
    Pending,
    Resolved,
    Cancelled,
    Expired,
}

impl InteractionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionScope {
    pub session_id: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum InteractionPayload {
    UserInput {
        questions: Vec<UserQuestion>,
    },
    #[serde(rename_all = "camelCase")]
    ToolApproval {
        name: String,
        arguments: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        working_directory: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_agent_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    PlanConfirmation {
        plan_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    pub interaction_id: String,
    pub kind: InteractionKind,
    pub status: InteractionStatus,
    pub scope: InteractionScope,
    pub payload: InteractionPayload,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<InteractionResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum InteractionResolution {
    UserInput {
        answers: HashMap<String, UserInputAnswer>,
    },
    ToolApproval {
        decision: ToolApprovalResolution,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    PlanConfirmation {
        decision: PlanConfirmationResolution,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolApprovalResolution {
    Approved,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlanConfirmationResolution {
    ImplementFreshContext,
    ContinuePlanning,
    Dismiss,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionChangedEvent {
    pub interaction: InteractionRequest,
}
