use std::collections::HashMap;
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

#[derive(Debug, Default)]
struct FakeWorkspaceContainerBackend {
    files: Arc<Mutex<HashMap<String, String>>>,
}

impl FakeWorkspaceContainerBackend {
    fn with_file(path: &str, content: &str) -> Self {
        let backend = Self::default();
        backend
            .files
            .lock()
            .unwrap()
            .insert(path.to_string(), content.to_string());
        backend
    }
}

impl ContainerBackend for FakeWorkspaceContainerBackend {
    async fn exec(&self, request: ContainerExecRequest) -> Result<ContainerExecOutput> {
        let mut status = 0;
        let mut stdout = String::new();
        if request.command.contains("wc -c") {
            let path = request
                .command
                .split("test -f ")
                .nth(1)
                .and_then(|rest| rest.split(';').next())
                .unwrap_or_default()
                .trim_matches('\'')
                .to_string();
            let files = self.files.lock().unwrap();
            if let Some(content) = files.get(&path) {
                stdout = format!("file\t{}", content.len());
            } else {
                stdout = "missing\t0".to_string();
            }
        } else if request.command.starts_with("rm -f -- ") {
            let path = request
                .command
                .trim_start_matches("rm -f -- ")
                .trim_matches('\'')
                .to_string();
            self.files.lock().unwrap().remove(&path);
        } else {
            status = 127;
        }
        Ok(ContainerExecOutput {
            status,
            stdout,
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            stdout_bytes: 0,
            stderr_bytes: 0,
            output_artifacts: Vec::new(),
        })
    }

    async fn copy_from(&self, request: ContainerCopyFromRequest) -> Result<Vec<u8>> {
        Ok(self
            .files
            .lock()
            .unwrap()
            .get(&request.path)
            .cloned()
            .unwrap_or_default()
            .into_bytes())
    }

    async fn copy_to(&self, request: ContainerCopyToRequest) -> Result<()> {
        self.files.lock().unwrap().insert(
            request.path,
            String::from_utf8(request.content).expect("utf8 content"),
        );
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

#[test]
fn container_tool_kind_rejects_dot_aliases() {
    assert_eq!(
        ContainerToolKind::from_name("container_exec"),
        Some(ContainerToolKind::Exec)
    );
    assert_eq!(
        ContainerToolKind::from_name("container_copy"),
        Some(ContainerToolKind::Copy)
    );
    assert_eq!(ContainerToolKind::from_name("container.exec"), None);
    assert_eq!(ContainerToolKind::from_name("container.copy"), None);
}

#[tokio::test]
async fn upload_trims_base64_payload() {
    let backend = RecordingBackend::default();

    let result = execute_container_tool(
        &backend,
        TOOL_CONTAINER_COPY,
        json!({
            "direction": "upload",
            "path": "/tmp/hello.txt",
            "contentBase64": " aGVsbG8= \n",
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

#[tokio::test]
async fn workspace_file_read_has_same_json_shape_for_local_and_container_backends() {
    let root = std::env::temp_dir().join(format!(
        "pure-workspace-file-read-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\n")
        .await
        .unwrap();
    let local = crate::tool::LocalWorkspaceFileBackend::new(root.clone(), false)
        .await
        .unwrap();
    let container = crate::tool::ContainerWorkspaceFileBackend::new(Arc::new(
        FakeWorkspaceContainerBackend::with_file("a.txt", "one\ntwo\n"),
    ));
    let input = json!({
        "path": "a.txt",
        "lineStart": 2,
        "lineCount": 1,
    });

    let local_output = crate::tool::execute_workspace_file_tool(
        &local,
        crate::tool::WorkspaceFileToolKind::ReadFile.name(),
        input.clone(),
        None,
    )
    .await
    .unwrap()
    .unwrap();
    let container_output = crate::tool::execute_workspace_file_tool(
        &container,
        crate::tool::WorkspaceFileToolKind::ReadFile.name(),
        input,
        None,
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(local_output.output, container_output.output);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn workspace_file_apply_patch_has_same_json_shape_for_local_and_container_backends() {
    let root = std::env::temp_dir().join(format!(
        "pure-workspace-file-patch-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let local = crate::tool::LocalWorkspaceFileBackend::new(root.clone(), false)
        .await
        .unwrap();
    let container = crate::tool::ContainerWorkspaceFileBackend::new(Arc::new(
        FakeWorkspaceContainerBackend::default(),
    ));
    let input = json!({
        "cwd": ".",
        "input": "*** Begin Patch\n*** Add File: src/lib.rs\n+pub fn ok() {}\n*** End Patch"
    });

    let local_output = crate::tool::execute_workspace_file_tool(
        &local,
        crate::tool::WorkspaceFileToolKind::ApplyPatch.name(),
        input.clone(),
        None,
    )
    .await
    .unwrap()
    .unwrap();
    let container_output = crate::tool::execute_workspace_file_tool(
        &container,
        crate::tool::WorkspaceFileToolKind::ApplyPatch.name(),
        input,
        None,
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(local_output.output, container_output.output);
    let _ = tokio::fs::remove_dir_all(root).await;
}
