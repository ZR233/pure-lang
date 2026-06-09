use std::path::{Path, PathBuf};

use pl_protocol::{AgentEvent, AgentEventSender};

#[cfg(test)]
use crate::permission::{PermissionDecision, decide_tool_permission};
use crate::tool::{ToolContext, WorkspaceAccess};
use crate::turn::{ToolApprovalDecision, ToolApprovalRequest, TurnOptions};

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
    let Some(root) = canonical_workspace_root(workspace_root) else {
        return WorkspaceAccess::ExternalAllowed;
    };
    if paths
        .iter()
        .any(|path| path_requires_external_access(path, &root))
    {
        WorkspaceAccess::ExternalAllowed
    } else {
        WorkspaceAccess::WorkspaceOnly
    }
}

pub(super) fn requested_paths_for_tool(name: &str, arguments: &serde_json::Value) -> Vec<String> {
    match name {
        "bash" => get_working_directory(arguments).into_iter().collect(),
        "write_stdin" => Vec::new(),
        "read_file" | "write_file" | "stat_path" | "create_directory" | "delete_path" => {
            argument_path(arguments, "path").into_iter().collect()
        }
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

pub(super) fn canonical_workspace_root(workspace_root: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(workspace_root).ok()
}

pub(super) fn path_requires_external_access(path: &str, workspace_root: &Path) -> bool {
    let path = Path::new(path.trim());
    if path.as_os_str().is_empty() {
        return false;
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return true;
    }
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    let resolved = canonicalize_existing_or_parent(&candidate);
    !resolved.starts_with(workspace_root)
}

pub(super) fn canonicalize_existing_or_parent(candidate: &Path) -> PathBuf {
    let mut current = candidate.to_path_buf();
    loop {
        if current.exists()
            && let Ok(canonical) = std::fs::canonicalize(&current)
        {
            return canonical;
        }
        let Some(parent) = current.parent() else {
            return candidate.to_path_buf();
        };
        if parent == current {
            return candidate.to_path_buf();
        }
        current = parent.to_path_buf();
    }
}

pub(super) fn permission_risk_summary(tool_name: &str) -> &'static str {
    if crate::mcp::is_mcp_tool_name(tool_name) {
        return "trusted MCP server tool";
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
            request_user_approval(options, request, event_tx).await
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
) -> ToolApprovalDecision {
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
