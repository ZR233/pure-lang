//! prompt 消息内容与附件运行时的构建。

use pl_core::AttachmentRuntime;

use crate::studio::StudioStore;
use crate::{AttachmentModality, ContentPart, MessageContent};

use super::super::resources::StudioAgentResources;
use super::errors::anyhow_error;

pub(super) fn prompt_content(
    prompt: &str,
    attachments: &[crate::studio::AttachmentRecord],
) -> MessageContent {
    if attachments.is_empty() {
        return MessageContent::text(prompt.to_string());
    }
    let mut parts = Vec::new();
    if !prompt.is_empty() {
        parts.push(ContentPart::Text {
            text: prompt.to_string(),
        });
    }
    parts.extend(
        attachments
            .iter()
            .map(|attachment| ContentPart::Attachment {
                attachment_id: attachment.id.clone(),
                modality: match attachment.modality {
                    pl_protocol::studio::StudioAttachmentModality::Image => {
                        AttachmentModality::Image
                    }
                    pl_protocol::studio::StudioAttachmentModality::Video => {
                        AttachmentModality::Video
                    }
                    pl_protocol::studio::StudioAttachmentModality::File => AttachmentModality::File,
                },
                media_type: attachment.media_type.clone(),
                filename: attachment.filename.clone(),
            }),
    );
    MessageContent::new(parts)
}

pub(super) fn attachment_runtime(
    store: StudioStore,
    resources: StudioAgentResources,
    thread_id: String,
) -> AttachmentRuntime {
    let writer_store = store.clone();
    let writer_resources = resources.clone();
    let writer_thread_id = thread_id.clone();
    AttachmentRuntime::new_batch(
        move |inputs| {
            let store = writer_store.clone();
            let resources = writer_resources.clone();
            let thread_id = writer_thread_id.clone();
            async move {
                let records = store
                    .persist_tool_image_records(&thread_id, inputs)
                    .await
                    .map_err(anyhow_error)?;
                let attachments = records
                    .iter()
                    .map(crate::studio::store::attachment::thread_attachment)
                    .collect();
                resources
                    .insert_thread_attachments(&thread_id, records)
                    .await;
                Ok(attachments)
            }
        },
        move |attachment_ids| {
            let resources = resources.clone();
            let thread_id = thread_id.clone();
            async move {
                let records = resources
                    .selected_thread_attachments(&thread_id, &attachment_ids)
                    .await
                    .map_err(anyhow_error)?;
                crate::studio::store::attachment::materialize_attachment_records(records)
                    .await
                    .map_err(anyhow_error)
            }
        },
    )
}
