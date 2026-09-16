//! Persistent raw media and model-owned image projection for Thread tools.
use crate::resource_store::FileResourceStore;
use pl_core::{context::ContextContent, tool::opaque::ToolError};
use pl_tool::media::{RetainedToolMedia, ToolMedia, ToolMediaHost, ToolMediaKind};
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct MediaHost(pub(super) FileResourceStore);

impl ToolMediaHost for MediaHost {
    async fn retain(&self, media: ToolMedia) -> Result<RetainedToolMedia, ToolError> {
        let reference = self
            .0
            .retain_bytes(media.bytes.clone(), &media.media_type)
            .await
            .map_err(ToolError::new)?;
        let context = if let Some(image) = media.model_image {
            let projected = if image.media_type == media.media_type
                && image.bytes.as_ref() == media.bytes.as_ref()
            {
                reference.clone()
            } else {
                self.0
                    .retain_bytes(image.bytes, &image.media_type)
                    .await
                    .map_err(ToolError::new)?
            };
            vec![
                pl_model::runtime::attachment_content(
                    projected,
                    pl_protocol::AttachmentModality::Image,
                )
                .map_err(ToolError::new)?,
            ]
        } else {
            let mut context = vec![ContextContent::Resource {
                reference: reference.clone(),
            }];
            match media.kind {
                ToolMediaKind::Image => context.push(ContextContent::Text { text: Arc::from("Image retained as a resource; this prepared model does not advertise image input.") }),
                ToolMediaKind::Audio | ToolMediaKind::Blob => {}
            }
            context
        };
        Ok(RetainedToolMedia { reference, context })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        resource_store::RESOURCE_ID_PREFIX,
        studio::thread_projection::{delivery_attachments, read_persisted_media},
    };
    use pl_core::{
        context::{ContextRecord, ContextSource, OpaquePayload},
        model::ModelToolCall,
        thread::{
            ToolDelivery, ToolOutcome,
            journal::{ContextChange, ThreadCommit, replay},
        },
        tool::opaque::{CallContext, Tool},
    };
    use pl_tool::{
        image::ThreadViewImageTool,
        workspace::{AgentWorkspace, ToolWorkspace, WorkspaceMutability},
        workspace_file::LocalWorkspaceFileBackend,
    };
    use pretty_assertions::assert_eq;

