use pl_protocol::{ContentPart, ImageSource, MessageContent, Result};

use super::protocol_error;
pub(super) fn message_content_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::MultiPart(parts) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub(super) fn data_url(source: &ImageSource, media_type: &str) -> Result<String> {
    match source {
        ImageSource::InlineBase64 { data } => Ok(format!("data:{media_type};base64,{data}")),
        ImageSource::Attachment { attachment_id } => Err(protocol_error(format!(
            "image attachment {attachment_id} was not materialized before model request"
        ))),
    }
}
