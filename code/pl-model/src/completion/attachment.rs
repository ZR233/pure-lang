//! Caller-owned attachment sources, independent of provider wire representations.
use super::AttachmentModality;
pub use pl_core::context::{ResourceAccess, ResourceReference};
use std::sync::Arc;

#[derive(Clone)]
pub struct AttachmentInput {
    pub attachment_id: String,
    pub modality: AttachmentModality,
    pub media_type: String,
    pub filename: Option<String>,
    pub source: AttachmentSource,
}

#[derive(Clone)]
pub enum AttachmentSource {
    Url {
        url: String,
    },
    Base64 {
        base64: String,
    },
    Bytes {
        bytes: Arc<[u8]>,
    },
    Resource {
        reference: ResourceReference,
        access: ResourceAccess,
    },
}

impl std::fmt::Debug for AttachmentInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttachmentInput")
            .field("attachment_id", &self.attachment_id)
            .field("modality", &self.modality)
            .field("media_type", &self.media_type)
            .finish_non_exhaustive()
    }
}
