use anyhow::{Context, Result};
use pl_core::{
    McpServerTransport, ProviderEdit, ProviderModelEdit, ProviderSettingsEdit,
    ProviderTemplateKind, ProviderUsageData, ProviderUsageState, RoleEdit, SessionRecord,
    StudioRuntimeSnapshot as CoreRuntimeSnapshot, ZhipuQuotaWindow,
};
use pl_protocol::{
    InteractionPayload, InteractionRequest, PlanLifecycleEvent, RuntimeCostAmount, SkillActivation,
    StudioAgentSnapshot, StudioAgentTimelineEvent, StudioAgentTimelineEventKind,
    StudioEventEnvelope, StudioEventKind, StudioLspHealth, StudioMcpHealth, StudioMessage,
    StudioPart, StudioPartDelta, StudioPartDeltaField, StudioSessionRuntime, StudioTurn,
};
use serde_json;

use super::runtime::BridgeRuntime;
use super::types::{
    BridgeActiveTurn, BridgeAgentSnapshotDto, BridgeAgentTimelineEventDto,
    BridgeAgentTimelinePayloadDto, BridgeEventEnvelope, BridgeEventPayload,
    BridgeInteractionChangedDto, BridgeInteractionPayloadDto, BridgeLspHealthDto,
    BridgeMcpHealthDto, BridgeMcpServerDto, BridgePlanLifecycleDto, BridgeRuntimeCostAmountDto,
    BridgeRuntimeStatus, BridgeSessionRuntimeDto, BridgeSessionStateResponse,
    BridgeSkillActivationDto, BridgeStudioMessageDto, BridgeStudioMessageProjectionDto,
    BridgeStudioPartDeltaDto, BridgeStudioPartDto, BridgeStudioPartProjectionDto,
    BridgeStudioPlanPartDto, BridgeStudioToolPartDto, BridgeStudioTurnDto, BridgeUserQuestionDto,
    BridgeUserQuestionOptionDto, ConfigSavedResponse, DeepSeekBalanceDto, DeepSeekBalanceInfoDto,
    ProjectDto, ProviderInput, ProviderModelInput, ProviderSettingsInput, ProviderUsageDto,
    ResolveInteractionResponse, RoleInput, RuntimeSnapshot, SessionDto, SkillSummaryDto,
    SkillsResponse, StopPromptResponse, SubmitPromptResponse, ZhipuCodingPlanUsageDto,
    ZhipuQuotaLimitDto, ZhipuToolUsageDetailDto,
};

// ── Core conversion functions ──

pub fn runtime_snapshot(snapshot: CoreRuntimeSnapshot) -> RuntimeSnapshot {
    RuntimeSnapshot {
        status: match snapshot.status {
            pl_core::StudioRuntimeStatus::Uninitialized => BridgeRuntimeStatus::Uninitialized,
            pl_core::StudioRuntimeStatus::Initializing => BridgeRuntimeStatus::Initializing,
            pl_core::StudioRuntimeStatus::Ready => BridgeRuntimeStatus::Ready,
            pl_core::StudioRuntimeStatus::ShuttingDown => BridgeRuntimeStatus::ShuttingDown,
            pl_core::StudioRuntimeStatus::Stopped => BridgeRuntimeStatus::Stopped,
            pl_core::StudioRuntimeStatus::Failed => BridgeRuntimeStatus::Failed,
        },
        active_turns: snapshot
            .active_turns
            .into_iter()
            .map(|turn| BridgeActiveTurn {
                session_id: turn.session_id,
                turn_id: turn.turn_id,
            })
            .collect(),
        updated_at: snapshot.updated_at,
        error: snapshot.error,
    }
}

pub fn bridge_event_envelope(event: StudioEventEnvelope) -> Option<BridgeEventEnvelope> {
    if !bridge_visible_event(&event) {
        return None;
    }
    Some(BridgeEventEnvelope {
        event_id: event.event_id,
        session_id: event.session_id,
        turn_id: event.turn_id,
        sequence: event.sequence,
        created_at: event.created_at,
        payload: bridge_event_payload(event.kind),
    })
}

