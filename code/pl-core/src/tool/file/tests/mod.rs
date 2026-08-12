use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::tool::{LocalWorkspaceFileTool, Tool, ToolContext, ToolInput, WorkspaceFileToolKind};
use crate::turn::TurnOptions;

use super::write::WriteFileTool;
use super::{CopyPathTool, DeletePathTool, MovePathTool};

fn unique_temp_dir(name: &str) -> PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-lang-{name}-{id}"))
}

async fn context(root: &Path) -> ToolContext {
    tokio::fs::create_dir_all(root).await.unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    ToolContext {
        event_tx,
        options: TurnOptions::default(),
        workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
        workspace: crate::tool::AgentWorkspace::local(root.to_path_buf()),
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        lsp_runtime: None,
        parent_session: std::sync::Arc::new(crate::AgentSession::new()),
        working_set: crate::TurnWorkingSetHandle::default(),
        tool_cache: crate::TurnToolCacheHandle::default(),
    }
}

fn input(arguments: serde_json::Value) -> ToolInput {
    ToolInput {
        arguments,
        session_id: "session".to_string(),
        tool_id: "tool".to_string(),
        revision_base: 0,
    }
}

fn read_file_tool() -> LocalWorkspaceFileTool {
    LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ReadFile)
}

fn list_files_tool() -> LocalWorkspaceFileTool {
    LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ListFiles)
}

fn apply_patch_tool() -> LocalWorkspaceFileTool {
    LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ApplyPatch)
}

#[cfg(unix)]
fn create_directory_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn remove_directory_symlink(link: &Path) -> std::io::Result<()> {
    std::fs::remove_file(link)
}

#[cfg(windows)]
fn remove_directory_symlink(link: &Path) -> std::io::Result<()> {
    std::fs::remove_dir(link)
}

mod apply_patch;
mod ops;
mod read;
