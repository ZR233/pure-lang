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