    fn image_context() -> CallContext {
        let projection = pl_protocol::tool_projection::ToolProjection {
            image: Some(pl_protocol::tool_projection::ImageProjection {
                max_width: Some(1),
                max_height: Some(1),
                ..Default::default()
            }),
        };
        CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: Some(
                OpaquePayload::new(
                    pl_protocol::tool_projection::FORMAT,
                    pl_protocol::tool_projection::VERSION,
                    serde_json::to_string(&projection).unwrap(),
                )
                .unwrap(),
            ),
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: tokio_util::sync::CancellationToken::new(),
            extensions: Default::default(),
            catalog: Vec::new().into(),
            extension_sequence: 0,
        }
    }

    fn delivery(output: pl_core::tool::ToolOutput) -> ToolDelivery {
        ToolDelivery {
            target: Default::default(),
            call_id: "call".into(),
            tool_id: "view_image".into(),
            delivered_context: output.context().to_vec(),
            output,
            outcome: ToolOutcome::Succeeded,
        }
    }

    /// Runs the real workspace reader, media host and archive store over a fresh PNG.
    async fn retain_source_image() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        FileResourceStore,
        ToolDelivery,
    ) {
        let source_dir = tempfile::tempdir().unwrap();
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(4, 4)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        std::fs::write(source_dir.path().join("photo.png"), encoded.into_inner()).unwrap();

        let resource_dir = tempfile::tempdir().unwrap();
        let store = FileResourceStore::new(resource_dir.path().to_path_buf());
        let workspace = ToolWorkspace::new(AgentWorkspace::confined(
            source_dir.path(),
            WorkspaceMutability::ReadOnly,
        ));
        let backend = LocalWorkspaceFileBackend::confined(workspace.clone())
            .await
            .unwrap();
        let tool = ThreadViewImageTool::new(
            Arc::new(backend),
            workspace.authorization(),
            Arc::new(MediaHost(store.clone())),
        );
        let input = OpaquePayload::new("application/json", 1, r#"{"path":"photo.png"}"#).unwrap();
        let output = tool.execute(input, image_context()).await.unwrap();
        (source_dir, resource_dir, store, delivery(output))
    }

    #[tokio::test]
    async fn archived_tool_image_projects_typed_attachment_and_reads_without_source_path() {
        let (source_dir, resource_dir, store, delivery) = retain_source_image().await;
        let attachments = delivery_attachments(&delivery).unwrap();
        assert_eq!(attachments.len(), 1);
        let attachment = &attachments[0];
        assert!(attachment.id.starts_with(RESOURCE_ID_PREFIX));
        assert_eq!(attachment.media_type, "image/jpeg");
        assert_eq!(attachment.filename.as_deref(), Some("photo.png"));
        // No receipt binds dimensions to the archived variant, so the size stays unknown even
        // though the archived bytes really are 1x1.
        assert_eq!((attachment.width, attachment.height), (None, None));

        let deliveries = [delivery.clone()];
        let bytes = read_persisted_media(&store, &deliveries, &attachment.id)
            .await
            .unwrap();
        assert_eq!(bytes.len() as u64, attachment.byte_size);
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Jpeg
        );
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (1, 1));

        // The archived variant survives removal of the original workspace source.
        std::fs::remove_file(source_dir.path().join("photo.png")).unwrap();
        assert_eq!(
            read_persisted_media(&store, &deliveries, &attachment.id)
                .await
                .unwrap(),
            bytes
        );

        // Unknown, cross-Thread, and corrupt resources are rejected rather than re-read.
        assert!(
            read_persisted_media(&store, &[], &attachment.id)
                .await
                .is_err()
        );
        let foreign = format!("{RESOURCE_ID_PREFIX}{}", "0".repeat(64));
        assert!(
            read_persisted_media(&store, &deliveries, &foreign)
                .await
                .is_err()
        );
        let object = resource_dir
            .path()
            .join(attachment.id.strip_prefix(RESOURCE_ID_PREFIX).unwrap());
        std::fs::write(&object, b"corrupt!").unwrap();
        assert!(
            read_persisted_media(&store, &deliveries, &attachment.id)
                .await
                .is_err()
        );
        std::fs::remove_file(&object).unwrap();
        assert!(
            read_persisted_media(&store, &deliveries, &attachment.id)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn media_without_a_tool_receipt_reports_no_guessed_size() {
        let resource_dir = tempfile::tempdir().unwrap();
        let store = FileResourceStore::new(resource_dir.path().to_path_buf());
        let reference = store
            .retain_bytes(Arc::from(&b"\x89PNG\r\n\x1a\n"[..]), "image/png")
            .await
            .unwrap();
        let context = pl_model::runtime::attachment_content(
            reference,
            pl_protocol::AttachmentModality::Image,
        )
        .unwrap();
        let output = pl_core::tool::ToolOutput::new(
            OpaquePayload::new("pl.tool.mcp-resources", 1, "{}").unwrap(),
            vec![context],
        );
        let attachments = delivery_attachments(&delivery(output)).unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!((attachments[0].width, attachments[0].height), (None, None));
        assert_eq!(attachments[0].filename, None);
    }

    fn journal_commit(delivery: &ToolDelivery) -> ThreadCommit {
        let call = ModelToolCall {
            call_id: delivery.call_id.clone(),
            tool_id: delivery.tool_id.clone(),
            arguments: OpaquePayload::new("application/json", 1, r#"{"path":"photo.png"}"#)
                .unwrap(),
        };
        let assistant = ContextRecord {
            id: "assistant".into(),
            turn_id: Some("turn".into()),
            source: ContextSource::Assistant,
            content: Vec::new(),
            tool_calls: vec![call],
        };
        let result = ContextRecord {
            id: "result".into(),
            turn_id: Some("turn".into()),
            source: ContextSource::ToolResult {
                call_id: delivery.call_id.clone(),
                tool_id: delivery.tool_id.clone(),
            },
            content: delivery.delivered_context.clone(),
            tool_calls: Vec::new(),
        };
        ThreadCommit {
            committed_at: 1,
            thread_id: "thread".into(),
            sequence: 1,
            permissions: Vec::new().into(),
            wake_messages_through: None,
            inputs: Vec::new().into(),
            tasks: Vec::new().into(),
            context: Some(ContextChange::Append {
                revision: 1,
                records: vec![assistant, result].into(),
            }),
            private_context: None,
            attempt: None,
            turn: None,
            discovered_tools: None,
            deliveries: vec![delivery.clone()].into(),
            extensions: Vec::new().into(),
            inbox: Vec::new().into(),
            consumed_messages: None,
            interactions: Vec::new().into(),
            replacements: Vec::new().into(),
            runtime_facts: None,
            lifecycle: None,
        }
    }

    #[tokio::test]
    async fn archived_tool_media_restores_through_cold_replay_and_reads_without_the_source() {
        let (source_dir, _resource_dir, store, delivery) = retain_source_image().await;
        let live = delivery_attachments(&delivery).unwrap();
        assert_eq!(live.len(), 1);
        let attachment_id = live[0].id.clone();

        // Round-trip the committed fact through the cold-store codec and replay the journal,
        // exactly as a restart restores history, instead of reusing the live delivery.
        let encoded = journal_commit(&delivery).encode().unwrap();
        let decoded = ThreadCommit::decode(&encoded).unwrap();
        let restored = replay(&[std::sync::Arc::new(decoded)]).unwrap();
        assert_eq!(restored.deliveries.len(), 1);

        let restored_attachments = delivery_attachments(&restored.deliveries[0]).unwrap();
        assert_eq!(restored_attachments, live);

        // The original workspace file no longer exists; the archived variant still reads back.
        std::fs::remove_file(source_dir.path().join("photo.png")).unwrap();
        let bytes = read_persisted_media(&store, &restored.deliveries, &attachment_id)
            .await
            .unwrap();
        assert_eq!(bytes.len() as u64, live[0].byte_size);
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Jpeg
        );
    }
}
