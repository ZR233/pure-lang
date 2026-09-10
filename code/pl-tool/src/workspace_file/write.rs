//! One dynamic write-file contract across local and worker-backed workspaces.
use super::{WorkspaceFileBackend, WorkspaceFileWriteRequest};
use crate::{file::WriteFileInput, workspace::ToolWorkspace};
use pl_core::{
    context::{ContextContent, OpaquePayload},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use std::sync::Arc;

/// Thread-owned write operation; physical path validation and file mutation belong to its backend.
#[derive(Debug)]
pub struct ThreadWriteFileTool<B> {
    backend: Arc<B>,
    workspace: ToolWorkspace,
}

impl<B: WorkspaceFileBackend + 'static> ThreadWriteFileTool<B> {
    /// Binds a backend to its matching immutable workspace policy and write lease.
    /// The backend must enforce that policy, as the local confined and workspace-policy backends do.
    pub fn new(backend: Arc<B>, workspace: ToolWorkspace) -> Self {
        Self { backend, workspace }
    }

    /// Transfers a foreground writer into the Thread's manager.
    ///
    /// # Errors
    /// Returns invalid tool registration identity errors.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        let authorization = self.workspace.authorization();
        Ok(Registration::new("write_file".into(), declaration, self)?
            .foreground_coexisting()
            .with_authorization(authorization))
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    path: String,
    mode: crate::workspace::WriteMode,
    bytes_written: usize,
}

impl<B: WorkspaceFileBackend + 'static> Tool for ThreadWriteFileTool<B> {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(crate::tool_error(
                "write_file",
                "unsupported argument encoding",
            )));
        }
        let input: WriteFileInput =
            serde_json::from_str(input.content()).map_err(ToolError::new)?;
        self.workspace
            .ensure_workspace_writable()
            .map_err(ToolError::new)?;
        let _lease = tokio::select! {
            biased;
            _ = context.cancellation.cancelled() => return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled)),
            lease = self.workspace.write_lock() => lease,
        };
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let granted = self.backend.for_grant(&context.grant);
        let backend = granted.as_ref().unwrap_or(self.backend.as_ref());
        let receipt = Receipt {
            path: input.path.clone(),
            mode: input.mode,
            bytes_written: input.content.len(),
        };
        backend
            .write_text(WorkspaceFileWriteRequest {
                mode: input.mode,
                path: input.path,
                cwd: None,
                content: input.content,
            })
            .await
            .map_err(ToolError::new)?;
        let payload = OpaquePayload::new(
            "pl.tool.file-write",
            1,
            serde_json::to_string(&receipt).map_err(ToolError::new)?,
        )
        .map_err(ToolError::new)?;
        Ok(ToolOutput::new(
            payload,
            vec![ContextContent::Text {
                text: Arc::from(format!(
                    "Wrote {} bytes to {}",
                    receipt.bytes_written, receipt.path
                )),
            }],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    #[tokio::test]
    async fn dynamic_write_preserves_utf8_bytes_and_create_never_overwrites_an_existing_file() {
        use pl_core::tool::opaque::Tool;
        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::ToolWorkspace::new(
            crate::workspace::AgentWorkspace::local(directory.path()),
        );
        let backend = super::super::LocalWorkspaceFileBackend::confined(workspace.clone())
            .await
            .unwrap();
        let tool = ThreadWriteFileTool::new(Arc::new(backend), workspace);
        let arguments = |mode: &str, content: &str| {
            pl_core::context::OpaquePayload::new(
                "application/json",
                1,
                serde_json::json!({"path":"note.txt", "mode":mode, "content":content}).to_string(),
            )
            .unwrap()
        };
        let original = "  原文\r\n";
        let output = Tool::execute(
            &tool,
            arguments("create", original),
            crate::test_support::thread_context(),
        )
        .await
        .unwrap();
        let payload: serde_json::Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({"path":"note.txt", "mode":"create", "bytesWritten":original.len()})
        );
        assert!(
            Tool::execute(
                &tool,
                arguments("create", "replacement"),
                crate::test_support::thread_context()
            )
            .await
            .is_err()
        );
        assert_eq!(
            tokio::fs::read(directory.path().join("note.txt"))
                .await
                .unwrap(),
            original.as_bytes()
        );
        Tool::execute(
            &tool,
            arguments("append", "\0尾\n"),
            crate::test_support::thread_context(),
        )
        .await
        .unwrap();
        assert_eq!(
            tokio::fs::read(directory.path().join("note.txt"))
                .await
                .unwrap(),
            format!("{original}\0尾\n").as_bytes()
        );
        Tool::execute(
            &tool,
            arguments("overwrite", "new\r\n"),
            crate::test_support::thread_context(),
        )
        .await
        .unwrap();
        assert_eq!(
            tokio::fs::read(directory.path().join("note.txt"))
                .await
                .unwrap(),
            b"new\r\n"
        );
    }

    #[tokio::test]
    async fn cancelled_dynamic_write_does_not_wait_for_another_writer_or_create_the_file() {
        use pl_core::tool::opaque::Tool;
        let directory = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::ToolWorkspace::new(
            crate::workspace::AgentWorkspace::local(directory.path()),
        );
        let guard = workspace.write_lock().await;
        let backend = super::super::LocalWorkspaceFileBackend::confined(workspace.clone())
            .await
            .unwrap();
        let tool = ThreadWriteFileTool::new(Arc::new(backend), workspace);
        let context = crate::test_support::thread_context();
        let cancellation = context.cancellation.clone();
        let arguments = pl_core::context::OpaquePayload::new("application/json", 1,
            serde_json::json!({"path":"cancelled.txt", "mode":"create", "content":"must not write"}).to_string()).unwrap();
        let operation = Tool::execute(&tool, arguments, context);
        tokio::pin!(operation);
        assert!(futures::poll!(&mut operation).is_pending());
        cancellation.cancel();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), operation)
            .await
            .expect("cancelled writer stops while the lock is held");
        assert!(result.is_err());
        assert!(!directory.path().join("cancelled.txt").exists());
        drop(guard);
    }
}
