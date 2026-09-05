use std::future::Future;
use std::path::Path;

use pl_protocol::Result;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::AttachmentRuntime;
use crate::tool::cache::ToolCachePolicy;
use crate::tool::{
    LocalWorkspaceFileBackend, MAX_SOURCE_BYTES, StaticTool, ToolCallContext, ToolPolicy,
    ToolResult, ToolWorkspace, WorkspaceFileBackend, WorkspaceFileReadBytesRequest,
    WorkspaceFileStatRequest, normalize_tool_image, tool_error,
};
use pl_model::model::{
    MediaRepresentation, ModelInfo, ModelInputCapability, ModelInputSource, ModelModality,
};

pub const TOOL_VIEW_IMAGE: &str = "view_image";
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewImageInput {
    /// Local path to an image in the current workspace.
    path: String,
}

#[derive(Debug, Clone)]
pub struct ViewImageTool {
    workspace: ToolWorkspace,
    remote_backend: Option<std::sync::Arc<crate::remote::RemoteWorkspaceFileBackend>>,
    capability: ModelInputCapability,
    attachment_runtime: AttachmentRuntime,
}

impl ViewImageTool {
    /// 为完整支持本地图片快照重放的模型构造工具；其余模型 fail closed。
    pub fn for_model(
        workspace: ToolWorkspace,
        model: &ModelInfo,
        attachment_runtime: AttachmentRuntime,
    ) -> Option<Self> {
        let capability = model.capabilities.input_capability(ModelModality::Image)?;
        let profile = model.binding.request.media_profile(ModelModality::Image)?;
        if !capability.supports_source(ModelInputSource::Local)
            || capability.limits.max_count == Some(0)
            || profile.first_send.is_empty()
            || !profile.replay.contains(&MediaRepresentation::DataUrl)
        {
            return None;
        }
        Some(Self {
            workspace,
            remote_backend: None,
            capability: capability.clone(),
            attachment_runtime,
        })
    }

    /// 为远端 workspace 构造同名图片工具；图片字节由 helper 读取，解码、压缩和附件落库
    /// 仍全部在本地 core 完成。
    pub fn for_remote_model(
        workspace: ToolWorkspace,
        backend: std::sync::Arc<crate::remote::RemoteWorkspaceFileBackend>,
        model: &ModelInfo,
        attachment_runtime: AttachmentRuntime,
    ) -> Option<Self> {
        let capability = model.capabilities.input_capability(ModelModality::Image)?;
        let profile = model.binding.request.media_profile(ModelModality::Image)?;
        if !capability.supports_source(ModelInputSource::Local)
            || capability.limits.max_count == Some(0)
            || profile.first_send.is_empty()
            || !profile.replay.contains(&MediaRepresentation::DataUrl)
        {
            return None;
        }
        Some(Self {
            workspace,
            remote_backend: Some(backend),
            capability: capability.clone(),
            attachment_runtime,
        })
    }

    fn ensure_available(&self) -> Result<()> {
        if !self.capability.supports_source(ModelInputSource::Local)
            || self.capability.limits.max_count == Some(0)
        {
            return Err(tool_error(
                TOOL_VIEW_IMAGE,
                "the current model no longer accepts local image snapshots",
            ));
        }
        Ok(())
    }
}

