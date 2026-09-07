//! Owner-mediated mutations of extensible session records.

use std::collections::BTreeMap;

pub use pl_protocol::SessionEntry;
use serde::{Serialize, de::DeserializeOwned};
pub use serde_json::Value as SessionValue;
pub use serde_json::{Error as SessionCodecError, Map as SessionObject, Number as SessionNumber};

/// Default maximum serialized payload size for an extension record.
pub const DEFAULT_ENTRY_MAX_BYTES: usize = 1024 * 1024;

/// A typed payload declares a stable namespaced identity independently of its Rust name.
pub trait SessionEntryPayload: Serialize + DeserializeOwned {
    const TYPE_ID: &'static str;
    const SCHEMA_VERSION: u32;
}

/// A requested mutation. Envelope identity and revisions are assigned by the owner.
#[derive(Debug, Clone)]
pub enum SessionEntryMutation {
    Put {
        id: String,
        expected_revision: Option<u64>,
        type_id: String,
        schema_version: u32,
        payload: SessionValue,
    },
    Delete {
        id: String,
        expected_revision: u64,
    },
}

impl SessionEntryMutation {
    /// Builds a typed insert (`None`) or compare-and-replace (`Some(revision)`).
    ///
    /// # Errors
    /// Returns an encoding error if the payload cannot be serialized.
    pub fn put<T: SessionEntryPayload>(
        id: impl Into<String>,
        expected_revision: Option<u64>,
        payload: &T,
    ) -> Result<Self, SessionEntryError> {
        Ok(Self::Put {
            id: id.into(),
            expected_revision,
            type_id: T::TYPE_ID.to_owned(),
            schema_version: T::SCHEMA_VERSION,
            payload: serde_json::to_value(payload)?,
        })
    }
}

/// Errors are distinguishable without parsing diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum SessionEntryError {
    #[error("invalid or reserved session entry identity: {0}")]
    InvalidIdentity(String),
    #[error("session entry {id} revision conflict: expected {expected:?}, actual {actual:?}")]
    Conflict {
        id: String,
        expected: Option<u64>,
        actual: Option<u64>,
    },
    #[error("session entry payload exceeds {limit} bytes")]
    TooLarge { limit: usize },
    #[error("session entry type/version mismatch: {type_id} v{version}")]
    TypeMismatch { type_id: String, version: u32 },
    #[error("session entry encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("session entry revision exhausted")]
    RevisionExhausted,
    #[error("session entry owner has not been bound")]
    Unbound,
}

/// Decodes only the declared type and version, preserving unknown payloads at the storage boundary.
///
/// # Errors
/// Returns a type/version mismatch or a payload decoding error.
pub fn decode_entry<T: SessionEntryPayload>(entry: &SessionEntry) -> Result<T, SessionEntryError> {
    if entry.type_id != T::TYPE_ID || entry.schema_version != T::SCHEMA_VERSION {
        return Err(SessionEntryError::TypeMismatch {
            type_id: entry.type_id.clone(),
            version: entry.schema_version,
        });
    }
    Ok(serde_json::from_value(entry.payload.clone())?)
}

pub(crate) fn validate_mutations(
    mutations: &[SessionEntryMutation],
    limit: usize,
) -> Result<(), SessionEntryError> {
    for mutation in mutations {
        let id = match mutation {
            SessionEntryMutation::Put {
                id,
                type_id,
                schema_version,
                payload,
                ..
            } => {
                if !type_id.contains('.') || type_id.starts_with("pl.") || *schema_version == 0 {
                    return Err(SessionEntryError::InvalidIdentity(type_id.clone()));
                }
                if serde_json::to_vec(payload)?.len() > limit {
                    return Err(SessionEntryError::TooLarge { limit });
                }
                id
            }
            SessionEntryMutation::Delete { id, .. } => id,
        };
        if id.is_empty() || id.starts_with("pl.") {
            return Err(SessionEntryError::InvalidIdentity(id.clone()));
        }
    }
    Ok(())
}

pub(crate) fn apply_mutations(
    entries: &mut BTreeMap<String, SessionEntry>,
    sequence: &mut u64,
    scope: (&str, Option<&str>, i64),
    mutations: Vec<SessionEntryMutation>,
) -> Result<(), SessionEntryError> {
    let (session_id, turn_id, now) = scope;
    for mutation in mutations {
        let (id, expected) = match &mutation {
            SessionEntryMutation::Put {
                id,
                expected_revision,
                ..
            } => (id, *expected_revision),
            SessionEntryMutation::Delete {
                id,
                expected_revision,
            } => (id, Some(*expected_revision)),
        };
        let previous = entries.get(id);
        let actual = previous.map(|entry| entry.revision);
        if actual != expected {
            return Err(SessionEntryError::Conflict {
                id: id.clone(),
                expected,
                actual,
            });
        }
        *sequence = sequence
            .checked_add(1)
            .ok_or(SessionEntryError::RevisionExhausted)?;
        match mutation {
            SessionEntryMutation::Put {
                id,
                type_id,
                schema_version,
                payload,
                ..
            } => {
                let ordinal = previous.map_or(*sequence, |entry| entry.ordinal);
                let created_at = previous.map_or(now, |entry| entry.created_at);
                entries.insert(
                    id.clone(),
                    SessionEntry {
                        session_id: session_id.to_owned(),
                        id,
                        turn_id: turn_id.map(str::to_owned),
                        type_id,
                        schema_version,
                        ordinal,
                        revision: *sequence,
                        created_at,
                        updated_at: now,
                        payload,
                    },
                );
            }
            SessionEntryMutation::Delete { id, .. } => {
                entries.remove(&id);
            }
        }
    }
    Ok(())
}
