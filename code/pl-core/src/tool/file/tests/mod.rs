use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::tool::{Tool, ToolContext, ToolInput};
use crate::turn::TurnOptions;

use super::read::{ReadFileTool, SearchFilesTool};
use super::write::WriteFileTool;
use super::{ApplyPatchTool, CopyPathTool, DeletePathTool, MovePathTool};

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
        mode: crate::turn::CompileMode::Auto,
        workspace_root: root.to_path_buf(),
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        agent_supervisor: crate::AgentSupervisor::default(),
        lsp_runtime: None,
        parent_session: std::sync::Arc::new(crate::CoreSession::new()),
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

mod apply_patch;
mod ops;
mod read;
