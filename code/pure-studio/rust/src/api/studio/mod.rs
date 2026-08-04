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
    load_provider_usages, load_session_history_page, load_web_search_settings, open_project,
    preview_project_cleanup, preview_recovery_issue_cleanup, resolve_interaction, resume_task,
    save_general_settings, save_instructions_settings, save_mcp_settings, save_provider_settings,
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
    BridgeSessionHistoryItem, BridgeSessionHistoryTurn, BridgeSessionStreamFrame,
    BridgeStudioRecoveryIssueDto, BridgeStudioSnapshotResponse, BridgeStudioUpdateCheckDto,
    BridgeStudioUpdateDto, BridgeStudioUpdateEventDto, BridgeUserQuestionDto,
    BridgeUserQuestionOptionDto, BridgeWebSearchProviderCapabilitiesDescriptor,
    BridgeWebSearchSettingsDto, DeepSeekBalanceDto, DeepSeekBalanceInfoDto, GeneralSettingsInput,
    InstructionsSettingsInput, LoadSessionHistoryPageRequest, LoadSessionHistoryPageResponse,
    McpServerInput, McpSettingsInput, ProjectDto, ProviderInput, ProviderModelInput,
    ProviderSecretInput, ProviderSettingsInput, ProviderUsageDto, ProviderUsagesResponse,
    ResolveInteractionResponse, RoleInput, RuntimeSnapshot, SessionDto, SkillSummaryDto,
    SkillsResponse, SkillsSettingsInput, StopPromptResponse, SubmitPromptResponse,
    WebSearchSettingsInput, ZhipuCodingPlanUsageDto, ZhipuQuotaLimitDto, ZhipuToolUsageDetailDto,
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
