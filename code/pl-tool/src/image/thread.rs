//! Workspace images delivered through immutable resources, without product attachment state.
use super::{MAX_SOURCE_BYTES, TOOL_VIEW_IMAGE, ViewImageInput, normalize_tool_image};
use crate::{
    media::{PreparedToolImage, ToolMedia, ToolMediaHost, ToolMediaKind, image_limits},
    workspace_file::{
        WorkspaceFileBackend, WorkspaceFileReadBytesRequest, WorkspaceFileStatRequest,
    },
};
use pl_core::{
    context::OpaquePayload,
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolAuthorization, ToolError},
    },
};
use std::sync::Arc;

/// A Thread-local reader over its configured backend and persistent resource host.
#[derive(Debug)]
pub struct ThreadViewImageTool<B, H> {
    backend: Arc<B>,
    media: Arc<H>,
    authorization: ToolAuthorization,
}

impl<B: WorkspaceFileBackend + 'static, H: ToolMediaHost> ThreadViewImageTool<B, H> {
    /// Binds physical reads and authorization to the same Thread installation.
    pub fn new(backend: Arc<B>, authorization: ToolAuthorization, media: Arc<H>) -> Self {
        Self {
            backend,
            authorization,
            media,
        }
    }

    /// Installs a deferred image reader without framework control permissions.
    ///
    /// # Errors
    /// Returns invalid registry identity errors.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        let authorization = self.authorization.clone();
        Ok(
            Registration::new(TOOL_VIEW_IMAGE.into(), declaration, self)?
                .deferred()
                .with_authorization(authorization),
        )
    }
}

impl<B: WorkspaceFileBackend + 'static, H: ToolMediaHost> Tool for ThreadViewImageTool<B, H> {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(failure("unsupported image argument encoding"));
        }
        let input: ViewImageInput =
            serde_json::from_str(input.content()).map_err(ToolError::new)?;
        let limits = image_limits(context.model_projection.as_ref())?
            .filter(|limits| limits.max_count != Some(0))
            .ok_or_else(|| failure("prepared model does not support image input"))?;
        if context.cancellation.is_cancelled() {
            return Err(failure("image read cancelled"));
        }
        let granted = self.backend.for_grant(&context.grant);
        let backend = granted.as_ref().unwrap_or(self.backend.as_ref());
        let stat = backend
            .stat(WorkspaceFileStatRequest {
                path: input.path.clone(),
                cwd: None,
            })
            .await
            .map_err(ToolError::new)?;
        if !stat.is_file {
            return Err(failure("image source is not a regular file"));
        }
        let bytes: Arc<[u8]> = backend
            .read_bytes(WorkspaceFileReadBytesRequest {
                path: input.path.clone(),
                cwd: None,
                max_bytes: MAX_SOURCE_BYTES,
            })
            .await
            .map_err(ToolError::new)?
            .into();
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(failure("image source exceeds the byte limit"));
        }
        if context.cancellation.is_cancelled() {
            return Err(failure("image read cancelled"));
        }
        let original = bytes.clone();
        let total_limit = limits.max_total_bytes;
        let (source_type, normalized) = tokio::task::spawn_blocking(move || {
            let format = image::guess_format(&original).map_err(ToolError::new)?;
            let source_type =
                super::media_type(format).ok_or_else(|| failure("unsupported image header"))?;
            let normalized =
                normalize_tool_image(TOOL_VIEW_IMAGE, original.to_vec(), None, &limits)
                    .map_err(ToolError::new)?;
            Ok::<_, ToolError>((source_type, normalized))
        })
        .await
        .map_err(ToolError::new)??;
        if total_limit.is_some_and(|limit| normalized.bytes.len() as u64 > limit) {
            return Err(failure("image exceeds the prepared total-byte limit"));
        }
        if context.cancellation.is_cancelled() {
            return Err(failure("image preparation cancelled"));
        }
        let dimensions = (normalized.width, normalized.height);
        let retained = self
            .media
            .retain(ToolMedia {
                kind: ToolMediaKind::Image,
                bytes: bytes.clone(),
                media_type: source_type.into(),
                model_image: Some(PreparedToolImage {
                    bytes: normalized.bytes.into(),
                    media_type: normalized.media_type,
                }),
            })
            .await?;
        retained.reference.verify(&bytes).map_err(ToolError::new)?;
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Receipt {
            path: String,
            original: pl_core::context::ResourceReference,
            model_width: u32,
            model_height: u32,
        }
        let payload = OpaquePayload::new(
            "pl.tool.image",
            1,
            serde_json::to_string(&Receipt {
                path: input.path,
                original: retained.reference,
                model_width: dimensions.0,
                model_height: dimensions.1,
            })
            .map_err(ToolError::new)?,
        )
        .map_err(ToolError::new)?;
        Ok(ToolOutput::new(payload, retained.context))
    }
}