pub fn is_session_state_event(event: &StudioEventEnvelope) -> bool {
    match &event.kind {
        StudioEventKind::MessageUpdated { .. }
        | StudioEventKind::MessageRemoved { .. }
        | StudioEventKind::MessagePartUpdated { .. }
        | StudioEventKind::MessagePartRemoved { .. }
        | StudioEventKind::MessagePartDelta { .. }
        | StudioEventKind::SessionHandoffChanged { .. }
        | StudioEventKind::TurnChanged { .. }
        | StudioEventKind::InteractionChanged { .. }
        | StudioEventKind::PlanLifecycleChanged { .. }
        | StudioEventKind::SessionRuntimeChanged { .. }
        | StudioEventKind::AgentChanged { .. }
        | StudioEventKind::AgentTimelineChanged { .. }
        | StudioEventKind::SkillActivated { .. }
        | StudioEventKind::SessionListChanged { .. }
        | StudioEventKind::McpHealthChanged { .. }
        | StudioEventKind::LspHealthChanged { .. } => true,
    }
}

fn bridge_visible_event(event: &StudioEventEnvelope) -> bool {
    !matches!(event.kind, StudioEventKind::SessionHandoffChanged { .. })
}

// ── Project/Session DTOs ──

pub fn project_dto(project: pl_core::ProjectRecord) -> ProjectDto {
    ProjectDto {
        id: project.id,
        name: project.name,
        path: project.path,
        updated_at: project.updated_at,
    }
}

pub fn session_dto(session: SessionRecord) -> SessionDto {
    SessionDto {
        id: session.id,
        project_id: session.project_id,
        title: session.title,
        mode: session.mode,
        updated_at: session.updated_at,
        visibility: session.visibility.as_str().to_string(),
        parent_session_id: session.parent_session_id,
    }
}

pub fn agent_bridge_dto(agent: pl_core::StudioAgentSnapshotRecord) -> BridgeAgentSnapshotDto {
    BridgeAgentSnapshotDto {
        id: agent.id,
        session_id: agent.session_id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status.as_str().to_string(),
        summary: agent.summary,
        depth: agent.depth as u32,
        error: agent.error,
        reason: agent.reason,
        updated_at: agent.updated_at,
    }
}

pub fn agent_event_bridge_dto(
    event: pl_core::StudioAgentTimelineEventRecord,
) -> Result<BridgeAgentTimelineEventDto> {
    let payload = serde_json::from_str::<StudioAgentTimelineEvent>(&event.payload_json)
        .with_context(|| {
            format!(
                "invalid agent timeline payload: {event_id}",
                event_id = event.event_id
            )
        })
        .map(|event| bridge_agent_timeline_payload(event.kind))?;
    Ok(BridgeAgentTimelineEventDto {
        event_id: event.event_id,
        session_id: event.session_id,
        sequence: event.sequence.max(0) as u64,
        created_at: event.created_at,
        payload,
    })
}

pub async fn bridge_session_runtime_view(
    bridge: &'static BridgeRuntime,
    session_id: &str,
) -> Result<BridgeSessionRuntimeDto> {
    let runtime = bridge.studio.session_runtime(session_id).await?;
    let active_skills = bridge
        .studio
        .store()
        .list_session_skill_names(session_id)
        .await?;
    Ok(BridgeSessionRuntimeDto {
        session_id: runtime.session_id,
        model: runtime.model,
        context_window: runtime.context_window,
        latest_context_tokens: runtime.latest_context_tokens,
        prompt_tokens: runtime.prompt_tokens,
        completion_tokens: runtime.completion_tokens,
        cached_prompt_tokens: runtime.cached_prompt_tokens,
        total_tokens: runtime.total_tokens,
        estimated_costs: runtime
            .estimated_costs
            .into_iter()
            .map(bridge_cost_amount)
            .collect(),
        has_unpriced_usage: runtime.has_unpriced_usage,
        active_skills,
        active_mcp_servers: bridge.studio.mcp_runtime().available_server_names().await,
        active_lsp_servers: bridge.studio.lsp_runtime().active_server_names().await,
        updated_at: runtime.updated_at,
    })
}

