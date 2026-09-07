//! Versioned, product-independent session storage records.

use serde::{Deserialize, Serialize};

/// Immutable envelope returned by the session owner. Payload interpretation belongs to its type owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub session_id: String,
    pub id: String,
    pub turn_id: Option<String>,
    pub type_id: String,
    pub schema_version: u32,
    pub ordinal: u64,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub payload: serde_json::Value,
}