fn failure(message: &str) -> ToolError {
    ToolError::new(crate::tool_error(TOOL_VIEW_IMAGE, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_support::{MemoryMedia, thread_context},
        workspace::{AgentWorkspace, ToolWorkspace, WorkspaceMutability},
        workspace_file::LocalWorkspaceFileBackend,
    };
    use pretty_assertions::assert_eq;

    fn projected_context() -> CallContext {
        let mut context = thread_context();
        context.model_projection = Some(
            OpaquePayload::new(
                pl_protocol::tool_projection::FORMAT,
                pl_protocol::tool_projection::VERSION,
                serde_json::to_string(&pl_protocol::tool_projection::ToolProjection {
                    image: Some(pl_protocol::tool_projection::ImageProjection {
                        max_width: Some(1),
                        max_height: Some(1),
                        ..Default::default()
                    }),
                })
                .unwrap(),
            )
            .unwrap(),
        );
        context
    }

    #[tokio::test]
    async fn dynamic_image_uses_magic_bytes_and_retains_original_and_prepared_variant() {
        let directory = tempfile::tempdir().unwrap();
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(4, 4)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let original = encoded.into_inner();
        tokio::fs::write(directory.path().join("misleading.txt"), &original)
            .await
            .unwrap();
        let workspace = ToolWorkspace::new(AgentWorkspace::confined(
            directory.path(),
            WorkspaceMutability::ReadOnly,
        ));
        let backend = LocalWorkspaceFileBackend::confined(workspace.clone())
            .await
            .unwrap();
        let media = Arc::new(MemoryMedia::default());
        let tool =
            ThreadViewImageTool::new(Arc::new(backend), workspace.authorization(), media.clone());
        let input =
            OpaquePayload::new("application/json", 1, "{\"path\":\"misleading.txt\"}").unwrap();
        assert!(tool.execute(input.clone(), thread_context()).await.is_err());
        assert!(media.0.lock().unwrap().is_empty());
        let output = tool.execute(input, projected_context()).await.unwrap();
        let receipt: serde_json::Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(receipt["modelWidth"], 1);
        assert_eq!(receipt["modelHeight"], 1);
        assert_eq!(receipt["original"]["mediaType"], "image/png");
        assert_eq!(media.0.lock().unwrap()[0].as_ref(), original.as_slice());
        let variant = media.1.lock().unwrap()[0].clone().unwrap();
        let image = image::load_from_memory(&variant).unwrap();
        assert_eq!((image.width(), image.height()), (1, 1));
        tokio::fs::remove_file(directory.path().join("misleading.txt"))
            .await
            .unwrap();
        assert_eq!(media.0.lock().unwrap()[0].as_ref(), original.as_slice());
    }

    #[tokio::test]
    async fn cancelled_image_read_does_not_archive_an_image() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = ToolWorkspace::new(AgentWorkspace::local(directory.path()));
        let backend = LocalWorkspaceFileBackend::confined(workspace.clone())
            .await
            .unwrap();
        let media = Arc::new(MemoryMedia::default());
        let tool =
            ThreadViewImageTool::new(Arc::new(backend), workspace.authorization(), media.clone());
        let context = projected_context();
        context.cancellation.cancel();
        let input =
            OpaquePayload::new("application/json", 1, "{\"path\":\"missing.png\"}").unwrap();
        let error = tool.execute(input, context).await.unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        assert!(media.0.lock().unwrap().is_empty());
    }
}
