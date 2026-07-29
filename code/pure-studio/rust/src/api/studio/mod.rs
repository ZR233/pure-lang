mod convert;
pub mod handlers;
mod runtime;
pub mod subscription;
pub mod types;

// Re-exports from submodules
pub use self::handlers::{
    BridgeStudioUpdateOperation, archive_project, archive_session, bootstrap_studio,
    check_studio_update, cleanup_project, cleanup_recovery_issue, create_session, init_app,
    initialize_runtime, install_studio_update, list_discovered_skills, load_provider_catalog,
    load_provider_usages, load_web_search_settings, open_project, preview_project_cleanup,
    preview_recovery_issue_cleanup, resolve_interaction, save_general_settings,
    save_instructions_settings, save_mcp_settings, save_provider_settings,
    save_runtime_permission_mode, save_skills_settings, save_web_search_settings, select_project,
    set_model_role, set_session_mode, shutdown_runtime, start_runtime, stop_prompt, submit_prompt,
};
pub use self::subscription::{
    BridgeEventSubscription, BridgeProductStreamEnvelope, BridgeSessionStreamEnvelope,
    create_product_subscription, create_session_subscription,
};
pub use self::types::{
    BridgeActiveTurn, BridgeError, BridgeErrorCode, BridgeInteractionChangedDto,
    BridgeInteractionPayloadDto, BridgeLspHealthDto, BridgeMcpHealthDto, BridgeMcpServerDto,
    BridgeModelCapabilities, BridgeModelCatalogDescriptor, BridgeModelDescriptor,
    BridgeModelPricing, BridgeModelReasoningDescriptor, BridgeProductEventEnvelope,
    BridgeProductEventPayload, BridgeProviderCatalogSnapshot,
    BridgeProviderConnectionModeDescriptor, BridgeProviderPresetDescriptor,
    BridgeProviderServiceCapabilitiesDescriptor, BridgeRecoveryCleanupPreviewDto,
    BridgeRecoveryCleanupResourceDto, BridgeRecoveryIssueAction, BridgeRecoveryIssueCategory,
    BridgeRecoveryIssueScope, BridgeRecoveryResourcePresence, BridgeRuntimeStatus,
    BridgeSessionStreamFrame, BridgeStudioRecoveryIssueDto, BridgeStudioSnapshotResponse,
    BridgeStudioUpdateCheckDto, BridgeStudioUpdateDto, BridgeStudioUpdateEventDto,
    BridgeUserQuestionDto, BridgeUserQuestionOptionDto,
    BridgeWebSearchProviderCapabilitiesDescriptor, BridgeWebSearchSettingsDto, DeepSeekBalanceDto,
    DeepSeekBalanceInfoDto, GeneralSettingsInput, InstructionsSettingsInput, McpServerInput,
    McpSettingsInput, ProjectDto, ProviderInput, ProviderModelInput, ProviderSecretInput,
    ProviderSettingsInput, ProviderUsageDto, ProviderUsagesResponse, ResolveInteractionResponse,
    RoleInput, RuntimeSnapshot, SessionDto, SkillSummaryDto, SkillsResponse, SkillsSettingsInput,
    StopPromptResponse, SubmitPromptResponse, WebSearchSettingsInput, ZhipuCodingPlanUsageDto,
    ZhipuQuotaLimitDto, ZhipuToolUsageDetailDto,
};

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
            kind: StudioProductEventKind::SessionListChanged {
                project_id: "project-1".to_string(),
                sessions: Vec::new(),
            },
        };

        let envelope = super::convert::event::bridge_product_event(event);

        assert_eq!(envelope.sequence, 7);
        assert_eq!(
            envelope.payload,
            BridgeProductEventPayload::SessionListChanged {
                project_id: "project-1".to_string(),
                sessions: Vec::new(),
            }
        );
    }

    #[test]
    fn archive_project_api_is_exposed_to_flutter() {
        let _archive = super::archive_project;
        let _preview = super::preview_project_cleanup;
        let _cleanup = super::cleanup_project;
    }

    #[test]
    fn list_discovered_skills_api_is_exposed_to_flutter() {
        let _api = super::list_discovered_skills;
    }

    #[test]
    fn small_command_responses_are_typed_for_flutter() {
        let _runtime_permission = super::save_runtime_permission_mode;
        let _provider_usages = super::load_provider_usages;
        let _submit = super::submit_prompt;
        let _stop = super::stop_prompt;
        let _resolve = super::resolve_interaction;
    }

    #[test]
    fn typed_settings_apis_are_exposed_to_flutter() {
        let _instructions = super::save_instructions_settings;
        let _skills = super::save_skills_settings;
        let _mcp = super::save_mcp_settings;
        let _general = super::save_general_settings;
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
                ))
                .collect::<Vec<_>>(),
            canonical
                .presets
                .iter()
                .map(|preset| (
                    preset.service_capabilities.web_search.hosted_responses,
                    preset.service_capabilities.web_search.standalone.as_deref(),
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            bridge
                .presets
                .iter()
                .map(|preset| (
                    preset.transport.protocol.as_str(),
                    preset
                        .transport
                        .connection_modes
                        .iter()
                        .map(|mode| mode.id.as_str())
                        .collect::<Vec<_>>(),
                    preset.transport.default_connection_mode.as_str(),
                ))
                .collect::<Vec<_>>(),
            canonical
                .presets
                .iter()
                .map(|preset| (
                    preset.transport.protocol.as_str(),
                    preset
                        .transport
                        .connection_modes
                        .iter()
                        .map(|mode| mode.id.as_str())
                        .collect::<Vec<_>>(),
                    preset.transport.default_connection_mode.as_str(),
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
    fn mcp_transport_label_accepts_ui_values() {
        assert_eq!(
            super::convert::settings::mcp_transport_from_label("streamableHttp"),
            McpServerTransport::StreamableHttp
        );
        assert_eq!(
            super::convert::settings::mcp_transport_from_label("streamable_http"),
            McpServerTransport::StreamableHttp
        );
        assert_eq!(
            super::convert::settings::mcp_transport_from_label("http"),
            McpServerTransport::StreamableHttp
        );
        assert_eq!(
            super::convert::settings::mcp_transport_from_label("stdio"),
            McpServerTransport::Stdio
        );
    }

    #[test]
    fn normalized_string_list_trims_sorts_and_deduplicates() {
        assert_eq!(
            super::convert::settings::normalized_string_list(vec![
                " beta ".to_string(),
                String::new(),
                "alpha".to_string(),
                "beta".to_string(),
            ]),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }
}
