use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[derive(Debug, Clone)]
struct DisplayContainerError(&'static str);

impl std::fmt::Display for DisplayContainerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
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
    type Error = DisplayContainerError;

    async fn exec(
        &self,
        request: ContainerExecRequest,
    ) -> std::result::Result<ContainerExecOutput, Self::Error> {
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

    async fn copy_from(
        &self,
        request: ContainerCopyFromRequest,
    ) -> std::result::Result<Vec<u8>, Self::Error> {
        Ok(self
            .files
            .lock()
            .unwrap()
            .get(&request.path)
            .cloned()
            .unwrap_or_default()
            .into_bytes())
    }

    async fn copy_to(
        &self,
        request: ContainerCopyToRequest,
    ) -> std::result::Result<(), Self::Error> {
        self.files.lock().unwrap().insert(
            request.path,
            String::from_utf8(request.content).expect("utf8 content"),
        );
        Ok(())
    }
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
        "startLine": 2,
        "maxLines": 1,
    });

    let local_output = crate::tool::execute_workspace_file_tool(
        &local,
        crate::tool::WorkspaceFileToolKind::ReadFile.name(),
        input.clone(),
        None,
        0,
    )
    .await
    .unwrap()
    .unwrap();
    let container_output = crate::tool::execute_workspace_file_tool(
        &container,
        crate::tool::WorkspaceFileToolKind::ReadFile.name(),
        input,
        None,
        0,
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
        0,
    )
    .await
    .unwrap()
    .unwrap();
    let container_output = crate::tool::execute_workspace_file_tool(
        &container,
        crate::tool::WorkspaceFileToolKind::ApplyPatch.name(),
        input,
        None,
        0,
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(local_output.output, container_output.output);
    let _ = tokio::fs::remove_dir_all(root).await;
}
