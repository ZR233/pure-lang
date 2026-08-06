use pl_protocol::{ContentPart, ImageSource, MessageContent, ModelContextItem, Result};

pub(super) fn materialize_context_items(
    items: &[ModelContextItem],
    attachments: &[crate::MaterializedAttachment],
) -> Result<Vec<ModelContextItem>> {
    items
        .iter()
        .map(|item| match item {
            ModelContextItem::Message { message } => {
                let mut message = message.clone();
                message.content = materialize_content(&message.content, attachments)?;
                Ok(ModelContextItem::from(message))
            }
            ModelContextItem::ToolResult { message, receipt } => {
                let mut message = message.clone();
                message.content = materialize_content(&message.content, attachments)?;
                Ok(ModelContextItem::ToolResult {
                    message,
                    receipt: receipt.clone(),
                })
            }
            ModelContextItem::Compaction { .. } => Ok(item.clone()),
        })
        .collect()
}

fn materialize_content(
    content: &MessageContent,
    attachments: &[crate::MaterializedAttachment],
) -> Result<MessageContent> {
    match content {
        MessageContent::Text(text) => Ok(MessageContent::Text(text.clone())),
        MessageContent::MultiPart(parts) => parts
            .iter()
            .map(|part| match part {
                ContentPart::Text { text } => Ok(ContentPart::Text { text: text.clone() }),
                ContentPart::Image {
                    source,
                    media_type,
                    filename,
                } => {
                    let ImageSource::Attachment { attachment_id } = source else {
                        return Ok(part.clone());
                    };
                    let attachment = attachments
                        .iter()
                        .find(|attachment| attachment.attachment_id == *attachment_id)
                        .ok_or_else(|| {
                            pl_protocol::PureError::ConfigError(format!(
                                "attachment {attachment_id} was not materialized"
                            ))
                        })?;
                    Ok(ContentPart::Image {
                        source: ImageSource::InlineBase64 {
                            data: attachment.data.clone(),
                        },
                        media_type: if media_type.is_empty() {
                            attachment.media_type.clone()
                        } else {
                            media_type.clone()
                        },
                        filename: filename.clone().or_else(|| attachment.filename.clone()),
                    })
                }
            })
            .collect::<Result<Vec<_>>>()
            .map(MessageContent::MultiPart),
    }
}
