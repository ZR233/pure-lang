//! Studio 命令请求体。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenProjectRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchSkillsRequest {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateThreadRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub input: StudioPromptInput,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioPromptInput {
    pub text: String,
    pub attachment_draft_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetThreadModeRequest {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameThreadRequest {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartTurnRequest {
    pub input: StudioPromptInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SteerTurnRequest {
    pub input: StudioPromptInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InterruptTurnRequest {
    pub expected_turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveInteractionRequest {
    pub resolution: crate::InteractionResolution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpectedRevisionRequest {
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpectedOpaqueRevisionRequest {
    pub expected_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeRequest {
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepairLspRequest {
    pub server_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", tag = "scope", deny_unknown_fields)]
pub enum McpResetRequest {
    Server { server_id: String },
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", tag = "scope", deny_unknown_fields)]
pub enum LspResetRequest {
    Server {
        project_id: String,
        server_id: String,
    },
    Workspace {
        project_id: String,
    },
    All,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_bodies_reject_unknown_fields() {
        let error = serde_json::from_value::<StartTurnRequest>(serde_json::json!({
            "input": {"text": "hello", "attachmentDraftIds": []},
            "unknown": true,
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn create_thread_title_is_optional_and_accepts_explicit_title() {
        let missing = serde_json::from_value::<CreateThreadRequest>(serde_json::json!({
            "input": {"text": "hello", "attachmentDraftIds": []},
            "mode": "mode.simple",
        }))
        .unwrap();
        assert_eq!(missing.title, None);

        let explicit = serde_json::from_value::<CreateThreadRequest>(serde_json::json!({
            "title": "Explicit title",
            "input": {"text": "hello", "attachmentDraftIds": []},
            "mode": "mode.simple",
        }))
        .unwrap();
        assert_eq!(explicit.title.as_deref(), Some("Explicit title"));
    }
}