pub fn interaction_request_bridge_dto(
    interaction: InteractionRequest,
) -> BridgeInteractionChangedDto {
    bridge_interaction_changed(pl_protocol::InteractionChangedEvent { interaction })
}

pub fn resolve_interaction_response(
    response: pl_core::StudioResolveInteractionResponse,
) -> ResolveInteractionResponse {
    ResolveInteractionResponse {
        session_id: response.session_id,
        interaction: bridge_interaction_changed(pl_protocol::InteractionChangedEvent {
            interaction: response.interaction,
        }),
        sessions: response
            .sessions
            .into_iter()
            .map(session_summary_dto)
            .collect(),
    }
}

// ── Event payload converters ──

pub fn bridge_event_payload(kind: StudioEventKind) -> BridgeEventPayload {
    match kind {
        StudioEventKind::TurnChanged { turn } => BridgeEventPayload::TurnChanged {
            turn: bridge_turn(turn),
        },
        StudioEventKind::MessageUpdated { message } => BridgeEventPayload::MessageUpdated {
            message: bridge_message(*message),
        },
        StudioEventKind::MessageRemoved { message_id } => {
            BridgeEventPayload::MessageRemoved { message_id }
        }
        StudioEventKind::MessagePartUpdated { part } => BridgeEventPayload::MessagePartUpdated {
            part: Box::new(bridge_part(*part)),
        },
        StudioEventKind::MessagePartRemoved {
            message_id,
            part_id,
        } => BridgeEventPayload::MessagePartRemoved {
            message_id,
            part_id,
        },
        StudioEventKind::MessagePartDelta { delta } => BridgeEventPayload::MessagePartDelta {
            delta: bridge_part_delta(*delta),
        },
        StudioEventKind::InteractionChanged { event } => BridgeEventPayload::InteractionChanged {
            event: bridge_interaction_changed(*event),
        },
        StudioEventKind::AgentChanged { agent } => BridgeEventPayload::AgentChanged {
            agent: Box::new(bridge_agent_snapshot(*agent)),
        },
        StudioEventKind::AgentTimelineChanged { event } => {
            BridgeEventPayload::AgentTimelineChanged {
                event: bridge_agent_timeline_event(*event),
            }
        }
        StudioEventKind::SessionRuntimeChanged { runtime } => {
            BridgeEventPayload::SessionRuntimeChanged {
                runtime: Box::new(bridge_session_runtime(*runtime)),
            }
        }
        StudioEventKind::SkillActivated { activation } => BridgeEventPayload::SkillActivated {
            activation: bridge_skill_activation(*activation),
        },
        StudioEventKind::PlanLifecycleChanged { event } => {
            BridgeEventPayload::PlanLifecycleChanged {
                event: BridgePlanLifecycleDto {
                    plan_id: event.plan_id,
                    state: event.state.as_str().to_string(),
                    turn_id: event.turn_id,
                    reason: event.reason,
                    updated_at: event.updated_at,
                },
            }
        }
        StudioEventKind::SessionHandoffChanged { .. } => {
            unreachable!("session handoff events are not bridge-visible")
        }
        StudioEventKind::SessionListChanged {
            project_id,
            sessions,
        } => BridgeEventPayload::SessionListChanged {
            project_id,
            sessions: sessions.into_iter().map(session_summary_dto).collect(),
        },
        StudioEventKind::McpHealthChanged { health } => BridgeEventPayload::McpHealthChanged {
            health: bridge_mcp_health(health),
        },
        StudioEventKind::LspHealthChanged { health } => BridgeEventPayload::LspHealthChanged {
            health: bridge_lsp_health(health),
        },
        StudioEventKind::Stale { lagged_events } => BridgeEventPayload::Stale { lagged_events },
    }
}

