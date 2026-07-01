mod convert;
pub mod handlers;
mod runtime;
pub mod types;

// Re-exports from submodules
pub use self::handlers::{
    archive_project, archive_session, bootstrap_studio, create_session, initialize_runtime,
    list_discovered_skills, load_provider_usages, load_session_state, load_studio_events,
    open_project, resolve_interaction, save_general_settings, save_instructions_settings,
    save_mcp_settings, save_provider_settings, save_runtime_permission_mode, save_skills_settings,
    select_project, set_model_role, set_session_mode, shutdown_runtime, start_runtime, stop_prompt,
    submit_prompt, subscribe_global_events, subscribe_session_events,
};
pub use self::types::{
    BridgeActiveTurn, BridgeAgentSnapshotDto, BridgeAgentTimelineEventDto,
    BridgeAgentTimelinePayloadDto, BridgeEventEnvelope, BridgeEventPayload,
    BridgeInteractionChangedDto, BridgeInteractionPayloadDto, BridgeLspHealthDto,
    BridgeMcpHealthDto, BridgeMcpServerDto, BridgePlanLifecycleDto, BridgeRuntimeCostAmountDto,
    BridgeRuntimeStatus, BridgeSessionRuntimeDto, BridgeSessionStateResponse,
    BridgeSkillActivationDto, BridgeStudioAgentPartDto, BridgeStudioEventsResponse,
    BridgeStudioMessageDto, BridgeStudioMessageProjectionDto, BridgeStudioPartDeltaDto,
    BridgeStudioPartDto, BridgeStudioPartProjectionDto, BridgeStudioPlanPartDto,
    BridgeStudioSnapshotResponse, BridgeStudioToolPartDto, BridgeStudioTurnDto,
    BridgeUserQuestionDto, BridgeUserQuestionOptionDto, ConfigSavedResponse, DeepSeekBalanceDto,
    DeepSeekBalanceInfoDto, InstructionsSettingsInput, McpServerInput, McpSettingsInput,
    ProjectDto, ProviderInput, ProviderModelInput, ProviderSettingsInput, ProviderUsageDto,
    ProviderUsagesResponse, ResolveInteractionResponse, RoleInput, RuntimeSnapshot, SessionDto,
    SkillSummaryDto, SkillsResponse, SkillsSettingsInput, StopPromptResponse, SubmitPromptResponse,
    ZhipuCodingPlanUsageDto, ZhipuQuotaLimitDto, ZhipuToolUsageDetailDto,
};

#[cfg(test)]
mod tests {
    use pl_core::McpServerTransport;
    use pl_protocol::StudioEventKind;
    use pretty_assertions::assert_eq;

    use super::{
        BridgeEventPayload, BridgeSessionStateResponse, BridgeStudioEventsResponse,
        BridgeStudioSnapshotResponse, ConfigSavedResponse, ProviderUsagesResponse,
        ResolveInteractionResponse, SkillsResponse, StopPromptResponse, SubmitPromptResponse,
    };

    #[test]
    fn bridge_event_envelope_uses_typed_payload() {
        let event = pl_protocol::StudioEventEnvelope {
            event_id: "event-1".to_string(),
            project_id: None,
            session_id: Some("session-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            sequence: 7,
            created_at: 10,
            kind: StudioEventKind::Stale { lagged_events: 2 },
        };

        let envelope =
            super::convert::event::bridge_event_envelope(event).expect("event is bridge-visible");

        assert_eq!(envelope.session_id.as_deref(), Some("session-1"));
        assert_eq!(envelope.sequence, 7);
        assert_eq!(
            envelope.payload,
            BridgeEventPayload::Stale { lagged_events: 2 }
        );
    }

    #[test]
    fn bridge_filters_legacy_session_handoff_events() {
        let event = pl_protocol::StudioEventEnvelope {
            event_id: "event-1".to_string(),
            project_id: None,
            session_id: Some("session-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            sequence: 7,
            created_at: 10,
            kind: StudioEventKind::SessionHandoffChanged {
                handoff: pl_protocol::StudioSessionHandoff {
                    origin_session_id: "session-1".to_string(),
                    target_session_id: "session-2".to_string(),
                    target_session: None,
                    kind: "planImplementation".to_string(),
                    status: "completed".to_string(),
                    plan_id: None,
                    updated_at: 10,
                },
            },
        };

        assert!(!super::convert::event::bridge_visible_event(&event));
    }

    #[test]
    fn bridge_event_envelope_rejects_session_handoff_events() {
        let event = pl_protocol::StudioEventEnvelope {
            event_id: "event-1".to_string(),
            project_id: None,
            session_id: Some("session-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            sequence: 7,
            created_at: 10,
            kind: StudioEventKind::SessionHandoffChanged {
                handoff: pl_protocol::StudioSessionHandoff {
                    origin_session_id: "session-1".to_string(),
                    target_session_id: "session-2".to_string(),
                    target_session: None,
                    kind: "planImplementation".to_string(),
                    status: "completed".to_string(),
                    plan_id: None,
                    updated_at: 10,
                },
            },
        };

        assert_eq!(super::convert::event::bridge_event_envelope(event), None);
    }

    #[test]
    fn archive_project_api_is_exposed_to_flutter() {
        let _api: fn(String, Option<String>) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::archive_project;
    }

    #[test]
    fn list_discovered_skills_api_is_exposed_to_flutter() {
        let _api: fn(String) -> anyhow::Result<SkillsResponse> = super::list_discovered_skills;
    }

    #[test]
    fn small_command_responses_are_typed_for_flutter() {
        let _runtime_permission: fn(String) -> anyhow::Result<ConfigSavedResponse> =
            super::save_runtime_permission_mode;
        let _provider_usages: fn() -> anyhow::Result<ProviderUsagesResponse> =
            super::load_provider_usages;
        let _submit: fn(String, String, Vec<String>) -> anyhow::Result<SubmitPromptResponse> =
            super::submit_prompt;
        let _stop: fn(String) -> anyhow::Result<StopPromptResponse> = super::stop_prompt;
        let _resolve: fn(String, String) -> anyhow::Result<ResolveInteractionResponse> =
            super::resolve_interaction;
    }

    #[test]
    fn load_studio_events_api_returns_typed_bridge_events() {
        let _api: fn(
            String,
            Option<i64>,
            Option<i64>,
        ) -> anyhow::Result<BridgeStudioEventsResponse> = super::load_studio_events;
    }

    #[test]
    fn typed_settings_apis_are_exposed_to_flutter() {
        let _session: fn(String) -> anyhow::Result<BridgeSessionStateResponse> =
            super::load_session_state;
        let _instructions: fn(String) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::save_instructions_settings;
        let _skills: fn(String) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::save_skills_settings;
        let _mcp: fn(String) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::save_mcp_settings;
        let _general: fn(String) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::save_general_settings;
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
