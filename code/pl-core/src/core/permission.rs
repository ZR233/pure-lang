use std::path::Path;

use pl_protocol::{
    AgentEvent, AgentEventSender, InteractionChangedEvent, InteractionKind, InteractionPayload,
    InteractionRequest, InteractionResolution, InteractionScope, InteractionStatus,
    ToolApprovalResolution,
};

#[cfg(test)]
use crate::permission::{PermissionDecision, decide_tool_permission};
use crate::tool::{PathAccess, ToolContext, ToolPathPolicy, WorkspaceAccess};
use crate::turn::{ToolApprovalDecision, ToolApprovalRequest, TurnOptions};

use super::turn_result::unix_seconds;

pub(super) fn approval_request(
    tool_call: &pl_model::ToolCall,
    context: &ToolContext,
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
        parent_agent_id: context
            .active_subagent
            .as_ref()
            .map(|subagent| subagent.id.clone()),
    }
}

pub(super) fn get_working_directory(arguments: &serde_json::Value) -> Option<String> {
    arguments
        .get("workingDirectory")
        .or_else(|| arguments.get("working_directory"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(super) fn requested_workspace_access(
    tool_call: &pl_model::ToolCall,
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
        "bash" => get_working_directory(arguments).into_iter().collect(),
        "write_stdin" => Vec::new(),
        "read_file" | "write_file" | "stat_path" | "create_directory" | "delete_path" => {
            argument_path(arguments, "path").into_iter().collect()
        }
        "lsp_query" => argument_path(arguments, "filePath").into_iter().collect(),
        "list_files" | "search_files" => argument_path(arguments, "path")
            .into_iter()
            .filter(|path| !path.trim().is_empty())
            .collect(),
        "copy_path" | "move_path" => ["from", "to"]
            .into_iter()
            .filter_map(|key| argument_path(arguments, key))
            .collect(),
        "apply_patch" => arguments
            .get("patch")
            .or_else(|| arguments.get("input"))
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
        "bash" => "shell command; may execute arbitrary process actions",
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

#[cfg(test)]
pub(super) async fn approve_tool_call(
    options: &TurnOptions,
    request: &ToolApprovalRequest,
    event_tx: AgentEventSender,
    context: &ToolContext,
) -> ToolApprovalDecision {
    match decide_tool_permission(options, context.mode, request, context.workspace_access) {
        PermissionDecision::Approved { .. } => ToolApprovalDecision::Approved,
        PermissionDecision::NeedsUserApproval { .. } => {
            request_user_approval(options, request, event_tx, "").await
        }
        PermissionDecision::NeedsAiReview { .. } => ToolApprovalDecision::Denied {
            reason: "AI reviewer approval requires the core execution path".to_string(),
        },
        PermissionDecision::Denied { reason } => ToolApprovalDecision::Denied { reason },
    }
}

pub(super) async fn request_user_approval(
    options: &TurnOptions,
    request: &ToolApprovalRequest,
    event_tx: AgentEventSender,
    turn_id: &str,
) -> ToolApprovalDecision {
    if let Some(callback) = &options.interaction_callback {
        let interaction = tool_approval_interaction(request, turn_id);
        let _ = event_tx.send(AgentEvent::InteractionChanged {
            event: InteractionChangedEvent {
                interaction: interaction.clone(),
            },
        });
        let resolution = match &options.cancellation_token {
            Some(token) => {
                tokio::select! {
                    resolution = callback(interaction.clone()) => resolution,
                    _ = token.cancelled() => InteractionResolution::ToolApproval {
                        decision: ToolApprovalResolution::Denied,
                        reason: Some(cancellation_reason()),
                    },
                }
            }
            None => callback(interaction.clone()).await,
        };
        return match resolution {
            InteractionResolution::ToolApproval { decision, reason } => match decision {
                ToolApprovalResolution::Approved => ToolApprovalDecision::Approved,
                ToolApprovalResolution::Denied => ToolApprovalDecision::Denied {
                    reason: reason.unwrap_or_else(|| "denied by user".to_string()),
                },
            },
            InteractionResolution::UserInput { .. }
            | InteractionResolution::PlanConfirmation { .. } => ToolApprovalDecision::Denied {
                reason: "interaction resolved with an incompatible payload".to_string(),
            },
        };
    }

    let _ = event_tx.send(AgentEvent::ToolApprovalRequested {
        id: request.id.clone(),
        name: request.name.clone(),
        arguments: serde_json::to_string(&request.arguments).unwrap_or_default(),
        working_directory: request.working_directory.clone(),
    });
    match &options.tool_approval_callback {
        Some(callback) => match &options.cancellation_token {
            Some(token) => {
                tokio::select! {
                    decision = callback(request.clone()) => decision,
                    _ = token.cancelled() => ToolApprovalDecision::Denied {
                        reason: cancellation_reason(),
                    },
                }
            }
            None => callback(request.clone()).await,
        },
        None => ToolApprovalDecision::Denied {
            reason: "manual approval required but no approver is configured".to_string(),
        },
    }
}

pub(super) fn cancellation_reason() -> String {
    "interrupted by user".to_string()
}

fn tool_approval_interaction(request: &ToolApprovalRequest, turn_id: &str) -> InteractionRequest {
    let now = unix_seconds();
    InteractionRequest {
        interaction_id: request.id.clone(),
        kind: InteractionKind::ToolApproval,
        status: InteractionStatus::Pending,
        scope: InteractionScope {
            session_id: String::new(),
            turn_id: turn_id.to_string(),
            item_id: Some(request.id.clone()),
            tool_id: Some(request.id.clone()),
            agent_path: request.parent_agent_id.clone(),
        },
        payload: InteractionPayload::ToolApproval {
            name: request.name.clone(),
            arguments: request.arguments.clone(),
            working_directory: request.working_directory.clone(),
            parent_agent_id: request.parent_agent_id.clone(),
        },
        created_at: now,
        updated_at: now,
        resolved_at: None,
        resolution: None,
    }
}

#[cfg(test)]
mod tests {
    use pl_model::ToolCall;
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
            None,
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
            None,
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
            "bash",
            serde_json::json!({ "command": "pwd", "workingDirectory": ".." }),
            None,
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
        let tool_call = ToolCall::custom("call-1", "apply_patch", patch, None);

        let access = requested_workspace_access(&tool_call, &root);

        assert_eq!(access, WorkspaceAccess::WorkspaceOnly);
        let _ = std::fs::remove_dir_all(root);
    }
}
