//! Explicit role-free media projection and verified preparation before request admission.
use super::{AdapterError, failure};
use crate::{
    completion::{
        AttachmentModality, CompletionRequest, PreparedContentPart, PreparedContentSource,
    },
    model::{ModelInfo, ModelInputSource, ModelModality},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use pl_core::{
    context::{ContextContent, OpaquePayload, ResourceReference},
    model::{ModelError, ModelFailureKind, ModelRequest},
};
use std::collections::{BTreeMap, HashMap};

pub(super) fn tool_projection(model: &ModelInfo) -> Result<OpaquePayload, ModelError> {
    use pl_protocol::tool_projection::{ImageProjection, ToolProjection};
    let image = model
        .capabilities
        .input_capability(ModelModality::Image)
        .filter(|capability| {
            capability.supports_source(ModelInputSource::Local)
                && capability.limits.max_count != Some(0)
        })
        .map(|capability| {
            let limits = &capability.limits;
            ImageProjection {
                max_count: limits.max_count,
                max_bytes: limits.max_bytes,
                max_total_bytes: limits.max_total_bytes,
                max_width: limits.max_width,
                max_height: limits.max_height,
                media_types: limits.media_types.clone(),
            }
        });
    let content = serde_json::to_string(&ToolProjection { image })
        .map_err(|source| failure(ModelFailureKind::UnsupportedContent, source))?;
    OpaquePayload::new(
        pl_protocol::tool_projection::FORMAT,
        pl_protocol::tool_projection::VERSION,
        content,
    )
    .map_err(|source| failure(ModelFailureKind::UnsupportedContent, source))
}

pub(super) const FORMAT: &str = "pl.model.attachment";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Attachment {
    pub reference: ResourceReference,
    pub modality: AttachmentModality,
}

/// Requests media embedding explicitly, without allowing the producer to choose a message role.
///
/// # Errors
/// Rejects malformed resource metadata or projection encoding failure.
pub fn attachment_content(
    reference: ResourceReference,
    modality: AttachmentModality,
) -> Result<ContextContent, ModelError> {
    reference
        .validate()
        .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
    let content = serde_json::to_string(&Attachment {
        reference,
        modality,
    })
    .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
    let payload = OpaquePayload::new(FORMAT, 1, content)
        .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
    Ok(ContextContent::Opaque { payload })
}

pub(super) fn decode(payload: &OpaquePayload) -> Result<Attachment, ModelError> {
    if payload.format() != FORMAT || payload.version() != 1 {
        return Err(invalid("unknown attachment projection"));
    }
    let attachment: Attachment = serde_json::from_str(payload.content())
        .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
    attachment
        .reference
        .validate()
        .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
    Ok(attachment)
}

pub(super) async fn prepare(
    request: &ModelRequest,
    encoded: &mut CompletionRequest,
    model: &ModelInfo,
) -> Result<(), ModelError> {
    let mut attachments = BTreeMap::<String, Attachment>::new();
    let mut occurrences = BTreeMap::<String, u64>::new();
    for record in request.context.records.iter() {
        for content in &record.content {
            if let ContextContent::Opaque { payload } = content
                && payload.format() == FORMAT
            {
                let attachment = decode(payload)?;
                let count = occurrences
                    .entry(attachment.reference.id().to_owned())
                    .or_default();
                *count = count
                    .checked_add(1)
                    .ok_or_else(|| invalid("attachment count overflow"))?;
                if let Some(previous) = attachments.get(attachment.reference.id()) {
                    if previous.reference != attachment.reference
                        || previous.modality != attachment.modality
                    {
                        return Err(invalid("conflicting attachment identity"));
                    }
                } else {
                    attachments.insert(attachment.reference.id().to_owned(), attachment);
                }
            }
        }
    }
    if attachments.is_empty() {
        return Ok(());
    }
    let resources = request
        .resources
        .as_ref()
        .ok_or_else(|| invalid("attachment resource reader is missing"))?;
    let mut totals = HashMap::<AttachmentModality, (u64, u64)>::new();
    for attachment in attachments.values() {
        let capability = model
            .capabilities
            .input_capability(modality(attachment.modality))
            .ok_or_else(|| invalid("model does not support the attachment modality"))?;
        if !capability.supports_source(ModelInputSource::Local) {
            return Err(invalid("model does not support retained media bytes"));
        }
        let limits = &capability.limits;
        if limits
            .max_bytes
            .is_some_and(|limit| attachment.reference.byte_len() > limit)
            || (!limits.media_types.is_empty()
                && !limits
                    .media_types
                    .iter()
                    .any(|media| media == attachment.reference.media_type()))
        {
            return Err(invalid("attachment violates the model media limits"));
        }
        let total = totals.entry(attachment.modality).or_default();
        let count = occurrences[attachment.reference.id()];
        total.0 = total
            .0
            .checked_add(count)
            .ok_or_else(|| invalid("attachment count overflow"))?;
        let bytes = attachment
            .reference
            .byte_len()
            .checked_mul(count)
            .ok_or_else(|| invalid("attachment byte count overflow"))?;
        total.1 = total
            .1
            .checked_add(bytes)
            .ok_or_else(|| invalid("attachment byte count overflow"))?;
        if limits
            .max_count
            .is_some_and(|limit| total.0 > u64::from(limit))
            || limits.max_total_bytes.is_some_and(|limit| total.1 > limit)
        {
            return Err(invalid("attachment batch violates the model media limits"));
        }
    }
    if totals.len() > 1
        && model.binding.request.media_mix_policy == crate::model::MediaMixPolicy::SingleModality
    {
        return Err(invalid("model rejects mixed media modalities"));
    }
    for attachment in attachments.values() {
        let profile = model
            .binding
            .request
            .media_profile(modality(attachment.modality))
            .ok_or_else(|| invalid("model has no media encoding profile"))?;
        if !profile
            .replay
            .contains(&crate::model::MediaRepresentation::DataUrl)
        {
            return Err(invalid("model cannot replay retained media bytes"));
        }
    }
    for attachment in attachments.into_values() {
        let bytes = resources
            .read(&attachment.reference, request.cancellation.clone())
            .await
            .map_err(|error| {
                let kind = match &error {
                    pl_core::context::ResourceReadError::Cancelled => ModelFailureKind::Cancelled,
                    pl_core::context::ResourceReadError::Integrity(_) => {
                        ModelFailureKind::IncompatibleContext
                    }
                    pl_core::context::ResourceReadError::Unavailable { .. } => {
                        ModelFailureKind::Unavailable
                    }
                };
                failure(kind, error)
            })?;
        if attachment.modality == AttachmentModality::Image {
            let limits = model
                .capabilities
                .input_capability(ModelModality::Image)
                .ok_or_else(|| invalid("image capability changed during preparation"))?
                .limits
                .clone();
            let image_bytes = bytes.clone();
            let media_type = attachment.reference.media_type().to_owned();
            tokio::task::spawn_blocking(move || {
                let reader = image::ImageReader::new(std::io::Cursor::new(image_bytes))
                    .with_guessed_format()
                    .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
                if reader
                    .format()
                    .is_none_or(|format| format.to_mime_type() != media_type)
                {
                    return Err(invalid("image bytes do not match their media type"));
                }
                let (width, height) = reader
                    .into_dimensions()
                    .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
                if limits.max_width.is_some_and(|limit| width > limit)
                    || limits.max_height.is_some_and(|limit| height > limit)
                {
                    return Err(invalid("image dimensions exceed the model limits"));
                }
                Ok(())
            })
            .await
            .map_err(|error| failure(ModelFailureKind::Unavailable, error))??;
        }
        if request.cancellation.is_cancelled() {
            return Err(failure(
                ModelFailureKind::Cancelled,
                AdapterError::Content("media preparation cancelled"),
            ));
        }
        encoded.prepared_content.push(PreparedContentPart {
            attachment_id: attachment.reference.id().to_owned(),
            modality: attachment.modality,
            media_type: attachment.reference.media_type().to_owned(),
            filename: None,
            sources: vec![PreparedContentSource::DataUrl {
                base64: STANDARD.encode(&bytes),
            }],
        });
    }
    Ok(())
}

fn modality(value: AttachmentModality) -> ModelModality {
    match value {
        AttachmentModality::Image => ModelModality::Image,
        AttachmentModality::Video => ModelModality::Video,
        AttachmentModality::File => ModelModality::File,
    }
}

fn invalid(message: &'static str) -> ModelError {
    failure(
        ModelFailureKind::UnsupportedContent,
        AdapterError::Content(message),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::context::{
        ContextRecord, ContextSnapshot, ContextSource, ResourceAccess, ResourceReadError,
        ResourceReader,
    };
    use pretty_assertions::assert_eq;
    use sha2::{Digest, Sha256};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Debug)]
    struct Bytes {
        bytes: Arc<[u8]>,
        reads: Arc<AtomicUsize>,
    }
    impl ResourceReader for Bytes {
        async fn read(
            &self,
            _: ResourceReference,
            _: tokio_util::sync::CancellationToken,
        ) -> Result<Arc<[u8]>, ResourceReadError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.bytes.clone())
        }
    }

    fn model() -> ModelInfo {
        let mut model = ModelInfo::compatible("media-test");
        model
            .capabilities
            .input
            .push(crate::model::ModelInputCapability::media(
                ModelModality::Image,
                vec![ModelInputSource::Local],
            ));
        model
            .binding
            .request
            .media
            .push(crate::model::ModelMediaInputProfile {
                modality: ModelModality::Image,
                wire: crate::model::MediaWireFormat::ChatImageUrl,
                first_send: vec![crate::model::MediaRepresentation::DataUrl],
                replay: vec![crate::model::MediaRepresentation::DataUrl],
            });
        model
    }

    fn request(content: Vec<ContextContent>, resources: ResourceAccess) -> ModelRequest {
        ModelRequest {
            tool_call_mode: pl_core::model::ToolCallMode::Parallel,
            solo_tool_ids: Vec::new().into(),
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            attempt_id: "attempt".into(),
            context: ContextSnapshot {
                revision: 1,
                records: vec![ContextRecord {
                    id: "input".into(),
                    turn_id: Some("turn".into()),
                    source: ContextSource::User,
                    content,
                    tool_calls: Vec::new(),
                }]
                .into(),
            },
            tools: Vec::new().into(),
            committed_private_context: None,
            resources: Some(resources),
            progress: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn verified_image_is_encoded_from_exact_archived_bytes() {
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .unwrap();
        let bytes = buffer.into_inner();
        let reference = ResourceReference::new(
            "image".into(),
            format!("sha256:{:x}", Sha256::digest(&bytes)),
            bytes.len() as u64,
            "image/png".into(),
        )
        .unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let request = request(
            vec![attachment_content(reference, AttachmentModality::Image).unwrap()],
            ResourceAccess::new(Bytes {
                bytes: bytes.clone().into(),
                reads: reads.clone(),
            }),
        );
        let mut encoded = super::super::codec::request(&request).unwrap();
        prepare(&request, &mut encoded, &model()).await.unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            encoded.prepared_content[0].sources,
            vec![PreparedContentSource::DataUrl {
                base64: STANDARD.encode(bytes)
            }]
        );
    }

    #[tokio::test]
    async fn repeated_image_occurrences_count_toward_limits_before_reading() {
        let mut model = model();
        model
            .capabilities
            .input
            .iter_mut()
            .find(|input| input.modality == ModelModality::Image)
            .unwrap()
            .limits
            .max_count = Some(1);
        let reference = ResourceReference::new(
            "image".into(),
            format!("sha256:{:x}", Sha256::digest(b"bytes")),
            5,
            "image/png".into(),
        )
        .unwrap();
        let content = attachment_content(reference, AttachmentModality::Image).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let request = request(
            vec![content.clone(), content],
            ResourceAccess::new(Bytes {
                bytes: Arc::from(&b"bytes"[..]),
                reads: reads.clone(),
            }),
        );
        let mut encoded = super::super::codec::request(&request).unwrap();
        assert!(prepare(&request, &mut encoded, &model).await.is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
}
