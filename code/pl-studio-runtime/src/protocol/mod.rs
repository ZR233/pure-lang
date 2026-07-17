mod event;
mod message;
mod runtime;

pub use event::*;
pub use message::*;
pub use runtime::*;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn studio_turn_status_is_camel_case() {
        assert_eq!(
            serde_json::to_value(StudioTurnStatus::WaitingForModel).unwrap(),
            serde_json::json!("waitingForModel")
        );
    }

    #[test]
    fn studio_part_delta_field_allows_dotted_tool_paths() {
        assert_eq!(
            serde_json::to_value(StudioPartDeltaField::ToolArguments).unwrap(),
            serde_json::json!("tool.arguments")
        );
    }

    #[test]
    fn studio_event_kind_fields_are_camel_case() {
        assert_eq!(
            serde_json::to_value(StudioEventKind::Stale { lagged_events: 2 }).unwrap(),
            serde_json::json!({
                "type": "stale",
                "laggedEvents": 2
            })
        );
        assert_eq!(
            serde_json::to_value(StudioEventKind::SessionListChanged {
                project_id: "project-1".to_string(),
                sessions: Vec::new()
            })
            .unwrap(),
            serde_json::json!({
                "type": "sessionListChanged",
                "projectId": "project-1",
                "sessions": []
            })
        );
    }
}
