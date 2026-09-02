#[path = "runtime.rs"]
mod bridge_runtime;
mod convert;
pub mod handlers;
pub mod subscription;
pub mod types;

// Re-exports from submodules
pub use self::handlers::{
    BridgeStudioUpdateOperation, activate_project, admit_attachment_drafts, archive_project,
    archive_thread, browse_remote_directories, check_provider_usage, check_studio_update,
    delete_ssh_server, discover_skills, init_app, install_studio_update, interrupt_turn,
    list_ssh_servers, list_thread_turns, list_threads_page, load_provider_catalog, open_project,
    open_remote_project, probe_lsp_server, read_agent_profiles, read_attachment_draft,
    read_deepseek_web_search_settings, read_lsp_state, read_mcp_state, read_provider_usage_state,
    read_settings_state, read_skills_state, read_studio_state, read_studio_update_state,
    read_thread, read_thread_attachment, read_web_search_settings, reload_settings_from_disk,
    remove_attachment_draft, rename_thread, repair_lsp_server, reset_lsp, reset_mcp,
    respond_interaction, retry_persistence, save_deepseek_web_search_settings,
    save_general_settings, save_instructions_settings, save_mcp_settings, save_provider_settings,
    save_runtime_permission_mode, save_skills_settings, save_ssh_server, save_user_agent_profile,
    save_web_search_settings, search_skills, set_model_role, set_system_agent_enabled,
    set_thread_mode, shutdown_runtime, start_new_thread, start_studio_runtime, start_turn,
    steer_turn, test_ssh_connection,
};
pub use self::subscription::{
    BridgeEventSubscription, BridgeProductStreamEnvelope, BridgeThreadStreamEnvelope,
    create_product_subscription, subscribe_thread,
};
pub use self::types::*;

#[cfg(test)]
mod tests {
    use pl_studio_runtime::StudioProductEventKind;
    use pretty_assertions::assert_eq;

    use super::BridgeProductEventPayload;

    #[test]
    fn frb_exports_cover_every_shared_studio_operation() {
        use pl_protocol::studio::StudioOperation;

        macro_rules! exported {
            ($($operation:ident => $symbol:ident),+ $(,)?) => {{
                let operations = [$(StudioOperation::$operation),+];
                $(let _ = super::$symbol;)+
                operations
            }};
        }

        let operations = exported![
            ReadState => read_studio_state,
            OpenProject => open_project,
            ActivateProject => activate_project,
            ArchiveProject => archive_project,
            ListThreadsPage => list_threads_page,
            StartNewThread => start_new_thread,
            ReadThread => read_thread,
            ArchiveThread => archive_thread,
            RenameThread => rename_thread,
            SetThreadMode => set_thread_mode,
            ListThreadTurns => list_thread_turns,
            StartTurn => start_turn,
            SteerTurn => steer_turn,
            InterruptTurn => interrupt_turn,
            AdmitAttachmentDrafts => admit_attachment_drafts,
            UploadAttachmentDrafts => admit_attachment_drafts,
            RemoveAttachmentDraft => remove_attachment_draft,
            ReadAttachmentDraft => read_attachment_draft,
            ReadThreadAttachment => read_thread_attachment,
            RespondInteraction => respond_interaction,
            LoadProviderCatalog => load_provider_catalog,
            ReadSettings => read_settings_state,
            ReloadSettings => reload_settings_from_disk,
            SaveWebSearchSettings => save_web_search_settings,
            SaveDeepSeekWebSearchSettings => save_deepseek_web_search_settings,
            SavePermissionSettings => save_runtime_permission_mode,
            SaveProviderSettings => save_provider_settings,
            SaveInstructionsSettings => save_instructions_settings,
            SaveSkillsSettings => save_skills_settings,
            SaveMcpSettings => save_mcp_settings,
            SaveGeneralSettings => save_general_settings,
            SetModelRole => set_model_role,
            ReadProviderUsage => read_provider_usage_state,
            CheckProviderUsage => check_provider_usage,
            ReadSkills => read_skills_state,
            DiscoverSkills => discover_skills,
            SearchSkills => search_skills,
            ReadMcp => read_mcp_state,
            ResetMcp => reset_mcp,
            ReadLsp => read_lsp_state,
            ProbeLsp => probe_lsp_server,
            RepairLsp => repair_lsp_server,
            ResetLsp => reset_lsp,
            ReadUpdate => read_studio_update_state,
            CheckUpdate => check_studio_update,
            RetryPersistence => retry_persistence,
            SubscribeProduct => create_product_subscription,
            SubscribeThread => subscribe_thread,
        ];

        assert_eq!(operations, StudioOperation::ALL);
    }

    #[test]
    fn bridge_product_event_uses_typed_payload() {
        let event = pl_studio_runtime::StudioProductEventEnvelope {
            event_id: "event-1".to_string(),
            sequence: 7,
            created_at: 10,
            kind: StudioProductEventKind::ThreadDirectoryChanged(
                pl_studio_runtime::StudioThreadDirectoryDelta {
                    revision: 3,
                    updated_at: 10,
                    upserted: Vec::new(),
                    removed: Vec::new(),
                },
            ),
        };

        let envelope = super::convert::event::bridge_product_event(event).unwrap();

        assert_eq!(envelope.sequence, 7);
        assert_eq!(
            envelope.payload,
            BridgeProductEventPayload::ThreadDirectoryChanged(super::BridgeThreadDirectoryDelta {
                revision: 3,
                updated_at: 10,
                upserted: Vec::new(),
                removed: Vec::new(),
            })
        );
    }

    #[test]
    fn provider_catalog_bridge_projects_the_canonical_pl_snapshot() {
        let canonical = pl_core::builtin_provider_catalog().snapshot().unwrap();
        let bridge = super::BridgeProviderCatalogSnapshot::from(canonical.clone());

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
                    preset
                        .service_capabilities
                        .web_search
                        .hosted_dialect
                        .as_str(),
                    preset.service_capabilities.web_search.standalone.as_deref(),
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
                    preset
                        .service_capabilities
                        .web_search
                        .hosted_dialect
                        .as_str(),
                    preset.service_capabilities.web_search.standalone.as_deref(),
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
}