pub fn bridge_turn(turn: StudioTurn) -> BridgeStudioTurnDto {
    BridgeStudioTurnDto {
        turn_id: turn.turn_id,
        session_id: turn.session_id,
        status: turn.status.as_str().to_string(),
        reason: turn.reason,
        updated_at: turn.updated_at,
    }
}

pub fn bridge_message(message: StudioMessage) -> BridgeStudioMessageDto {
    BridgeStudioMessageDto {
        message_id: message.message_id,
        session_id: message.session_id,
        turn_id: message.turn_id,
        role: message.role.as_str().to_string(),
        status: message.status.as_str().to_string(),
        created_at: message.created_at,
        updated_at: message.updated_at,
        completed_at: message.completed_at,
        error: message.error,
    }
}

pub fn bridge_part(part: StudioPart) -> BridgeStudioPartDto {
    BridgeStudioPartDto {
        part_id: part.part_id,
        message_id: part.message_id,
        session_id: part.session_id,
        turn_id: part.turn_id,
        part_type: part.part_type.as_str().to_string(),
        order: part.order,
        revision: part.revision,
        status: part.status.as_str().to_string(),
        created_at: part.created_at,
        updated_at: part.updated_at,
        completed_at: part.completed_at,
        error: part.error,
        text_channel: part.text_channel.map(|c| c.to_string()),
        activity_group_id: part.activity_group_id,
        text: part.text,
        tool: part.tool.map(|tool| BridgeStudioToolPartDto {
            tool_call_id: tool.tool_call_id,
            call_id: tool.call_id,
            provider_item_id: tool.provider_item_id,
            name: tool.name,
            arguments: tool.arguments,
            result: tool.result,
            exit_code: tool.exit_code,
            timed_out: tool.timed_out,
            working_directory: tool.working_directory,
            denial_reason: tool.denial_reason,
        }),
        agent: part.agent.map(|agent| BridgeStudioAgentPartDto {
            id: agent.id,
            path: agent.path,
            parent_path: agent.parent_path,
            role: agent.role,
            task: agent.task,
            status: agent.status.as_str().to_string(),
            summary: agent.summary,
            depth: agent.depth,
            error: agent.error,
            reason: agent.reason,
        }),
        plan: part.plan.map(|plan| BridgeStudioPlanPartDto {
            content: plan.content,
        }),
        synthetic: part.synthetic,
        ignored: part.ignored,
    }
}

pub fn bridge_part_delta(delta: StudioPartDelta) -> BridgeStudioPartDeltaDto {
    BridgeStudioPartDeltaDto {
        part_id: delta.part_id,
        revision: delta.revision,
        field: bridge_part_delta_field(delta.field),
        delta: delta.delta,
        chunk_index: delta.chunk_index,
    }
}

pub fn bridge_part_delta_field(field: StudioPartDeltaField) -> String {
    match field {
        StudioPartDeltaField::Text => "text".to_string(),
        StudioPartDeltaField::ReasoningSummary => "reasoning.summary".to_string(),
        StudioPartDeltaField::PlanContent => "planContent".to_string(),
        StudioPartDeltaField::ToolArguments => "tool.arguments".to_string(),
        StudioPartDeltaField::ToolResult => "tool.result".to_string(),
    }
}

pub fn bridge_interaction_changed(
    event: pl_protocol::InteractionChangedEvent,
) -> BridgeInteractionChangedDto {
    let interaction = event.interaction;
    BridgeInteractionChangedDto {
        interaction_id: interaction.interaction_id,
        kind: interaction.kind.as_str().to_string(),
        status: interaction.status.as_str().to_string(),
        session_id: interaction.scope.session_id,
        turn_id: interaction.scope.turn_id,
        item_id: interaction.scope.item_id,
        tool_id: interaction.scope.tool_id,
        agent_path: interaction.scope.agent_path,
        payload: bridge_interaction_payload(interaction.payload),
        created_at: interaction.created_at,
        updated_at: interaction.updated_at,
        resolved_at: interaction.resolved_at,
    }
}

