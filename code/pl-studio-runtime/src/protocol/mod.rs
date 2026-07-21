mod event;
mod runtime;

pub use event::*;
pub use runtime::*;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn studio_product_event_kind_fields_are_camel_case() {
        assert_eq!(
            serde_json::to_value(StudioProductEventKind::SessionListChanged {
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
