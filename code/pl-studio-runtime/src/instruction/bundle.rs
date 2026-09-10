//! Already rendered instructions supplied by a host. No files, profiles, or catalogs are read here.

use pl_protocol::Message;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Frozen instruction text and explicitly attributed prelude/turn messages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionBundle {
    pub instructions: String,
    pub prelude_messages: Vec<Message>,
    pub turn_messages: Vec<Message>,
    pub prefix_section_hashes: BTreeMap<String, String>,
}

impl InstructionBundle {
    /// Creates a minimal instruction bundle without product defaults.
    pub fn new(instructions: impl Into<String>) -> Self {
        Self {
            instructions: instructions.into(),
            ..Self::default()
        }
    }
}
