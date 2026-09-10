//! Opaque Thread tools over the existing workspace file and patch implementations.
use super::*;
use pl_core::{
    context::{ContextContent, OpaquePayload},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};

/// One workspace operation; backend policy and resource leases are supplied by the host.
#[derive(Debug)]
pub struct ThreadWorkspaceFileTool<B: WorkspaceFileBackend> {
    kind: WorkspaceFileToolKind,
    backend: Arc<B>,
    workspace: ToolWorkspace,
}

impl<B: WorkspaceFileBackend + 'static> ThreadWorkspaceFileTool<B> {
    /// Creates a fresh operation instance over the host-selected backend lease.
    pub fn new(kind: WorkspaceFileToolKind, backend: Arc<B>, workspace: ToolWorkspace) -> Self {
        Self {
            kind,
            backend,
            workspace,
        }
    }

    /// Transfers this tool with deterministic host-encoded declaration material.
    ///
    /// # Errors
    /// Returns invalid registration identity.
    pub fn registration(
        self,
        declaration: OpaquePayload,
    ) -> std::result::Result<Registration, RegistryError> {
        let mutation = self.kind == WorkspaceFileToolKind::ApplyPatch;
        let authorization = self.workspace.authorization();
        let registration = Registration::new(self.kind.name().into(), declaration, self)?
            .with_authorization(authorization);
        Ok(if mutation {
            registration.foreground_coexisting()
        } else {
            registration
        })
    }
}

impl<B: WorkspaceFileBackend + 'static> Tool for ThreadWorkspaceFileTool<B> {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> std::result::Result<ToolOutput, ToolError> {
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let input = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        let _write_guard = if self.kind == WorkspaceFileToolKind::ApplyPatch {
            self.workspace
                .ensure_workspace_writable()
                .map_err(ToolError::new)?;
            Some(self.workspace.write_lock().await)
        } else {
            None
        };
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let granted = self.backend.for_grant(&context.grant);
        let backend = granted.as_ref().unwrap_or(self.backend.as_ref());
        let result = execute_workspace_file_tool(backend, self.kind.name(), input)
            .await
            .map_err(ToolError::new)?
            .ok_or_else(|| {
                ToolError::new(tool_error(
                    self.kind.name(),
                    "unsupported workspace operation",
                ))
            })?;
        // Projection was produced at execution time. Historical readers never rerun it.
        let output = ToolOutput::new(
            OpaquePayload::new("pl.tool.workspace-file", 1, result.output)
                .map_err(ToolError::new)?,
            vec![ContextContent::Text {
                text: Arc::from(result.model_output),
            }],
        );
        if result.success {
            Ok(output)
        } else {
            Err(
                ToolError::new(tool_error(self.kind.name(), "workspace operation failed"))
                    .with_output(output),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn context() -> CallContext {
        CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: tokio_util::sync::CancellationToken::new(),
            extensions: Arc::new(Default::default()),
            catalog: Vec::new().into(),
            extension_sequence: 0,
        }
    }

    #[tokio::test]
    async fn opaque_read_preserves_bytes_and_patch_respects_frozen_directory_policy() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("note.txt");
        let original = "你好\r\noriginal\r\n";
        tokio::fs::write(&path, original).await.unwrap();
        let workspace = ToolWorkspace::new(crate::workspace::AgentWorkspace::directory(
            root.path(),
            Some(Vec::new()),
        ));
        let backend = Arc::new(
            LocalWorkspaceFileBackend::confined(workspace.clone())
                .await
                .unwrap(),
        );
        let read = ThreadWorkspaceFileTool::new(
            WorkspaceFileToolKind::ReadFile,
            backend.clone(),
            workspace.clone(),
        );
        let output = Tool::execute(
            &read,
            OpaquePayload::text(r#"{"path":"note.txt"}"#),
            context(),
        )
        .await
        .unwrap();
        let payload: serde_json::Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(payload["text"], original);
        let patch =
            ThreadWorkspaceFileTool::new(WorkspaceFileToolKind::ApplyPatch, backend, workspace);
        let input = serde_json::json!({"input":"*** Begin Patch\n*** Update File: note.txt\n@@\n-original\n+changed\n*** End Patch"});
        assert!(
            Tool::execute(&patch, OpaquePayload::text(input.to_string()), context())
                .await
                .is_err()
        );
        assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), original);
    }
}
