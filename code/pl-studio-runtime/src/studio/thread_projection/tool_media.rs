//! Archived tool media shared by the Timeline projection and safe attachment reads.
//!
//! Both consumers decode the same persisted facts: a displayable attachment is always a
//! variant a tool actually archived, and a read is authorized only by a reference this Thread
//! itself persisted. Both sides scan every committed delivery and reject contradictory facts
//! instead of trusting the first match. Neither side guesses sizes, trusts client paths, or
//! scans a second registry.
use super::ProjectionError;
use crate::resource_store::FileResourceStore;
use anyhow::Context;
use pl_core::{
    context::{ContextContent, ResourceReader, ResourceReference},
    thread::ToolDelivery,
    tool::opaque::ToolError,
};
use pl_protocol::{AttachmentModality, ThreadAttachment};
use tokio_util::sync::CancellationToken;

/// One archived media variant referenced by a persisted tool delivery.
#[derive(Debug, Clone)]
struct ArchivedToolMedia {
    reference: ResourceReference,
    modality: AttachmentModality,
    filename: Option<String>,
}

impl ArchivedToolMedia {
    fn into_attachment(self) -> ThreadAttachment {
        ThreadAttachment {
            id: self.reference.id().to_owned(),
            modality: self.modality,
            media_type: self.reference.media_type().to_owned(),
            filename: self.filename,
            // The versioned `view_image` receipt has no binding to a specific archived variant,
            // so no dimension is ever reported from it; unknown sizes stay unknown.
            width: None,
            height: None,
            byte_size: self.reference.byte_len(),
        }
    }
}

/// Wraps a rejected persisted-media fact as a projection failure.
fn rejected(message: &str) -> ProjectionError {
    ProjectionError::ToolMedia(ToolError::new(std::io::Error::other(message.to_owned())))
}

/// Decodes the media actually delivered by one tool call.
///
/// Attachment identity, media type and byte length always come from the model-owned projection's
/// archived reference. A `view_image` receipt only supplies the display filename, and only when
/// it describes exactly one archived image attachment: a receipt that claims several attachments
/// or a non-image modality is contradictory and rejected. The receipt never contributes a
/// dimension because it does not bind to a specific archived variant.
fn archived_media(delivery: &ToolDelivery) -> Result<Vec<ArchivedToolMedia>, ProjectionError> {
    let receipt = pl_tool::image::saved_view_image_receipt(delivery.output.payload())
        .map_err(ProjectionError::ToolMedia)?;
    let mut attachments = Vec::new();
    for content in &delivery.delivered_context {
        let ContextContent::Opaque { payload } = content else {
            continue;
        };
        if let Some(attachment) = pl_model::runtime::decode_attachment(payload)? {
            attachments.push(attachment);
        }
    }
    let filename = match (receipt.as_ref(), attachments.as_slice()) {
        (Some(_), []) => None,
        (Some(receipt), [attachment]) if attachment.modality == AttachmentModality::Image => {
            Some(receipt.path.clone())
        }
        (Some(_), [_]) => {
            return Err(rejected(
                "view_image receipt describes a non-image model attachment",
            ));
        }
        (Some(_), _) => {
            return Err(rejected(
                "view_image receipt cannot describe multiple model attachments",
            ));
        }
        (None, _) => None,
    };
    Ok(attachments
        .into_iter()
        .map(|attachment| ArchivedToolMedia {
            reference: attachment.reference,
            modality: attachment.modality,
            filename: filename.clone(),
        })
        .collect())
}

/// Typed attachments for one terminal tool delivery, in delivered order.
///
/// # Errors
/// Rejects a corrupt model attachment projection or an undecodable tool receipt.
pub(crate) fn delivery_attachments(
    delivery: &ToolDelivery,
) -> Result<Vec<ThreadAttachment>, ProjectionError> {
    Ok(archived_media(delivery)?
        .into_iter()
        .map(ArchivedToolMedia::into_attachment)
        .collect())
}

/// Returns the archived reference this Thread persisted for `attachment_id`.
///
/// Only a reference recorded inside the Thread's own committed delivery context authorizes a
/// read: a resource id prefix, a digest, or Thread accessibility alone never does. Every
/// delivery is scanned so an id reused with a different reference or modality is rejected
/// instead of resolving to whichever fact appeared first.
///
/// # Errors
/// Rejects a corrupt persisted media projection or a contradictory archived fact for one id.
pub(crate) fn persisted_reference(
    deliveries: &[ToolDelivery],
    attachment_id: &str,
) -> Result<Option<ResourceReference>, ProjectionError> {
    let mut resolved: Option<(ResourceReference, AttachmentModality)> = None;
    for delivery in deliveries {
        for media in archived_media(delivery)? {
            if media.reference.id() != attachment_id {
                continue;
            }
            match &resolved {
                None => resolved = Some((media.reference, media.modality)),
                Some((reference, modality)) => {
                    if reference != &media.reference || modality != &media.modality {
                        return Err(rejected(
                            "conflicting archived tool media share one attachment id",
                        ));
                    }
                }
            }
        }
    }
    Ok(resolved.map(|(reference, _)| reference))
}

/// Reads archived bytes only after proving this Thread itself persisted the reference.
///
/// A resource id prefix, digest, or Thread accessibility never authorizes a read; an unknown,
/// unreferenced, corrupt, or missing resource fails instead of re-reading a workspace path.
///
/// # Errors
/// Rejects an unreferenced or corrupt projection, and unavailable or integrity-failing bytes.
pub(crate) async fn read_persisted_media(
    store: &FileResourceStore,
    deliveries: &[ToolDelivery],
    attachment_id: &str,
) -> anyhow::Result<Vec<u8>> {
    let reference = persisted_reference(deliveries, attachment_id)?.with_context(|| {
        format!(
            "attachment {attachment_id} is not referenced by this Thread's persisted tool media"
        )
    })?;
    let bytes = store
        .read(reference, CancellationToken::new())
        .await
        .with_context(|| format!("archived tool media {attachment_id} is unavailable"))?;
    Ok(bytes.to_vec())
}
