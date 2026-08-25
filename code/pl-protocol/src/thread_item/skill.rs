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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_item_round_trips_the_full_activation_fact() {
        let state = crate::ThreadItemState::Skill(ThreadSkillItem::new(SkillActivation {
            name: "pdf".to_string(),
            source: "system".to_string(),
            provider_id: "local-filesystem".to_string(),
            resource_base: crate::SkillActivationResourceBase::Directory {
                path: "/skills/pdf".to_string(),
            },
            turn_id: "turn-1".to_string(),
            cause: crate::SkillActivationCause::Tool {
                tool_call_id: "tool-1".to_string(),
            },
            activated_at: 7,
        }));

        let json = serde_json::to_string(&state).unwrap();
        let restored: crate::ThreadItemState = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, state);
        assert!(json.contains(r#""kind":"skill""#));
    }

    #[test]
    fn legacy_skill_item_maps_path_and_tool_call_to_typed_fields() {
        let activation: SkillActivation = serde_json::from_value(serde_json::json!({
            "name": "pdf",
            "source": "system",
            "path": "/skills/pdf",
            "turnId": "turn-1",
            "toolCallId": "tool-1",
            "activatedAt": 7
        }))
        .unwrap();

        assert_eq!(activation.directory_path(), Some("/skills/pdf"));
        assert!(matches!(
            activation.cause,
            crate::SkillActivationCause::Tool { ref tool_call_id } if tool_call_id == "tool-1"
        ));
    }
}
