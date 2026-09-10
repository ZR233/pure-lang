//! Shared producer schema for model capabilities frozen for a tool invocation.
use serde::{Deserialize, Serialize};

/// Namespace of the model-produced opaque tool projection material.
pub const FORMAT: &str = "pl.model.tool-projection";
/// Current producer schema; consumers reject unsupported versions explicitly.
pub const VERSION: u32 = 1;

/// Only advertised capabilities may be used to embed media in a tool result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProjection {
    pub image: Option<ImageProjection>,
}

/// Provider-independent image bounds selected by the prepared model adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageProjection {
    pub max_count: Option<u32>,
    pub max_bytes: Option<u64>,
    pub max_total_bytes: Option<u64>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub media_types: Vec<String>,
}