pub fn bridge_interaction_payload(payload: InteractionPayload) -> BridgeInteractionPayloadDto {
    match payload {
        InteractionPayload::UserInput { questions } => BridgeInteractionPayloadDto::UserInput {
            questions: questions
                .into_iter()
                .map(|question| BridgeUserQuestionDto {
                    id: question.id,
                    header: question.header,
                    question: question.question,
                    is_other: question.is_other,
                    is_secret: question.is_secret,
                    options: question.options.map(|options| {
                        options
                            .into_iter()
                            .map(|option| BridgeUserQuestionOptionDto {
                                label: option.label,
                                description: option.description,
                            })
                            .collect()
                    }),
                })
                .collect(),
        },
        InteractionPayload::ToolApproval {
            name,
            arguments_json,
            working_directory,
            parent_agent_id,
        } => BridgeInteractionPayloadDto::ToolApproval {
            name,
            arguments_json,
            working_directory,
            parent_agent_id,
        },
        InteractionPayload::PlanConfirmation { plan_id, content } => {
            BridgeInteractionPayloadDto::PlanConfirmation { plan_id, content }
        }
    }
}

pub fn bridge_agent_snapshot(snapshot: StudioAgentSnapshot) -> BridgeAgentSnapshotDto {
    BridgeAgentSnapshotDto {
        id: snapshot.id,
        session_id: snapshot.session_id,
        path: snapshot.path,
        parent_path: snapshot.parent_path,
        role: snapshot.role,
        task: snapshot.task,
        status: snapshot.status.as_str().to_string(),
        summary: snapshot.summary,
        depth: snapshot.depth as u32,
        error: snapshot.error,
        reason: snapshot.reason,
        updated_at: snapshot.updated_at,
    }
}

pub fn bridge_agent_timeline_event(event: StudioAgentTimelineEvent) -> BridgeAgentTimelineEventDto {
    BridgeAgentTimelineEventDto {
        event_id: event.event_id,
        session_id: event.session_id,
        sequence: event.sequence.max(0) as u64,
        created_at: event.created_at,
        payload: bridge_agent_timeline_payload(event.kind),
    }
}

pub fn bridge_agent_timeline_payload(
    payload: StudioAgentTimelineEventKind,
) -> BridgeAgentTimelinePayloadDto {
    match payload {
        StudioAgentTimelineEventKind::SpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
        } => BridgeAgentTimelinePayloadDto::SpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
        },
        StudioAgentTimelineEventKind::SpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status,
            prompt,
            error,
        } => BridgeAgentTimelinePayloadDto::SpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status,
            prompt,
            error,
        },
        StudioAgentTimelineEventKind::InteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
        } => BridgeAgentTimelinePayloadDto::InteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
        },
        StudioAgentTimelineEventKind::InteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            prompt,
            error,
        } => BridgeAgentTimelinePayloadDto::InteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            prompt,
            error,
        },
        StudioAgentTimelineEventKind::WaitingBegin {
            call_id,
            sender_path,
        } => BridgeAgentTimelinePayloadDto::WaitingBegin {
            call_id,
            sender_path,
        },
        StudioAgentTimelineEventKind::WaitingEnd {
            call_id,
            sender_path,
            timed_out,
        } => BridgeAgentTimelinePayloadDto::WaitingEnd {
            call_id,
            sender_path,
            timed_out,
        },
        StudioAgentTimelineEventKind::CloseBegin {
            call_id,
            sender_path,
            receiver_path,
        } => BridgeAgentTimelinePayloadDto::CloseBegin {
            call_id,
            sender_path,
            receiver_path,
        },
        StudioAgentTimelineEventKind::CloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            error,
        } => BridgeAgentTimelinePayloadDto::CloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            error,
        },
    }
}

