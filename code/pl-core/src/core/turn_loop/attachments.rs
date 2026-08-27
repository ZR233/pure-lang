use std::collections::BTreeSet;

use pl_model::{PreparedContentPart, PreparedContentSource};
use pl_protocol::{ContentPart, ModelContextItem, Result};

pub(super) fn prepare_context_items(
    items: &[ModelContextItem],
    attachments: &[crate::MaterializedAttachment],
) -> Result<(Vec<ModelContextItem>, Vec<PreparedContentPart>)> {
    let mut referenced = BTreeSet::new();
    for item in items {
        let Some(message) = item.as_message() else {
            continue;
        };
        for part in &message.content.parts {
            let ContentPart::Attachment {
                attachment_id,
                modality,
                ..
            } = part
            else {
                continue;
            };
            let attachment = attachments
                .iter()
                .find(|attachment| attachment.attachment_id == *attachment_id)
                .ok_or_else(|| {
                    pl_protocol::PureError::ConfigError(format!(
                        "attachment {attachment_id} was not materialized"
                    ))
                })?;
            if attachment.modality != *modality {
                return Err(pl_protocol::PureError::ConfigError(format!(
                    "attachment {attachment_id} materialized with a different modality"
                )));
            }
            referenced.insert(attachment_id.clone());
        }
    }

    let prepared_content = referenced
        .into_iter()
        .map(|attachment_id| {
            let attachment = attachments
                .iter()
                .find(|attachment| attachment.attachment_id == attachment_id)
                .expect("referenced attachment was checked above");
            let mut sources = Vec::with_capacity(2);
            if let Some(url) = &attachment.initial_remote_url {
                sources.push(PreparedContentSource::RemoteUrl { url: url.clone() });
            }
            sources.push(PreparedContentSource::DataUrl {
                base64: attachment.data.clone(),
            });
            PreparedContentPart {
                attachment_id,
                modality: attachment.modality,
                media_type: attachment.media_type.clone(),
                filename: attachment.filename.clone(),
                sources,
            }
        })
        .collect();

    Ok((items.to_vec(), prepared_content))
}
