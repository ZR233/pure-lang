#[path = "runtime.rs"]
mod bridge_runtime;
mod convert;
pub mod handlers;
pub mod subscription;
pub mod types;

// Re-exports from submodules
pub use self::handlers::{
    BridgeStudioUpdateOperation, apply_task_recovery, archive_project, archive_thread,
    bootstrap_studio, check_studio_update, cleanup_project, cleanup_recovery_issue, create_thread,
    init_app, initialize_runtime, install_studio_update, interrupt_turn, list_discovered_skills,
    list_thread_turns, list_threads, load_provider_catalog, load_provider_usages,
    load_web_search_settings, open_project, preview_project_cleanup,
    preview_recovery_issue_cleanup, preview_task_recovery, read_thread, respond_interaction,
    save_general_settings, save_instructions_settings, save_mcp_settings, save_provider_settings,
    save_runtime_permission_mode, save_skills_settings, save_web_search_settings, select_project,
    set_model_role, set_thread_mode, shutdown_runtime, start_runtime, start_turn, steer_turn,
};
pub use self::subscription::{
    BridgeEventSubscription, BridgeProductStreamEnvelope, BridgeThreadStreamEnvelope,
    create_product_subscription, subscribe_thread,
};
pub use self::types::*;

#[cfg(test)]
mod tests {
    use pl_studio_runtime::McpServerTransport;
    use pl_studio_runtime::StudioProductEventKind;
    use pretty_assertions::assert_eq;

    use super::BridgeProductEventPayload;

    #[test]
    fn bridge_product_event_uses_typed_payload() {
        let event = pl_studio_runtime::StudioProductEventEnvelope {
            event_id: "event-1".to_string(),
            project_id: None,
            sequence: 7,
            created_at: 10,
            kind: StudioProductEventKind::ThreadDirectoryChanged {
                project_id: "project-1".to_string(),
                threads: Vec::new(),
            },
        };

        let envelope = super::convert::event::bridge_product_event(event);

        assert_eq!(envelope.sequence, 7);
        assert_eq!(
            envelope.payload,
            BridgeProductEventPayload::ThreadDirectoryChanged {
                project_id: "project-1".to_string(),
                threads: Vec::new(),
            }
        );
    }

    #[test]
    fn provider_catalog_bridge_projects_the_canonical_pl_snapshot() {
        let canonical = pl_studio_runtime::builtin_provider_catalog()
            .snapshot()
            .unwrap();
        let bridge = super::load_provider_catalog().unwrap();

        assert_eq!(bridge.schema_version, canonical.schema_version);
        assert_eq!(bridge.revision, canonical.revision);
        assert_eq!(
            bridge
                .presets
                .iter()
                .map(|preset| preset.id.as_str())
                .collect::<Vec<_>>(),
            canonical
                .presets
                .iter()
                .map(|preset| preset.id.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            bridge
                .presets
                .iter()
                .map(|preset| (
                    preset.service_capabilities.web_search.hosted_responses,
                    preset.service_capabilities.web_search.standalone.as_deref(),
                    preset.service_capabilities.responses_tool_search,
                    preset
                        .service_capabilities
                        .responses_programmatic_tool_calling,
                ))
                .collect::<Vec<_>>(),
            canonical
                .presets
                .iter()
                .map(|preset| (
                    preset.service_capabilities.web_search.hosted_responses,
                    preset.service_capabilities.web_search.standalone.as_deref(),
                    preset.service_capabilities.responses_tool_search,
                    preset
                        .service_capabilities
                        .responses_programmatic_tool_calling,
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            bridge
                .model_catalogs
                .iter()
                .flat_map(|catalog| catalog.models.iter())
                .map(|model| (
                    model.transport.protocol.as_str(),
                    model
                        .transport
                        .connection_modes
                        .iter()
                        .map(|mode| mode.id.as_str())
                        .collect::<Vec<_>>(),
                    model.transport.default_connection_mode.as_str(),
                ))
                .collect::<Vec<_>>(),
            canonical
                .model_catalogs
                .values()
                .flat_map(|catalog| catalog.models.iter())
                .map(|model| (
                    model.transport.protocol.as_str(),
                    model
                        .transport
                        .connection_modes
                        .iter()
                        .map(|mode| mode.id.as_str())
                        .collect::<Vec<_>>(),
                    model.transport.default_connection_mode.as_str(),
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            bridge
                .model_catalogs
                .iter()
                .map(|catalog| (
                    catalog.id.as_str(),
                    catalog
                        .models
                        .iter()
                        .map(|model| model.id.as_str())
                        .collect::<Vec<_>>(),
                ))
                .collect::<Vec<_>>(),
            canonical
                .model_catalogs
                .values()
                .map(|catalog| (
                    catalog.id.as_str(),
                    catalog
                        .models
                        .iter()
                        .map(|model| model.id.as_str())
                        .collect::<Vec<_>>(),
                ))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn mcp_transport_label_accepts_only_canonical_ui_values() {
        assert_eq!(
            super::convert::settings::mcp_transport_from_label("streamableHttp").unwrap(),
            McpServerTransport::StreamableHttp
        );
        assert_eq!(
            super::convert::settings::mcp_transport_from_label("stdio").unwrap(),
            McpServerTransport::Stdio
        );
        assert!(super::convert::settings::mcp_transport_from_label("streamable_http").is_err());
    }
}
