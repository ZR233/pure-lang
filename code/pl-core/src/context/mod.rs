//! Thread-owned context semantics and producer-owned opaque content.

mod content;
mod payload;

pub use content::{
    ContextContent, ContextError, ContextRecord, ContextSnapshot, ContextSource, ResourceError,
    ResourceReference,
};
pub use payload::{OpaquePayload, PayloadError};

mod resource;
pub use resource::{ResourceAccess, ResourceReadError, ResourceReader};

mod inheritance;
pub use inheritance::{ContextInheritance, HistoryInheritance, InstructionInheritance};

/// Stable identity of actual UTF-8 or binary content, without interpreting its format.
pub fn content_hash(content: &[u8]) -> String {
    use sha2::Digest;
    format!("sha256:{:x}", sha2::Sha256::digest(content))
}
