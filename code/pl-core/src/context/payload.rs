//! Provider-independent content carried by a Thread. Payload owners interpret their own formats.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Immutable UTF-8 content whose encoding and semantics belong to its producer.
///
/// The framework never parses the content. JSON, plain text, and custom tool grammars retain
/// their original bytes, including whitespace, number spelling, and embedded NUL characters.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    try_from = "PayloadEnvelope",
    into = "PayloadEnvelope"
)]
pub struct OpaquePayload {
    format: Arc<str>,
    version: u32,
    content: Arc<str>,
}

impl OpaquePayload {
    /// Freezes producer-owned content without parsing or normalizing it.
    ///
    /// # Errors
    /// Rejects an empty format identifier or a zero schema version. Unknown formats and
    /// versions are accepted; compatibility is a decision for the content owner.
    pub fn new(
        format: impl Into<Arc<str>>,
        version: u32,
        content: impl Into<Arc<str>>,
    ) -> Result<Self, PayloadError> {
        let format = format.into();
        if format.is_empty() {
            return Err(PayloadError::MissingFormat);
        }
        if version == 0 {
            return Err(PayloadError::MissingVersion);
        }
        Ok(Self {
            format,
            version,
            content: content.into(),
        })
    }

    /// Freezes unstructured text without normalization or parsing.
    pub fn text(content: impl Into<Arc<str>>) -> Self {
        Self {
            format: Arc::from("text/plain"),
            version: 1,
            content: content.into(),
        }
    }

    /// Returns the producer-defined encoding identifier.
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Returns the producer-defined schema version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Returns the exact content supplied by the producer.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns the number of UTF-8 bytes, not characters or serialized-envelope bytes.
    pub fn byte_len(&self) -> usize {
        self.content.len()
    }

    /// Returns a digest of the exact content bytes; format and version remain separate identity.
    pub fn content_digest(&self) -> String {
        format!("sha256:{:x}", Sha256::digest(self.content.as_bytes()))
    }
}

impl fmt::Debug for OpaquePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpaquePayload")
            .field("format", &self.format)
            .field("version", &self.version)
            .field("bytes", &self.byte_len())
            .finish_non_exhaustive()
    }
}

/// Invalid framework envelope metadata. Content syntax is deliberately not validated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PayloadError {
    #[error("opaque payload format must not be empty")]
    MissingFormat,
    #[error("opaque payload version must not be zero")]
    MissingVersion,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayloadEnvelope {
    format: Arc<str>,
    version: u32,
    content: Arc<str>,
}

impl TryFrom<PayloadEnvelope> for OpaquePayload {
    type Error = PayloadError;

    fn try_from(envelope: PayloadEnvelope) -> Result<Self, Self::Error> {
        Self::new(envelope.format, envelope.version, envelope.content)
    }
}

impl From<OpaquePayload> for PayloadEnvelope {
    fn from(payload: OpaquePayload) -> Self {
        Self {
            format: payload.format,
            version: payload.version,
            content: payload.content,
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn unknown_payloads_round_trip_without_interpreting_content() {
        for content in [
            "",
            "not JSON\r\n\0中文🦀",
            "{ \"z\": 90071992547409931234567890, \"a\": 1e+03 }\n",
            "{\"approved\":true,\"endTurn\":true}",
        ] {
            let payload = OpaquePayload::new("third-party/future", 123, content).unwrap();
            let encoded = serde_json::to_string(&payload).unwrap();
            let restored: OpaquePayload = serde_json::from_str(&encoded).unwrap();
            assert_eq!(restored, payload);
            assert_eq!(restored.content().as_bytes(), content.as_bytes());
            assert_eq!(restored.byte_len(), content.len());
        }
    }

    #[test]
    fn byte_digests_distinguish_equivalent_json_spellings() {
        let compact = OpaquePayload::new("application/json", 1, "{\"a\":1}").unwrap();
        let spaced = OpaquePayload::new("application/json", 1, "{ \"a\": 1 }").unwrap();
        assert_ne!(compact.content_digest(), spaced.content_digest());
        assert_eq!(compact.clone().content_digest(), compact.content_digest());
    }

    #[test]
    fn decoding_validates_envelope_without_exposing_content_in_debug() {
        assert!(
            serde_json::from_str::<OpaquePayload>(
                r#"{"format":"","version":1,"content":"secret"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<OpaquePayload>(
                r#"{"format":"text/plain","version":0,"content":"secret"}"#
            )
            .is_err()
        );
        let payload = OpaquePayload::new("text/plain", 1, "private body").unwrap();
        assert!(!format!("{payload:?}").contains("private body"));
    }
}
