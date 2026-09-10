//! Shared exact-byte media retention and frozen model projection for tools.
pub use pl_core::context::{ContextContent, ResourceReference};
use pl_core::{context::OpaquePayload, tool::opaque::ToolError};
use std::sync::Arc;

/// Media semantics supplied by tools, without provider protocols or message authority.
#[derive(Debug, Clone, Copy)]
pub enum ToolMediaKind {
    Image,
    Audio,
    Blob,
}

/// Exact received bytes. The resource host must retain them before returning a reference.
#[derive(Debug)]
pub struct ToolMedia {
    pub kind: ToolMediaKind,
    pub bytes: Arc<[u8]>,
    pub media_type: String,
    pub model_image: Option<PreparedToolImage>,
}

/// An image normalized under the originating prepared model's advertised bounds.
#[derive(Debug)]
pub struct PreparedToolImage {
    pub bytes: Arc<[u8]>,
    pub media_type: String,
}

/// Durable material and the actual model-visible media projection selected by the host.
#[derive(Debug)]
pub struct RetainedToolMedia {
    pub reference: ResourceReference,
    pub context: Vec<ContextContent>,
}

/// The host supplies persistent resources and model-owned projection as one preparation port.
pub trait ToolMediaHost: Send + Sync + std::fmt::Debug + 'static {
    /// Returns only after bytes are retained. Failure must not imply a remote call was undone.
    fn retain(
        &self,
        media: ToolMedia,
    ) -> impl std::future::Future<Output = Result<RetainedToolMedia, ToolError>> + Send;
}

pub(crate) fn image_limits(
    projection: Option<&OpaquePayload>,
) -> Result<Option<crate::image::ImageOutputLimits>, ToolError> {
    let Some(projection) = projection else {
        return Ok(None);
    };
    if projection.format() != pl_protocol::tool_projection::FORMAT
        || projection.version() != pl_protocol::tool_projection::VERSION
    {
        return Err(pl_core::tool::opaque::ToolError::new(
            std::io::Error::other("unknown model tool projection format"),
        ));
    }
    let projection: pl_protocol::tool_projection::ToolProjection =
        serde_json::from_str(projection.content()).map_err(ToolError::new)?;
    Ok(projection
        .image
        .map(|image| crate::image::ImageOutputLimits {
            max_count: image.max_count,
            max_bytes: image.max_bytes,
            max_total_bytes: image.max_total_bytes,
            max_width: image.max_width,
            max_height: image.max_height,
            media_types: image.media_types,
        }))
}
