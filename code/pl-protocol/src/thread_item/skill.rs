//! Durable Skill activation timeline payload.

use serde::{Deserialize, Serialize};

use crate::SkillActivation;

/// A successful `skill_view` activation recorded in the Thread timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSkillItem {
    activation: SkillActivation,
}

impl ThreadSkillItem {
    /// Creates the terminal timeline payload from the canonical activation fact.
    pub fn new(activation: SkillActivation) -> Self {
        Self { activation }
    }

    /// Returns the successful Skill activation recorded by this item.
    pub fn activation(&self) -> &SkillActivation {
        &self.activation
    }
}
