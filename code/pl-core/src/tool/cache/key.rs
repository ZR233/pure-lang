use std::path::Path;

use serde_json::Value;

use super::ToolCachePolicy;

pub(super) fn cache_key(
    tool_name: &str,
    arguments: &Value,
    workspace_root: &Path,
    policy: ToolCachePolicy,
    workspace_epoch: u64,
) -> String {
    let canonical_arguments = crate::working_set::canonical_json_string(arguments);
    let repository_view = repository_view(arguments);
    let epoch = effective_epoch(policy, repository_view, workspace_epoch);
    crate::working_set::canonical_content_hash(
        format!(
            "{tool_name}\0{}\0{}\0{repository_view:?}\0{epoch}",
            workspace_root.display(),
            canonical_arguments
        )
        .as_bytes(),
    )
}

pub(super) fn effective_epoch(
    policy: ToolCachePolicy,
    repository_view: RepositoryView,
    workspace_epoch: u64,
) -> u64 {
    match (policy, repository_view) {
        (ToolCachePolicy::UntilWorkspaceMutation, RepositoryView::Workspace) => workspace_epoch,
        (ToolCachePolicy::Never, _)
        | (ToolCachePolicy::WithinTurn, _)
        | (ToolCachePolicy::UntilWorkspaceMutation, RepositoryView::Project) => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RepositoryView {
    Project,
    Workspace,
}

pub(super) fn repository_view(arguments: &Value) -> RepositoryView {
    if contains_project_path(arguments) {
        RepositoryView::Project
    } else {
        RepositoryView::Workspace
    }
}

fn contains_project_path(value: &Value) -> bool {
    match value {
        Value::String(value) => value == "/project/repo" || value.starts_with("/project/repo/"),
        Value::Array(items) => items.iter().any(contains_project_path),
        Value::Object(map) => map.values().any(contains_project_path),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}
