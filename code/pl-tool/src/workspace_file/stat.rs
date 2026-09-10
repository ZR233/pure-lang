//! Path metadata projection shared by local and remote Thread tools.
use super::{WorkspaceFileBackend, WorkspaceFileStatRequest};
use pl_core::{
    context::{ContextContent, OpaquePayload},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolAuthorization, ToolError},
    },
};
use std::sync::Arc;

/// A Thread-owned metadata reader with frozen backend access authority.
#[derive(Debug)]
pub struct ThreadStatPathTool<B> {
    backend: Arc<B>,
    authorization: ToolAuthorization,
}

impl<B: WorkspaceFileBackend + 'static> ThreadStatPathTool<B> {
    /// Binds the operation to the host-selected workspace lease.
    pub fn new(backend: Arc<B>, authorization: ToolAuthorization) -> Self {
        Self {
            backend,
            authorization,
        }
    }

    /// Transfers the reader into the Thread tool manager.
    ///
    /// # Errors
    /// Returns invalid registration identity errors.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        let authorization = self.authorization.clone();
        Ok(Registration::new("stat_path".into(), declaration, self)?
            .with_authorization(authorization))
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    path: String,
    exists: bool,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    len: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    readonly: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    modified_at: Option<i64>,
}

impl<B: WorkspaceFileBackend + 'static> Tool for ThreadStatPathTool<B> {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(crate::tool_error(
                "stat_path",
                "unsupported argument encoding",
            )));
        }
        let input: crate::file::PathInput =
            serde_json::from_str(input.content()).map_err(ToolError::new)?;
        let granted = self.backend.for_grant(&context.grant);
        let backend = granted.as_ref().unwrap_or(self.backend.as_ref());
        let stat = backend
            .stat_optional(WorkspaceFileStatRequest {
                path: input.path.clone(),
                cwd: None,
            })
            .await
            .map_err(ToolError::new)?;
        let receipt = match stat {
            Some(stat) => Receipt {
                path: stat.path,
                exists: true,
                kind: Some(if stat.is_file {
                    "file"
                } else if stat.is_dir {
                    "directory"
                } else {
                    "other"
                }),
                len: stat.len,
                readonly: stat.readonly,
                modified_at: stat.modified_at,
            },
            None => Receipt {
                path: input.path,
                exists: false,
                kind: None,
                len: None,
                readonly: None,
                modified_at: None,
            },
        };
        let content = serde_json::to_string(&receipt).map_err(ToolError::new)?;
        Ok(ToolOutput::new(
            OpaquePayload::new("pl.tool.path-stat", 1, content.clone()).map_err(ToolError::new)?,
            vec![ContextContent::Text {
                text: content.into(),
            }],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn missing_path_is_a_fact_but_workspace_escape_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::ToolWorkspace::new(
            crate::workspace::AgentWorkspace::local(root.path()),
        );
        let backend = Arc::new(
            super::super::LocalWorkspaceFileBackend::confined(workspace.clone())
                .await
                .unwrap(),
        );
        let tool = ThreadStatPathTool::new(backend, workspace.authorization());
        let input = |path: &str| {
            OpaquePayload::new(
                "application/json",
                1,
                serde_json::json!({"path":path}).to_string(),
            )
            .unwrap()
        };
        let result = tool
            .execute(input("absent"), crate::test_support::thread_context())
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(result.payload().content()).unwrap(),
            serde_json::json!({"path":"absent","exists":false})
        );
        assert!(
            tool.execute(input("../outside"), crate::test_support::thread_context())
                .await
                .is_err()
        );
        tokio::fs::write(root.path().join("present"), "hello")
            .await
            .unwrap();
        let result = tool
            .execute(input("present"), crate::test_support::thread_context())
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(result.payload().content()).unwrap();
        assert_eq!(payload["len"], 5);
        assert_eq!(payload["type"], "file");
        assert_eq!(
            result.context(),
            &[ContextContent::Text {
                text: result.payload().content().into()
            }]
        );
    }
}
