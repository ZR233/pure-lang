//! Portable model-visible content. Business payloads never determine message authority.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::OpaquePayload;

/// Content independently projected by a producer from its complete business payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ContextContent {
    Text { text: Arc<str> },
    Resource { reference: ResourceReference },
    Opaque { payload: OpaquePayload },
}

/// Immutable material identity. Physical paths and provider file IDs belong to adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceReference {
    id: String,
    content_digest: String,
    byte_len: u64,
    media_type: String,
}

impl ResourceReference {
    /// Binds a stable resource identity to the exact material retained by its owner.
    ///
    /// # Errors
    /// Rejects empty identifiers, empty media types, or a malformed SHA-256 digest.
    pub fn new(
        id: String,
        content_digest: String,
        byte_len: u64,
        media_type: String,
    ) -> Result<Self, ResourceError> {
        let resource = Self {
            id,
            content_digest,
            byte_len,
            media_type,
        };
        resource.validate()?;
        Ok(resource)
    }

    /// Validates restored metadata without accessing external material.
    ///
    /// # Errors
    /// Returns a missing identity or invalid digest error.
    pub fn validate(&self) -> Result<(), ResourceError> {
        if self.id.is_empty() || self.media_type.is_empty() {
            return Err(ResourceError::MissingIdentity);
        }
        if !self
            .content_digest
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(ResourceError::InvalidDigest);
        }
        Ok(())
    }

    /// Returns the resource owner's lookup identity.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the expected content digest.
    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }

    /// Returns the complete material length, independently of its model projection.
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Returns the declared media type.
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    /// Verifies material before a model adapter consumes it.
    ///
    /// # Errors
    /// Rejects truncated or replaced content, or malformed reference metadata.
    pub fn verify(&self, content: &[u8]) -> Result<(), ResourceError> {
        self.validate()?;
        let digest = format!("sha256:{:x}", Sha256::digest(content));
        if content.len() as u64 != self.byte_len || digest != self.content_digest {
            return Err(ResourceError::ContentMismatch);
        }
        Ok(())
    }
}

/// Resources cannot be restored by guessing paths or accepting different bytes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResourceError {
    #[error("resource identity and media type must not be empty")]
    MissingIdentity,
    #[error("resource requires a SHA-256 content digest")]
    InvalidDigest,
    #[error("resource bytes differ from their recorded length or digest")]
    ContentMismatch,
}

/// Model-message attribution and content. Tools supply content, not the source role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextRecord {
    pub id: String,
    pub turn_id: Option<String>,
    pub source: ContextSource,
    pub content: Vec<ContextContent>,
    /// Framework calls emitted by this assistant record, preserved for protocol replay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<crate::model::ModelToolCall>,
}

/// Framework attribution, without application-specific event kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ContextSource {
    Instruction,
    User,
    Assistant,
    Runtime { source_id: String },
    ToolResult { call_id: String, tool_id: String },
}

/// A frozen model context version, shared without copying content or interpreting payloads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSnapshot {
    pub revision: u64,
    pub records: Arc<[ContextRecord]>,
}

/// Invalid framework relationships in a context replacement or restored snapshot.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContextError {
    #[error("empty or duplicate context record identity: {0}")]
    RecordIdentity(String),
    #[error("tool calls require assistant attribution")]
    InvalidCallSource,
    #[error("empty or duplicate tool call identity: {0}")]
    CallIdentity(String),
    #[error("tool result does not match an outstanding call: {0}")]
    UnmatchedResult(String),
    #[error("context contains unresolved tool calls")]
    PendingCalls,
    #[error("tool-call batch is interrupted by a non-result record: {0}")]
    InterruptedBatch(String),
}

impl ContextSnapshot {
    /// Validates framework relationships without decoding text, resources or opaque payloads.
    ///
    /// # Errors
    /// Rejects identity conflicts, forged call attribution, unmatched results and unfinished pairs.
    pub fn validate_complete(&self) -> Result<(), ContextError> {
        if !self.pending_calls()?.is_empty() {
            return Err(ContextError::PendingCalls);
        }
        Ok(())
    }

    /// Validates record relationships and returns outstanding calls without executing them.
    ///
    /// # Errors
    /// Rejects identity conflicts, invalid call sources and unmatched results.
    pub fn pending_calls(&self) -> Result<Vec<crate::model::ModelToolCall>, ContextError> {
        let mut records = std::collections::BTreeSet::new();
        let mut calls = std::collections::BTreeSet::new();
        let mut pending = std::collections::BTreeMap::new();
        for record in self.records.iter() {
            if !pending.is_empty() && !matches!(record.source, ContextSource::ToolResult { .. }) {
                return Err(ContextError::InterruptedBatch(record.id.clone()));
            }

            if record.id.is_empty() || !records.insert(&record.id) {
                return Err(ContextError::RecordIdentity(record.id.clone()));
            }
            if !record.tool_calls.is_empty() && record.source != ContextSource::Assistant {
                return Err(ContextError::InvalidCallSource);
            }
            for call in &record.tool_calls {
                if call.call_id.is_empty()
                    || call.tool_id.is_empty()
                    || !calls.insert(&call.call_id)
                {
                    return Err(ContextError::CallIdentity(call.call_id.clone()));
                }
                pending.insert(&call.call_id, call);
            }
            if let ContextSource::ToolResult { call_id, tool_id } = &record.source
                && pending.remove(call_id).map(|call| &call.tool_id) != Some(tool_id)
            {
                return Err(ContextError::UnmatchedResult(call_id.clone()));
            }
        }
        Ok(pending.into_values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_pair_validation_uses_envelopes_without_interpreting_dynamic_arguments() {
        let call = crate::model::ModelToolCall {
            call_id: "call".into(),
            tool_id: "tool".into(),
            arguments: OpaquePayload::text("not JSON\0"),
        };
        let assistant = ContextRecord {
            id: "assistant".into(),
            turn_id: Some("turn".into()),
            source: ContextSource::Assistant,
            content: Vec::new(),
            tool_calls: vec![call],
        };
        let result = ContextRecord {
            id: "result".into(),
            turn_id: Some("turn".into()),
            source: ContextSource::ToolResult {
                call_id: "call".into(),
                tool_id: "tool".into(),
            },
            content: vec![ContextContent::Text {
                text: Arc::from("result"),
            }],
            tool_calls: Vec::new(),
        };
        let snapshot = |records: Vec<ContextRecord>| ContextSnapshot {
            revision: 1,
            records: records.into(),
        };
        snapshot(vec![assistant.clone(), result.clone()])
            .validate_complete()
            .unwrap();
        assert!(matches!(
            snapshot(vec![assistant.clone()]).validate_complete(),
            Err(ContextError::PendingCalls)
        ));
        assert!(matches!(
            snapshot(vec![result]).validate_complete(),
            Err(ContextError::UnmatchedResult(_))
        ));
        let mut forged = assistant;
        forged.source = ContextSource::User;
        assert!(matches!(
            snapshot(vec![forged]).validate_complete(),
            Err(ContextError::InvalidCallSource)
        ));
    }
}
