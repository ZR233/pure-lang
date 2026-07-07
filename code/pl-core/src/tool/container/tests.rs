use std::sync::{Arc, Mutex};

use pl_protocol::Result;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::tool::{Tool, ToolContext, ToolInput, WorkspaceAccess};
use crate::{AgentSupervisor, CompileMode, CoreSession, TurnOptions};

use super::*;

#[derive(Debug, Default)]
struct RecordingBackend {
    copied_to: Arc<Mutex<Option<ContainerCopyToRequest>>>,
}

impl ContainerBackend for RecordingBackend {
    async fn exec(&self, _request: ContainerExecRequest) -> Result<ContainerExecOutput> {
        Ok(ContainerExecOutput {
            status: 0,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            stdout_bytes: 0,
            stderr_bytes: 0,
            output_artifacts: Vec::new(),
        })
    }

    async fn copy_from(&self, _request: ContainerCopyFromRequest) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }

    async fn copy_to(&self, request: ContainerCopyToRequest) -> Result<()> {
        *self.copied_to.lock().unwrap() = Some(request);
        Ok(())
    }
}

#[derive(Debug)]
struct FailingExecBackend;

impl ContainerBackend for FailingExecBackend {
    async fn exec(&self, _request: ContainerExecRequest) -> Result<ContainerExecOutput> {
        Ok(ContainerExecOutput {
            status: 7,
            stdout: "visible stdout".to_string(),
            stderr: "visible stderr".to_string(),
            stdout_truncated: true,
            stderr_truncated: false,
            stdout_bytes: 4096,
            stderr_bytes: 14,
            output_artifacts: Vec::new(),
        })
    }

    async fn copy_from(&self, _request: ContainerCopyFromRequest) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }

    async fn copy_to(&self, _request: ContainerCopyToRequest) -> Result<()> {
        Ok(())
    }
}

async fn tool_context() -> ToolContext {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    ToolContext {
        event_tx,
        options: TurnOptions::default(),
        workspace_access: WorkspaceAccess::WorkspaceOnly,
        mode: CompileMode::Auto,
        workspace_root: std::env::temp_dir(),
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        agent_supervisor: AgentSupervisor::default(),
        agent_tool_registrar: None,
        lsp_runtime: None,
        parent_session: Arc::new(CoreSession::new()),
    }
}

#[tokio::test]
async fn registered_container_tool_preserves_exec_metadata() {
    let tool = ContainerTool::new(ContainerToolKind::Exec, Arc::new(FailingExecBackend));

    let output = tool
        .execute(
            ToolInput {
                arguments: json!({ "command": "false" }),
                session_id: "session".to_string(),
                tool_id: "tool".to_string(),
                revision_base: 0,
            },
            tool_context().await,
        )
        .await
        .expect("execute");

    assert_eq!(output.exit_code, Some(7));
    assert!(output.truncated.stdout.was_truncated);
    assert_eq!(output.truncated.stdout.original_length, 4096);
    assert_eq!(output.truncated.stdout.content, "visible stdout");
    assert_eq!(output.truncated.stderr.content, "visible stderr");
}

#[tokio::test]
async fn upload_trims_base64_payload() {
    let backend = RecordingBackend::default();

    let result = execute_container_tool(
        &backend,
        TOOL_CONTAINER_CP_UPLOAD,
        json!({
            "path": "/tmp/hello.txt",
            "content_base64": " aGVsbG8= \n",
        }),
        None,
    )
    .await
    .expect("execute")
    .expect("handled");

    assert!(result.success);
    let copied = backend.copied_to.lock().unwrap().clone().expect("copy");
    assert_eq!(copied.path, "/tmp/hello.txt");
    assert_eq!(copied.content, b"hello");
}
