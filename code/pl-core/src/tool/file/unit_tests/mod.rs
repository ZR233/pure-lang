use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::tool::{
    AgentWorkspace, LocalWorkspaceFileTool, StatPathTool, Tool, ToolCallContext, ToolInput,
    ToolWorkspace, WorkspaceFileToolKind,
};

use super::write::WriteFileTool;
use super::{CopyPathTool, DeletePathTool, MovePathTool};

fn unique_temp_dir(name: &str) -> PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-lang-{name}-{id}"))
}

async fn context(root: &Path) -> ToolCallContext {
    tokio::fs::create_dir_all(root).await.unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    ToolCallContext::test(event_tx)
}

fn tool_workspace(root: &Path) -> ToolWorkspace {
    ToolWorkspace::new(AgentWorkspace::local(root.to_path_buf()))
}

fn input(arguments: serde_json::Value) -> ToolInput {
    ToolInput { arguments }
}

fn read_file_tool(root: &Path) -> LocalWorkspaceFileTool {
    LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ReadFile, tool_workspace(root))
}

fn list_files_tool(root: &Path) -> LocalWorkspaceFileTool {
    LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ListFiles, tool_workspace(root))
}

fn apply_patch_tool(root: &Path) -> LocalWorkspaceFileTool {
    LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ApplyPatch, tool_workspace(root))
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
