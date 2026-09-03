use std::path::Path;

use pl_protocol::{
    InteractionRequest, InteractionResolution, InteractionScope,
    ToolApprovalRequest as InteractionToolApprovalRequest, ToolApprovalResolution,
    ToolApprovalResolutionPayload,
};

use crate::tool::{PathAccess, SubagentContext, ToolPathPolicy, WorkspaceAccess};
use crate::turn::{ToolApprovalDecision, ToolApprovalRequest, TurnOptions};

use crate::time::unix_seconds;

pub(super) fn approval_request(
    tool_call: &pl_model::completion::ToolCall,
    active_subagent: Option<&SubagentContext>,
) -> ToolApprovalRequest {
    let arguments = tool_call.arguments_for_display();
    let tool_arguments = tool_call.arguments_for_tool();
    let working_directory =
        get_working_directory(&tool_arguments).or_else(|| get_working_directory(&arguments));
    ToolApprovalRequest {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments,
        working_directory,
        parent_agent_id: active_subagent.map(|subagent| subagent.id.clone()),
    }
}

pub(super) fn get_working_directory(arguments: &serde_json::Value) -> Option<String> {
    arguments
        .get("cwd")
        .or_else(|| arguments.get("workingDirectory"))
        .or_else(|| arguments.get("working_directory"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(super) fn requested_workspace_access(
    tool_call: &pl_model::completion::ToolCall,
    workspace_root: &Path,
) -> WorkspaceAccess {
    let arguments = tool_call.arguments_for_tool();
    let paths = requested_paths_for_tool(&tool_call.name, &arguments);
    let Ok(policy) = ToolPathPolicy::new(workspace_root.to_path_buf(), false, "permission") else {
        return WorkspaceAccess::ExternalAllowed;
    };
    if paths
        .iter()
        .any(|path| path_requires_external_access(path, &policy))
    {
        WorkspaceAccess::ExternalAllowed
    } else {
        WorkspaceAccess::WorkspaceOnly
    }
}

pub(super) fn requested_paths_for_tool(name: &str, arguments: &serde_json::Value) -> Vec<String> {
    if name.starts_with("lsp_query_") {
        return argument_path(arguments, "filePath").into_iter().collect();
    }
    match name {
        "exec" => get_working_directory(arguments).into_iter().collect(),
        "write_stdin" => Vec::new(),
        "read_file" | "write_file" | "stat_path" | "create_directory" | "delete_path" => {
            argument_path(arguments, "path").into_iter().collect()
        }
        "lsp_query" => argument_path(arguments, "filePath").into_iter().collect(),
        "list_files" => argument_path(arguments, "path")
            .into_iter()
            .filter(|path| !path.trim().is_empty())
            .collect(),
        "copy_path" | "move_path" => ["from", "to"]
            .into_iter()
            .filter_map(|key| argument_path(arguments, key))
            .collect(),
        "apply_patch" => arguments
            .get("input")
            .and_then(serde_json::Value::as_str)
            .map(paths_from_patch_text)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub(super) fn argument_path(arguments: &serde_json::Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(super) fn paths_from_patch_text(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch.lines() {
        let line = line.trim_start();
        for prefix in [
            "*** Add File:",
            "*** Delete File:",
            "*** Update File:",
            "*** Move to:",
        ] {
            if let Some(path) = line.strip_prefix(prefix) {
                paths.push(path.trim().to_string());
            }
        }
    }
    paths
}

pub(super) fn path_requires_external_access(path: &str, policy: &ToolPathPolicy) -> bool {
    policy.access_for_input(path) == PathAccess::External
}

pub(super) fn permission_risk_summary(tool_name: &str) -> &'static str {
    if crate::mcp::is_mcp_tool_name(tool_name) {
        return "trusted MCP server tool";
    }
    if tool_name.starts_with("lsp_query_") {
        return "read-only LSP code intelligence query";
    }
    match tool_name {
        "exec" => "shell command; may execute arbitrary process actions",
        "write_stdin" => "stdin or polling for an already approved shell process",
        "write_file" => "file write; may create, overwrite, or append content",
        "create_directory" => "filesystem write; creates directories",
        "delete_path" => "destructive filesystem operation",
        "copy_path" => "filesystem write; copies files or directories",
        "move_path" => "filesystem write; moves or renames paths",
        "apply_patch" => "batch filesystem edit",
        "lsp_query" => "read-only LSP code intelligence query",
        "skill_manage" => "project skill write or management",
        _ => "read-only or coordination tool",
    }
}

pub(super) async fn request_user_approval(
    options: &TurnOptions,
    request: &ToolApprovalRequest,
    turn_id: &str,
) -> ToolApprovalDecision {
    let Some(callback) = &options.interaction_callback else {
        return ToolApprovalDecision::Denied {
            reason: "manual approval required but no interaction runtime is configured".to_string(),
        };
    };
    let interaction = tool_approval_interaction(request, turn_id);
    let resolution = match &options.cancellation_token {
        Some(token) => {
            tokio::select! {
                resolution = callback(interaction.clone()) => resolution,
                _ = token.cancelled() => InteractionResolution::ToolApproval(ToolApprovalResolutionPayload {
                    decision: ToolApprovalResolution::Denied,
                    reason: Some(cancellation_reason()),
                }),
            }
        }
        None => callback(interaction.clone()).await,
    };
    match resolution {
        InteractionResolution::ToolApproval(value) => match value.decision {
            ToolApprovalResolution::Approved => ToolApprovalDecision::Approved,
            ToolApprovalResolution::Denied => ToolApprovalDecision::Denied {
                reason: value.reason.unwrap_or_else(|| "denied by user".to_string()),
            },
        },
        InteractionResolution::UserInput(_) => ToolApprovalDecision::Denied {
            reason: "interaction resolved with an incompatible payload".to_string(),
        },
    }
}

pub(super) fn cancellation_reason() -> String {
    "interrupted by user".to_string()
}

fn tool_approval_interaction(request: &ToolApprovalRequest, turn_id: &str) -> InteractionRequest {
    let now = unix_seconds();
    InteractionRequest::tool_approval(
        request.id.clone(),
        InteractionScope {
            thread_id: String::new(),
            turn_id: turn_id.to_string(),
            item_id: Some(request.id.clone()),
            tool_id: Some(request.id.clone()),
            agent_path: request.parent_agent_id.clone(),
        },
        InteractionToolApprovalRequest {
            name: request.name.clone(),
            arguments: request.arguments.clone(),
            working_directory: request.working_directory.clone(),
            parent_agent_id: request.parent_agent_id.clone(),
        },
        now,
    )
}

#[cfg(test)]
mod tests {
    use pl_model::completion::ToolCall;
    use pretty_assertions::assert_eq;

    use super::*;

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "pure-permission-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn relative_file_path_requests_workspace_only_access() {
        let root = unique_temp_dir("relative");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let tool_call = ToolCall::function(
            "call-1",
            "read_file",
            serde_json::json!({ "path": "src/lib.rs" }),
            "call-1",
        );

        let access = requested_workspace_access(&tool_call, &root);

        assert_eq!(access, WorkspaceAccess::WorkspaceOnly);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn external_absolute_path_requests_external_access() {
        let root = unique_temp_dir("root");
        let outside = unique_temp_dir("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let tool_call = ToolCall::function(
            "call-1",
            "read_file",
            serde_json::json!({ "path": outside.join("secret.txt") }),
            "call-1",
        );

        let access = requested_workspace_access(&tool_call, &root);

        assert_eq!(access, WorkspaceAccess::ExternalAllowed);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn parent_segment_requests_external_access() {
        let root = unique_temp_dir("parent");
        std::fs::create_dir_all(&root).unwrap();
        let tool_call = ToolCall::function(
            "call-1",
            "exec",
            serde_json::json!({ "command": "pwd", "cwd": ".." }),
            "call-1",
        );

        let access = requested_workspace_access(&tool_call, &root);

        assert_eq!(access, WorkspaceAccess::ExternalAllowed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn custom_apply_patch_paths_are_classified() {
        let root = unique_temp_dir("patch");
        std::fs::create_dir_all(&root).unwrap();
        let patch = "*** Begin Patch\n*** Add File: src/new.rs\n+fn main() {}\n*** End Patch";
        let tool_call = ToolCall::custom("call-1", "apply_patch", patch, "call-1");

        let access = requested_workspace_access(&tool_call, &root);

        assert_eq!(access, WorkspaceAccess::WorkspaceOnly);
        let _ = std::fs::remove_dir_all(root);
    }
}
