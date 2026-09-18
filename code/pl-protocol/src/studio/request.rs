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
    /// 会话工作区模式；省略时按 `local` 解释。
    #[serde(default)]
    pub workspace_mode: crate::ThreadWorkspaceMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioPromptInput {
    pub input_id: String,
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
pub struct SubmitPromptRequest {
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
        let error = serde_json::from_value::<SubmitPromptRequest>(serde_json::json!({
            "input": {"inputId": "request-1", "text": "hello", "attachmentDraftIds": []},
            "unknown": true,
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn create_thread_title_is_optional_and_accepts_explicit_title() {
        let missing = serde_json::from_value::<CreateThreadRequest>(serde_json::json!({
            "input": {"inputId": "request-1", "text": "hello", "attachmentDraftIds": []},
            "mode": "mode.simple",
        }))
        .unwrap();
        assert_eq!(missing.workspace_mode, crate::ThreadWorkspaceMode::Local);
        assert_eq!(missing.title, None);

        let explicit = serde_json::from_value::<CreateThreadRequest>(serde_json::json!({
            "title": "Explicit title",
            "input": {"inputId": "request-1", "text": "hello", "attachmentDraftIds": []},
            "mode": "mode.simple",
        }))
        .unwrap();
        assert_eq!(explicit.title.as_deref(), Some("Explicit title"));
    }

    #[test]
    fn create_thread_workspace_mode_is_camel_case_and_defaults_to_local() {
        let worktree = serde_json::from_value::<CreateThreadRequest>(serde_json::json!({
            "input": {"inputId": "request-1", "text": "hello", "attachmentDraftIds": []},
            "mode": "mode.simple",
            "workspaceMode": "worktree",
        }))
        .unwrap();
        assert_eq!(
            worktree.workspace_mode,
            crate::ThreadWorkspaceMode::Worktree
        );
        let encoded = serde_json::to_value(&worktree).unwrap();
        assert_eq!(encoded["workspaceMode"], serde_json::json!("worktree"));

        let local = serde_json::from_value::<CreateThreadRequest>(serde_json::json!({
            "input": {"inputId": "request-1", "text": "hello", "attachmentDraftIds": []},
            "mode": "mode.simple",
        }))
        .unwrap();
        assert_eq!(local.workspace_mode, crate::ThreadWorkspaceMode::Local);
    }
}