pub fn bridge_session_runtime(snapshot: StudioSessionRuntime) -> BridgeSessionRuntimeDto {
    BridgeSessionRuntimeDto {
        session_id: snapshot.session_id,
        model: snapshot.model,
        context_window: snapshot.context_window,
        latest_context_tokens: snapshot.latest_context_tokens,
        prompt_tokens: snapshot.prompt_tokens,
        completion_tokens: snapshot.completion_tokens,
        cached_prompt_tokens: snapshot.cached_prompt_tokens,
        total_tokens: snapshot.total_tokens,
        estimated_costs: snapshot
            .estimated_costs
            .into_iter()
            .map(bridge_cost_amount)
            .collect(),
        has_unpriced_usage: snapshot.has_unpriced_usage,
        active_skills: Vec::new(),
        active_mcp_servers: Vec::new(),
        active_lsp_servers: Vec::new(),
        updated_at: snapshot.updated_at,
    }
}

pub fn bridge_cost_amount(amount: RuntimeCostAmount) -> BridgeRuntimeCostAmountDto {
    BridgeRuntimeCostAmountDto {
        currency: amount.currency,
        amount: amount.amount,
    }
}

pub fn bridge_skill_activation(activation: SkillActivation) -> BridgeSkillActivationDto {
    BridgeSkillActivationDto {
        name: activation.name,
        source: activation.source.as_str().to_string(),
        path: activation.path,
        turn_id: activation.turn_id,
        tool_call_id: activation.tool_call_id,
        activated_at: activation.activated_at,
    }
}

pub fn session_summary_dto(session: pl_protocol::StudioSessionSummary) -> SessionDto {
    SessionDto {
        id: session.id,
        project_id: session.project_id,
        title: session.title,
        mode: session.mode,
        updated_at: session.updated_at,
        visibility: session.visibility,
        parent_session_id: session.parent_session_id,
    }
}

pub fn bridge_mcp_health(health: StudioMcpHealth) -> BridgeMcpHealthDto {
    BridgeMcpHealthDto {
        active_mcp_servers: health.active_mcp_servers,
        mcp_servers: health
            .mcp_servers
            .into_iter()
            .map(|server| BridgeMcpServerDto {
                id: server.id,
                enabled: server.enabled,
                transport: server.transport.to_string(),
                command: server.command,
                url: server.url,
                endpoint: server.endpoint,
                status_kind: server.status_kind.as_str().to_string(),
                availability_kind: server.availability_kind.as_str().to_string(),
            })
            .collect(),
    }
}

pub fn bridge_lsp_health(health: StudioLspHealth) -> BridgeLspHealthDto {
    BridgeLspHealthDto {
        active_lsp_servers: health.active_lsp_servers,
    }
}

// ── Utility functions ──