impl StaticTool for ViewImageTool {
    type Input = ViewImageInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin(TOOL_VIEW_IMAGE),
            "Read a local workspace image and add it to the model's visual context. Supports validated PNG, JPEG, WebP, and GIF files; image bytes are never returned as text.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
            .with_parallel_tool_calls()
            .with_cache_policy(ToolCachePolicy::UntilWorkspaceMutation)
    }

    fn execute(
        &self,
        input: ViewImageInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult>> + Send {
        async move {
            self.ensure_available()?;
            let path = input.path.trim();
            if path.is_empty() {
                return Err(tool_error(TOOL_VIEW_IMAGE, "path must not be empty"));
            }

            let stat_request = WorkspaceFileStatRequest {
                path: path.to_string(),
                cwd: None,
            };
            let read_request = WorkspaceFileReadBytesRequest {
                path: path.to_string(),
                cwd: None,
                max_bytes: MAX_SOURCE_BYTES,
            };
            let (stat, bytes) = if let Some(backend) = &self.remote_backend {
                (
                    backend.stat(stat_request).await?,
                    backend.read_bytes(read_request).await?,
                )
            } else {
                let backend =
                    LocalWorkspaceFileBackend::for_call(&self.workspace, &context).await?;
                (
                    backend.stat(stat_request).await?,
                    backend.read_bytes(read_request).await?,
                )
            };
            if !stat.is_file {
                return Err(tool_error(
                    TOOL_VIEW_IMAGE,
                    format!("'{path}' is not a regular file"),
                ));
            }
            let limits = self.capability.clone();
            let normalized = tokio::task::spawn_blocking(move || {
                normalize_tool_image(TOOL_VIEW_IMAGE, bytes, None, &limits)
            })
            .await
            .map_err(|error| {
                tool_error(TOOL_VIEW_IMAGE, format!("image task failed: {error}"))
            })??;
            let filename = safe_filename(path);
            let input = crate::ToolImageAttachmentInput {
                filename: filename.clone(),
                media_type: normalized.media_type.clone(),
                content_sha256: normalized.content_sha256.clone(),
                width: normalized.width,
                height: normalized.height,
                data: normalized.bytes,
            };
            let attachment = self.attachment_runtime.write_image(input).await?;
            let result = ToolResult::json(serde_json::json!({
                "viewedImage": true,
                "filename": filename,
                "mediaType": attachment.media_type,
                "width": attachment.width,
                "height": attachment.height,
                "byteSize": attachment.byte_size,
                "contentSha256": normalized.content_sha256,
            }))?;
            Ok(result.with_model_attachment(attachment))
        }
    }
}

