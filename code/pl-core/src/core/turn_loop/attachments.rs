use std::collections::{BTreeMap, BTreeSet};

use pl_model::completion::{PreparedContentPart, PreparedContentSource};
use pl_protocol::{AttachmentModality, ContentPart, ModelContextItem, Result, ThreadAttachment};

#[derive(Debug, Clone)]
struct AttachmentExpectation {
    modality: AttachmentModality,
    media_type: String,
    filename: Option<String>,
    persisted: Option<ThreadAttachment>,
}

pub(super) async fn prepare_context_items(
    items: &[ModelContextItem],
    attachments: &mut Vec<crate::MaterializedAttachment>,
    runtime: Option<&crate::AttachmentRuntime>,
) -> Result<(Vec<ModelContextItem>, Vec<PreparedContentPart>)> {
    let expected = collect_expectations(items)?;
    let missing = expected
        .keys()
        .filter(|attachment_id| {
            !attachments
                .iter()
                .any(|attachment| &attachment.attachment_id == *attachment_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        let runtime = runtime.ok_or_else(|| {
            pl_protocol::PureError::ConfigError(format!(
                "attachments were not materialized and no attachment loader is available: {}",
                missing.join(", ")
            ))
        })?;
        let loaded = runtime.load(missing.clone()).await?;
        validate_loaded_ids(&missing, &loaded)?;
        attachments.extend(loaded);
    }

    for (attachment_id, expectation) in &expected {
        let attachment = attachments
            .iter()
            .find(|attachment| attachment.attachment_id == *attachment_id)
            .ok_or_else(|| {
                pl_protocol::PureError::ConfigError(format!(
                    "attachment {attachment_id} was not materialized"
                ))
            })?;
        validate_materialized_attachment(attachment, expectation)?;
    }

    let prepared_content = expected
        .into_keys()
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

fn collect_expectations(
    items: &[ModelContextItem],
) -> Result<BTreeMap<String, AttachmentExpectation>> {
    let mut expected = BTreeMap::new();
    for item in items {
        if let Some(message) = item.as_message() {
            for part in &message.content.parts {
                let ContentPart::Attachment {
                    attachment_id,
                    modality,
                    media_type,
                    filename,
                } = part
                else {
                    continue;
                };
                insert_expectation(
                    &mut expected,
                    attachment_id,
                    AttachmentExpectation {
                        modality: *modality,
                        media_type: media_type.clone(),
                        filename: filename.clone(),
                        persisted: None,
                    },
                )?;
            }
        }
        if let ModelContextItem::ToolMedia { items } = item {
            for item in items {
                let attachment = &item.attachment;
                insert_expectation(
                    &mut expected,
                    &attachment.id,
                    AttachmentExpectation {
                        modality: attachment.modality,
                        media_type: attachment.media_type.clone(),
                        filename: attachment.filename.clone(),
                        persisted: Some(attachment.clone()),
                    },
                )?;
            }
        }
    }
    Ok(expected)
}

fn insert_expectation(
    expected: &mut BTreeMap<String, AttachmentExpectation>,
    attachment_id: &str,
    value: AttachmentExpectation,
) -> Result<()> {
    if let Some(existing) = expected.get_mut(attachment_id) {
        if existing.modality != value.modality
            || existing.media_type != value.media_type
            || existing.filename != value.filename
        {
            return Err(pl_protocol::PureError::ConfigError(format!(
                "attachment {attachment_id} has conflicting context metadata"
            )));
        }
        if let (Some(existing), Some(value)) = (&existing.persisted, &value.persisted)
            && existing != value
        {
            return Err(pl_protocol::PureError::ConfigError(format!(
                "attachment {attachment_id} has conflicting persisted metadata"
            )));
        }
        if existing.persisted.is_none() {
            existing.persisted = value.persisted;
        }
        return Ok(());
    }
    expected.insert(attachment_id.to_string(), value);
    Ok(())
}

fn validate_loaded_ids(
    requested: &[String],
    loaded: &[crate::MaterializedAttachment],
) -> Result<()> {
    let requested = requested.iter().collect::<BTreeSet<_>>();
    let mut returned = BTreeSet::new();
    for attachment in loaded {
        if !requested.contains(&attachment.attachment_id) {
            return Err(pl_protocol::PureError::ConfigError(format!(
                "attachment loader returned unrequested attachment {}",
                attachment.attachment_id
            )));
        }
        if !returned.insert(&attachment.attachment_id) {
            return Err(pl_protocol::PureError::ConfigError(format!(
                "attachment loader returned duplicate attachment {}",
                attachment.attachment_id
            )));
        }
    }
    if returned.len() != requested.len() {
        let missing = requested
            .into_iter()
            .filter(|attachment_id| !returned.contains(*attachment_id))
            .cloned()
            .collect::<Vec<_>>();
        return Err(pl_protocol::PureError::ConfigError(format!(
            "attachment loader did not return: {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

fn validate_materialized_attachment(
    attachment: &crate::MaterializedAttachment,
    expectation: &AttachmentExpectation,
) -> Result<()> {
    if attachment.modality != expectation.modality
        || attachment.media_type != expectation.media_type
        || attachment.filename != expectation.filename
    {
        return Err(pl_protocol::PureError::ConfigError(format!(
            "attachment {} materialized with different context metadata",
            attachment.attachment_id
        )));
    }
    if let Some(persisted) = &expectation.persisted
        && (attachment.byte_size != persisted.byte_size
            || attachment.width != persisted.width
            || attachment.height != persisted.height)
    {
        return Err(pl_protocol::PureError::ConfigError(format!(
            "attachment {} materialized with different persisted metadata",
            attachment.attachment_id
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use pl_protocol::ToolMediaContext;
    use pretty_assertions::assert_eq;

    use super::*;

    fn thread_attachment() -> ThreadAttachment {
        ThreadAttachment {
            id: "attachment-1".to_string(),
            modality: AttachmentModality::Image,
            media_type: "image/png".to_string(),
            filename: Some("image.png".to_string()),
            width: Some(16),
            height: Some(8),
            byte_size: 3,
        }
    }

    fn tool_media() -> Vec<ModelContextItem> {
        vec![ModelContextItem::ToolMedia {
            items: vec![ToolMediaContext {
                call_id: "call-1".to_string(),
                label: "image.png".to_string(),
                attachment: thread_attachment(),
            }],
        }]
    }

    fn materialized() -> crate::MaterializedAttachment {
        crate::MaterializedAttachment {
            attachment_id: "attachment-1".to_string(),
            modality: AttachmentModality::Image,
            media_type: "image/png".to_string(),
            filename: Some("image.png".to_string()),
            data: "cG5n".to_string(),
            byte_size: 3,
            width: Some(16),
            height: Some(8),
            initial_remote_url: None,
        }
    }

    #[tokio::test]
    async fn missing_tool_media_is_loaded_once_and_cached_for_the_turn() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let loader_calls = calls.clone();
        let runtime = crate::AttachmentRuntime::new(
            |_| async { panic!("writer is not used") },
            move |ids| {
                let calls = loader_calls.clone();
                async move {
                    calls.lock().unwrap().push(ids);
                    Ok(vec![materialized()])
                }
            },
        );
        let mut cache = Vec::new();

        let (_, first) = prepare_context_items(&tool_media(), &mut cache, Some(&runtime))
            .await
            .unwrap();
        let (_, second) = prepare_context_items(&tool_media(), &mut cache, Some(&runtime))
            .await
            .unwrap();

        assert_eq!(
            calls.lock().unwrap().as_slice(),
            &[vec!["attachment-1".to_string()]]
        );
        assert_eq!(first, second);
        assert_eq!(first[0].attachment_id, "attachment-1");
        assert_eq!(first[0].sources.len(), 1);
    }

    #[tokio::test]
    async fn loader_metadata_mismatch_is_rejected() {
        let runtime = crate::AttachmentRuntime::new(
            |_| async { panic!("writer is not used") },
            |_| async {
                let mut attachment = materialized();
                attachment.width = Some(99);
                Ok(vec![attachment])
            },
        );
        let error = prepare_context_items(&tool_media(), &mut Vec::new(), Some(&runtime))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("different persisted metadata"));
    }

    #[tokio::test]
    async fn missing_attachment_without_host_loader_fails_closed() {
        let error = prepare_context_items(&tool_media(), &mut Vec::new(), None)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("no attachment loader"));
    }
}