pub fn normalized_string_list(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

pub fn mcp_transport_from_label(label: &str) -> McpServerTransport {
    match label.trim() {
        "streamableHttp" | "streamable_http" | "http" => McpServerTransport::StreamableHttp,
        _ => McpServerTransport::Stdio,
    }
}

// ── Provider settings converters ──

pub fn provider_settings_edit(
    input: ProviderSettingsInput,
    current: &pl_core::PureConfig,
) -> Result<ProviderSettingsEdit> {
    Ok(ProviderSettingsEdit {
        default_provider: input.default_provider_id,
        providers: input
            .providers
            .into_iter()
            .map(|provider| provider_edit(provider, current))
            .collect::<Result<Vec<_>>>()?,
        roles: input.roles.into_iter().map(role_edit).collect(),
    })
}

fn provider_edit(input: ProviderInput, current: &pl_core::PureConfig) -> Result<ProviderEdit> {
    let kind = ProviderTemplateKind::from_key(&input.template_kind).with_context(|| {
        format!(
            "unsupported provider template: {kind}",
            kind = input.template_kind
        )
    })?;
    let current_token = current
        .providers
        .get(&input.id)
        .and_then(|provider| provider.bearer_token.clone());
    let bearer_token = if input.bearer_token.trim().is_empty() {
        current_token
    } else {
        Some(input.bearer_token)
    };
    Ok(ProviderEdit {
        key: input.id,
        kind,
        name: input.name,
        base_url: Some(input.base_url),
        bearer_token,
        default_model: input.default_model,
        custom_models: input
            .custom_models
            .into_iter()
            .map(provider_model_edit)
            .collect(),
    })
}

fn provider_model_edit(input: ProviderModelInput) -> ProviderModelEdit {
    ProviderModelEdit {
        slug: input.slug,
        display_name: input.display_name,
        reasoning_efforts: input.reasoning_efforts,
        base_instructions: input.base_instructions.unwrap_or_default(),
    }
}

pub fn provider_usage_dto(record: pl_core::ProviderUsageRecord) -> ProviderUsageDto {
    match record.state {
        ProviderUsageState::Error { message } => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "error".to_string(),
            usage_kind: String::new(),
            message: Some(message),
            balance: None,
            coding_plan: None,
        },
        ProviderUsageState::Pending => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "pending".to_string(),
            usage_kind: String::new(),
            message: None,
            balance: None,
            coding_plan: None,
        },
        ProviderUsageState::Ready(ProviderUsageData::DeepSeekBalance(balance)) => {
            ProviderUsageDto {
                provider_id: record.provider_id,
                updated_at: record.updated_at,
                status: "ready".to_string(),
                usage_kind: "deepseekBalance".to_string(),
                message: None,
                balance: Some(DeepSeekBalanceDto {
                    is_available: balance.is_available,
                    balances: balance
                        .balances
                        .into_iter()
                        .map(|item| DeepSeekBalanceInfoDto {
                            currency: item.currency,
                            total_balance: item.total_balance,
                            granted_balance: item.granted_balance,
                            topped_up_balance: item.topped_up_balance,
                        })
                        .collect(),
                }),
                coding_plan: None,
            }
        }
        ProviderUsageState::Ready(ProviderUsageData::ZhipuCodingPlan(usage)) => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "ready".to_string(),
            usage_kind: "zhipuCodingPlan".to_string(),
            message: None,
            balance: None,
            coding_plan: Some(ZhipuCodingPlanUsageDto {
                level: usage.level,
                limits: usage
                    .limits
                    .into_iter()
                    .map(|limit| {
                        let (window, label) = zhipu_window_labels(&limit.window);
                        ZhipuQuotaLimitDto {
                            window: window.to_string(),
                            label: label.to_string(),
                            percentage: limit.percentage,
                            current_value: limit.current_value,
                            total: limit.total,
                            remaining: limit.remaining,
                            next_reset_at: limit.next_reset_at,
                            usage_details: limit.usage_details.map(|details| {
                                details
                                    .into_iter()
                                    .map(|detail| ZhipuToolUsageDetailDto {
                                        name: detail.name,
                                        current_value: detail.current_value,
                                        total: detail.total,
                                        percentage: detail.percentage,
                                    })
                                    .collect()
                            }),
                        }
                    })
                    .collect(),
            }),
        },
    }
}

fn zhipu_window_labels(window: &ZhipuQuotaWindow) -> (&'static str, &str) {
    match window {
        ZhipuQuotaWindow::FiveHour => ("fiveHour", "5h"),
        ZhipuQuotaWindow::Weekly => ("weekly", "7d"),
        ZhipuQuotaWindow::McpMonthly => ("mcpMonthly", "MCP"),
        ZhipuQuotaWindow::Other(label) => ("other", label.as_str()),
    }
}

fn role_edit(input: RoleInput) -> RoleEdit {
    RoleEdit {
        key: input.key,
        provider: input.provider,
        model: input.model,
        effort: input.effort,
    }
}
