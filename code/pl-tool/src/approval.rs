//! Built-in tool access assessment. It describes access requirements without granting approval.
use crate::workspace::{PathAccess, ToolPathPolicy};

use std::path::Path;

/// Business effect classification interpreted by the host's approval policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolEffect {
    Read,
    WorkspaceWrite,
    Process,
    AgentControl,
    BranchControl,
}
/// Requested workspace path scope; this classification is not an execution grant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceAccess {
    #[default]
    WorkspaceOnly,
    ExternalAllowed,
}
impl WorkspaceAccess {
    pub fn allows_external(self) -> bool {
        matches!(self, Self::ExternalAllowed)
    }
}
/// Tool-owned path assessment for the host's approval policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolAccessAssessment {
    pub requested_access: WorkspaceAccess,
    pub working_directory: Option<String>,
}

/// Invocation capability issued by Studio after authorizing access beyond a host-permitted workspace.
pub const HOST_WORKSPACE_ACCESS: &str = "pl.tool.workspace.external";

/// Interprets built-in tool parameters for the Studio approval policy.
/// Invalid JSON requests conservative external access; typed execution still validates parameters.
pub fn assess(name: &str, raw: &str, root: &Path) -> ToolAccessAssessment {
    let Ok(arguments) = serde_json::from_str::<serde_json::Value>(raw) else {
        return ToolAccessAssessment {
            requested_access: WorkspaceAccess::ExternalAllowed,
            working_directory: None,
        };
    };
    ToolAccessAssessment {
        requested_access: requested_workspace_access(name, &arguments, root),
        working_directory: get_working_directory(&arguments),
    }
}

/// Classifies remote POSIX paths without touching the host filesystem.
/// Worker-side confinement remains authoritative for symlinks and actual resources.
pub fn assess_remote(name: &str, raw: &str, root: &Path) -> ToolAccessAssessment {
    let Ok(arguments) = serde_json::from_str::<serde_json::Value>(raw) else {
        return ToolAccessAssessment {
            requested_access: WorkspaceAccess::ExternalAllowed,
            working_directory: None,
        };
    };
    let working_directory = get_working_directory(&arguments);
    let external = requested_paths_for_tool(name, &arguments)
        .into_iter()
        .chain(working_directory.clone())
        .any(|path| {
            crate::remote::path::relative_workspace_path(&root.to_string_lossy(), Path::new(&path))
                .is_err()
        });
    ToolAccessAssessment {
        requested_access: if external {
            WorkspaceAccess::ExternalAllowed
        } else {
            WorkspaceAccess::WorkspaceOnly
        },
        working_directory,
    }
}

fn get_working_directory(arguments: &serde_json::Value) -> Option<String> {
    arguments
        .get("cwd")
        .or_else(|| arguments.get("workingDirectory"))
        .or_else(|| arguments.get("working_directory"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn requested_workspace_access(
    name: &str,
    arguments: &serde_json::Value,
    workspace_root: &Path,
) -> WorkspaceAccess {
    let paths = requested_paths_for_tool(name, arguments)
        .into_iter()
        .chain(get_working_directory(arguments))
        .collect::<Vec<_>>();
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

fn requested_paths_for_tool(name: &str, arguments: &serde_json::Value) -> Vec<String> {
    if name.starts_with("lsp_query_") {
        return argument_path(arguments, "filePath").into_iter().collect();
    }
    match name {
        "exec" => get_working_directory(arguments).into_iter().collect(),
        "write_stdin" => Vec::new(),
        "read_file" | "write_file" | "stat_path" | "view_image" | "create_directory"
        | "delete_path" => argument_path(arguments, "path").into_iter().collect(),
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

fn argument_path(arguments: &serde_json::Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn paths_from_patch_text(patch: &str) -> Vec<String> {
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

fn path_requires_external_access(path: &str, policy: &ToolPathPolicy) -> bool {
    policy.access_for_input(path) == PathAccess::External
}
