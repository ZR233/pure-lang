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