fn safe_filename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("image")
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use pl_protocol::AttachmentModality;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{
        AgentWorkspace, MAX_DATA_URL_BASE64_BYTES, MAX_IMAGE_SIDE, StaticToolTestExt, ToolInput,
        ToolResultContent, base64_encoded_len,
    };

    fn model(slug: &str) -> ModelInfo {
        pl_model::model::default_models()
            .into_iter()
            .find(|model| model.slug == slug)
            .expect("bundled model")
    }

    fn context() -> ToolCallContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolCallContext::test(event_tx)
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image =
            DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, Rgb([240, 30, 40])));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    fn runtime(writes: Arc<Mutex<Vec<crate::ToolImageAttachmentInput>>>) -> AttachmentRuntime {
        AttachmentRuntime::new(
            move |input| {
                let writes = writes.clone();
                async move {
                    let attachment = pl_protocol::ThreadAttachment {
                        id: format!("attachment-{}", input.content_sha256),
                        modality: AttachmentModality::Image,
                        media_type: input.media_type.clone(),
                        filename: Some(input.filename.clone()),
                        width: Some(input.width),
                        height: Some(input.height),
                        byte_size: input.data.len() as u64,
                    };
                    writes.lock().unwrap().push(input);
                    Ok(attachment)
                }
            },
            |_| async { Ok(Vec::new()) },
        )
    }

    #[test]
    fn text_model_does_not_expose_view_image() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let tool = ViewImageTool::for_model(
            ToolWorkspace::new(AgentWorkspace::local(".")),
            &model("deepseek-v4-flash"),
            runtime(writes),
        );

        assert!(tool.is_none());
    }

    #[test]
    fn vision_model_with_snapshot_replay_exposes_view_image() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let tool = ViewImageTool::for_model(
            ToolWorkspace::new(AgentWorkspace::local(".")),
            &model("deepseek-v4-flash-vision-exp"),
            runtime(writes),
        );

        assert!(tool.is_some());
    }

    #[tokio::test]
    async fn magic_bytes_drive_format_and_tool_returns_typed_attachment() {
        let workspace = tempfile::tempdir().unwrap();
        tokio::fs::write(workspace.path().join("misleading.txt"), png(64, 32))
            .await
            .unwrap();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let tool = ViewImageTool::for_model(
            ToolWorkspace::new(AgentWorkspace::confined(
                workspace.path(),
                crate::tool::WorkspaceMutability::ReadOnly,
            )),
            &model("deepseek-v4-flash-vision-exp"),
            runtime(writes.clone()),
        )
        .unwrap();

        let result = tool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({"path": "misleading.txt"}),
                },
                context(),
            )
            .await
            .unwrap();

        assert_eq!(result.model_attachments.len(), 1);
        assert_eq!(result.model_attachments[0].media_type, "image/png");
        assert_eq!(result.model_attachments[0].width, Some(64));
        assert_eq!(result.model_attachments[0].height, Some(32));
        assert!(matches!(result.content, ToolResultContent::Json(_)));
        assert_eq!(writes.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn corrupt_image_fails_before_attachment_writer() {
        let workspace = tempfile::tempdir().unwrap();
        tokio::fs::write(workspace.path().join("broken.png"), b"not an image")
            .await
            .unwrap();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let tool = ViewImageTool::for_model(
            ToolWorkspace::new(AgentWorkspace::local(workspace.path())),
            &model("deepseek-v4-flash-vision-exp"),
            runtime(writes.clone()),
        )
        .unwrap();

        let error = tool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({"path": "broken.png"}),
                },
                context(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("file header"));
        assert!(writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn source_over_hard_limit_fails_before_attachment_writer() {
        let workspace = tempfile::tempdir().unwrap();
        let file = tokio::fs::File::create(workspace.path().join("oversized.png"))
            .await
            .unwrap();
        file.set_len((MAX_SOURCE_BYTES + 1) as u64).await.unwrap();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let tool = ViewImageTool::for_model(
            ToolWorkspace::new(AgentWorkspace::local(workspace.path())),
            &model("deepseek-v4-flash-vision-exp"),
            runtime(writes.clone()),
        )
        .unwrap();

        let error = tool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({"path": "oversized.png"}),
                },
                context(),
            )
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("byte source limit"),
            "unexpected error: {error}"
        );
        assert!(writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn directory_is_rejected_before_attachment_writer() {
        let workspace = tempfile::tempdir().unwrap();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let tool = ViewImageTool::for_model(
            ToolWorkspace::new(AgentWorkspace::local(workspace.path())),
            &model("deepseek-v4-flash-vision-exp"),
            runtime(writes.clone()),
        )
        .unwrap();

        let error = tool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({"path": "."}),
                },
                context(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("not a regular file"));
        assert!(writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn attachment_writer_failure_does_not_return_media_context() {
        let workspace = tempfile::tempdir().unwrap();
        tokio::fs::write(workspace.path().join("valid.png"), png(16, 8))
            .await
            .unwrap();
        let runtime = AttachmentRuntime::new(
            |_| async {
                Err(tool_error(
                    TOOL_VIEW_IMAGE,
                    "attachment database transaction failed",
                ))
            },
            |_| async { Ok(Vec::new()) },
        );
        let tool = ViewImageTool::for_model(
            ToolWorkspace::new(AgentWorkspace::local(workspace.path())),
            &model("deepseek-v4-flash-vision-exp"),
            runtime,
        )
        .unwrap();

        let error = tool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({"path": "valid.png"}),
                },
                context(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("database transaction failed"));
    }

    #[tokio::test]
    async fn large_image_is_normalized_to_bounded_snapshot() {
        let workspace = tempfile::tempdir().unwrap();
        tokio::fs::write(workspace.path().join("large.png"), png(4000, 100))
            .await
            .unwrap();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let tool = ViewImageTool::for_model(
            ToolWorkspace::new(AgentWorkspace::local(workspace.path())),
            &model("deepseek-v4-flash-vision-exp"),
            runtime(writes.clone()),
        )
        .unwrap();

        tool.execute_raw(
            ToolInput {
                arguments: serde_json::json!({"path": "large.png"}),
            },
            context(),
        )
        .await
        .unwrap();

        let writes = writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert!(writes[0].width <= MAX_IMAGE_SIDE);
        assert!(writes[0].height <= MAX_IMAGE_SIDE);
        assert!(base64_encoded_len(writes[0].data.len()) <= MAX_DATA_URL_BASE64_BYTES);
        assert_eq!(writes[0].media_type, "image/jpeg");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn confined_workspace_rejects_symlink_escape_before_writer() {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.png"), png(8, 8)).unwrap();
        symlink(
            outside.path().join("secret.png"),
            workspace.path().join("escape.png"),
        )
        .unwrap();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let tool = ViewImageTool::for_model(
            ToolWorkspace::new(AgentWorkspace::confined(
                workspace.path(),
                crate::tool::WorkspaceMutability::ReadOnly,
            )),
            &model("deepseek-v4-flash-vision-exp"),
            runtime(writes.clone()),
        )
        .unwrap();

        let error = tool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({"path": "escape.png"}),
                },
                context(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("symbolic link"));
        assert!(writes.lock().unwrap().is_empty());
    }
}
