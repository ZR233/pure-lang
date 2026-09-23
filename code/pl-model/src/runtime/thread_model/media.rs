//! Explicit role-free media projection and verified preparation before request admission.
use super::{AdapterError, failure};
use crate::{
    completion::{AttachmentInput, AttachmentModality, AttachmentSource, CompletionRequest},
    model::{ModelInfo, ModelInputSource, ModelModality},
};
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

/// Host-owned media identity and modality that a model request consumes as one attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolAttachment {
    pub reference: ResourceReference,
    pub modality: AttachmentModality,
}

/// Decodes a versioned attachment projection produced by [`attachment_content`].
///
/// Returns `Ok(None)` for another producer's payload so callers can inspect mixed context
/// without treating unrelated opaque content as a media projection.
///
/// # Errors
/// Rejects malformed projection content, an unsupported version, or invalid reference metadata.
pub fn decode_attachment(payload: &OpaquePayload) -> Result<Option<ToolAttachment>, ModelError> {
    if payload.format() != FORMAT {
        return Ok(None);
    }
    let attachment = decode(payload)?;
    Ok(Some(ToolAttachment {
        reference: attachment.reference,
        modality: attachment.modality,
    }))
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
        encoded.attachments.push(AttachmentInput {
            attachment_id: attachment.reference.id().to_owned(),
            modality: attachment.modality,
            media_type: attachment.reference.media_type().to_owned(),
            filename: None,
            source: AttachmentSource::Bytes { bytes },
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
