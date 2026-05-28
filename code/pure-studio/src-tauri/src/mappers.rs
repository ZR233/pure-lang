use pl_core::{
    ConfigStore, ModelCapabilityConfig, ModelConfig, ModelRole, ProjectRecord, ProviderConfig,
    ProviderEdit, ProviderModelEdit, ProviderTemplateKind, PureConfig, RoleEdit, SessionRecord,
    SessionRuntimeRecord, StudioAgentEventRecord, StudioRuntime, TraceEvent, TraceEventKind,
    TurnResultStatus, infer_provider_template_kind,
};
use pl_protocol::{Message, MessageContent, MessageRole};

use crate::dto::{
    AgentEventDto, ConfigDto, MessageDto, ModelDto, ProjectDto, ProviderDto, ProviderInput,
    ProviderSettingsInput, ProviderTemplateDto, RoleDto, RoleInput, SessionDto, SessionRuntimeDto,
    TimelineItemDto, UsageDto,
};
use crate::state::{CommandError, CommandResult};

pub fn config_dto(store: &ConfigStore) -> CommandResult<ConfigDto> {
    let config = store.load_or_default()?;
    Ok(ConfigDto {
        toml: config.to_toml_pretty()?,
        providers: provider_dtos(&config),
        roles: role_dtos(&config),
        templates: provider_template_dtos()?,
        config_exists: store.config_exists(),
    })
}

pub async fn load_session_runtime_dto(
    studio: &StudioRuntime,
    session_id: &str,
) -> CommandResult<SessionRuntimeDto> {
    let config = studio.config_store().load_or_default()?;
    let record = studio.session_runtime(session_id).await?;
    Ok(session_runtime_dto(record, &config))
}

pub fn session_runtime_dto(record: SessionRuntimeRecord, config: &PureConfig) -> SessionRuntimeDto {
    let cache_hit_rate = if record.prompt_tokens == 0 {
        None
    } else {
        Some(record.cached_prompt_tokens as f64 / record.prompt_tokens as f64)
    };
    let current_model = config
        .resolve_role(ModelRole::Planner)
        .ok()
        .and_then(|resolved| {
            resolved
                .models
                .iter()
                .find(|model| model.slug == record.model)
                .cloned()
                .or_else(|| {
                    resolved
                        .models
                        .iter()
                        .find(|model| model.slug == resolved.role_config.model)
                        .cloned()
                })
        });
    SessionRuntimeDto {
        session_id: record.session_id,
        model: record.model,
        context_window: record.context_window,
        latest_context_tokens: record.latest_context_tokens,
        prompt_tokens: record.prompt_tokens,
        completion_tokens: record.completion_tokens,
        cached_prompt_tokens: record.cached_prompt_tokens,
        total_tokens: record.total_tokens,
        cache_hit_rate,
        currency: record.currency.or_else(|| {
            current_model
                .as_ref()
                .and_then(|model| model.currency.clone())
        }),
        input_price_per_mtok: current_model
            .as_ref()
            .and_then(|model| model.input_price_per_mtok),
        output_price_per_mtok: current_model
            .as_ref()
            .and_then(|model| model.output_price_per_mtok),
        cache_read_price_per_mtok: current_model
            .as_ref()
            .and_then(|model| model.cache_read_price_per_mtok),
        estimated_cost: record.estimated_cost,
        active_skills: config.runtime.active_skills.clone(),
        active_mcp_servers: config.runtime.active_mcp_servers.clone(),
        updated_at: record.updated_at,
    }
}

pub fn role_dtos(config: &PureConfig) -> Vec<RoleDto> {
    ModelRole::all()
        .into_iter()
        .map(|role| {
            let role_config = config.role_config(role);
            RoleDto {
                key: role.key().to_string(),
                display_name: role.display_name().to_string(),
                provider: role_config.provider.clone(),
                model: role_config.model.clone(),
                effort: role_config.effort.as_str().to_string(),
            }
        })
        .collect()
}

pub fn provider_dtos(config: &PureConfig) -> Vec<ProviderDto> {
    config
        .providers
        .iter()
        .map(|(provider_key, provider)| provider_dto(provider_key, provider))
        .collect()
}

pub fn provider_dto(provider_key: &str, provider: &ProviderConfig) -> ProviderDto {
    let kind = infer_provider_template_kind(provider_key, provider);
    let default_slugs = kind.default_model_slugs();
    let default_models = kind.default_models().unwrap_or_default();
    let custom_models = provider
        .models
        .iter()
        .filter(|model| !default_slugs.contains(&model.slug.as_str()))
        .map(model_dto)
        .collect::<Vec<_>>();
    let models = default_models
        .iter()
        .map(model_dto)
        .chain(custom_models.iter().cloned())
        .collect::<Vec<_>>();
    ProviderDto {
        id: provider_key.to_string(),
        template_kind: kind.key().to_string(),
        name: provider.name.clone(),
        subtitle: format!("{} Platform", provider.name),
        status: provider_status(provider).to_string(),
        base_url: provider.base_url.clone().unwrap_or_default(),
        bearer_token: String::new(),
        has_bearer_token: provider
            .bearer_token
            .as_ref()
            .is_some_and(|token| !token.trim().is_empty()),
        default_model: provider.default_model.clone(),
        model_count: models.len().to_string(),
        updated_at: "Loaded".to_string(),
        wire_api: provider.wire_api.to_string(),
        models,
        default_models: default_models.iter().map(model_dto).collect(),
        custom_models,
    }
}

pub fn provider_template_dtos() -> CommandResult<Vec<ProviderTemplateDto>> {
    ProviderTemplateKind::all()
        .into_iter()
        .map(provider_template_dto)
        .collect()
}

pub fn provider_template_dto(kind: ProviderTemplateKind) -> CommandResult<ProviderTemplateDto> {
    let info = kind.provider_config()?;
    Ok(ProviderTemplateDto {
        id: kind.key().to_string(),
        name: info.name,
        base_url: info.base_url.unwrap_or_default(),
        default_model: info.default_model,
        wire_api: info.wire_api.to_string(),
        default_models: kind.default_models()?.iter().map(model_dto).collect(),
    })
}

pub fn provider_status(provider: &ProviderConfig) -> &'static str {
    if provider.bearer_token.is_some() {
        "Healthy"
    } else {
        "Needs setup"
    }
}

pub fn model_dto(model: &ModelConfig) -> ModelDto {
    ModelDto {
        slug: model.slug.clone(),
        display_name: model.display_name.clone(),
        description: model.description.clone(),
        context_window: model.context_window,
        max_context_window: model.max_context_window,
        auto_compact_token_limit: model.auto_compact_token_limit,
        default_temperature: model.default_temperature,
        max_output_tokens: model.max_output_tokens,
        currency: model.currency.clone(),
        input_price_per_mtok: model.input_price_per_mtok,
        output_price_per_mtok: model.output_price_per_mtok,
        cache_read_price_per_mtok: model.cache_read_price_per_mtok,
        reasoning_efforts: model.reasoning_efforts.clone(),
        capabilities: model
            .capabilities
            .iter()
            .map(capability_name)
            .map(str::to_string)
            .collect(),
        input_modalities: model
            .input_modalities
            .iter()
            .map(|modality| format!("{modality:?}").to_ascii_lowercase())
            .collect(),
        truncation_mode: format!("{:?}", model.truncation_policy.mode).to_ascii_lowercase(),
        truncation_limit: model.truncation_policy.limit,
    }
}

pub fn capability_name(capability: &ModelCapabilityConfig) -> &'static str {
    match capability {
        ModelCapabilityConfig::Streaming => "streaming",
        ModelCapabilityConfig::FunctionCalling => "function_calling",
        ModelCapabilityConfig::Vision => "vision",
        ModelCapabilityConfig::ParallelToolCalls => "parallel_tool_calls",
        ModelCapabilityConfig::Reasoning => "reasoning",
        ModelCapabilityConfig::WebSearch => "web_search",
        ModelCapabilityConfig::CustomTools => "custom_tools",
        ModelCapabilityConfig::FreeformTools => "freeform_tools",
    }
}

pub fn provider_edit(
    input: ProviderInput,
    current_token: Option<String>,
) -> CommandResult<ProviderEdit> {
    let bearer_token = if input.bearer_token.trim().is_empty() {
        current_token
    } else {
        Some(input.bearer_token)
    };
    Ok(ProviderEdit {
        key: input.id,
        kind: provider_template_kind(&input.template_kind)?,
        name: input.name,
        base_url: Some(input.base_url),
        bearer_token,
        default_model: input.default_model,
        wire_api: input.wire_api,
        custom_models: input
            .custom_models
            .into_iter()
            .map(|model| ProviderModelEdit {
                slug: model.slug,
                display_name: model.display_name,
                reasoning_efforts: model.reasoning_efforts,
            })
            .collect(),
    })
}

pub fn role_edit(input: RoleInput) -> RoleEdit {
    RoleEdit {
        key: input.key,
        provider: input.provider,
        model: input.model,
        effort: input.effort,
    }
}

pub fn provider_template_kind(value: &str) -> CommandResult<ProviderTemplateKind> {
    ProviderTemplateKind::from_key(value).ok_or_else(|| {
        CommandError::from_display(format!("unsupported provider template: {value}"))
    })
}

pub fn project_dtos(projects: Vec<ProjectRecord>) -> Vec<ProjectDto> {
    projects
        .into_iter()
        .map(|project| ProjectDto {
            id: project.id,
            name: project.name,
            path: project.path,
            updated_at: project.updated_at,
        })
        .collect()
}

pub fn session_dtos(sessions: Vec<SessionRecord>) -> Vec<SessionDto> {
    sessions
        .into_iter()
        .map(|session| SessionDto {
            id: session.id,
            project_id: session.project_id,
            title: session.title,
            mode: session.mode,
            updated_at: session.updated_at,
        })
        .collect()
}

pub fn message_dtos(messages: Vec<Message>) -> Vec<MessageDto> {
    messages.into_iter().map(message_dto).collect()
}

pub fn agent_event_dtos(events: Vec<StudioAgentEventRecord>) -> Vec<AgentEventDto> {
    events.into_iter().map(agent_event_dto).collect()
}

pub fn agent_event_dto(event: StudioAgentEventRecord) -> AgentEventDto {
    AgentEventDto {
        event_id: event.event_id,
        id: event.agent_id,
        path: event.path,
        parent_path: event.parent_path,
        role: event.role,
        task: event.task,
        status: event.status.as_str().to_string(),
        summary: event.summary,
        depth: event.depth,
        error: event.error,
        updated_at: event.created_at,
    }
}

pub fn message_dto(message: Message) -> MessageDto {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
    .to_string();
    MessageDto {
        role,
        content: message_content_text(message.content),
        reasoning_content: message.reasoning_content,
        metadata: message.metadata,
    }
}

pub fn turn_result_status_label(status: TurnResultStatus) -> &'static str {
    match status {
        TurnResultStatus::Completed => "completed",
        TurnResultStatus::Failed => "failed",
        TurnResultStatus::Interrupted => "interrupted",
    }
}

pub fn trace_events_to_timeline_items(events: &[TraceEvent]) -> Vec<TimelineItemDto> {
    events.iter().map(trace_event_to_timeline_item).collect()
}

pub fn trace_event_to_timeline_item(event: &TraceEvent) -> TimelineItemDto {
    let sequence = event.sequence;
    let timestamp = event.timestamp;
    match &event.kind {
        TraceEventKind::TurnStarted { turn_id } => TimelineItemDto {
            kind: "turn".to_string(),
            sequence,
            timestamp,
            turn_id: Some(turn_id.clone()),
            tool_call_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_status: None,
            tool_result: None,
            inference_model: None,
            inference_usage: None,
            turn_status: Some("started".to_string()),
            turn_model: None,
            turn_usage: None,
        },
        TraceEventKind::TurnCompleted {
            turn_id,
            model,
            usage,
            ..
        } => TimelineItemDto {
            kind: "turn".to_string(),
            sequence,
            timestamp,
            turn_id: Some(turn_id.clone()),
            tool_call_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_status: None,
            tool_result: None,
            inference_model: None,
            inference_usage: None,
            turn_status: Some("completed".to_string()),
            turn_model: Some(model.clone()),
            turn_usage: Some(UsageDto {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                cached_prompt_tokens: usage.cached_prompt_tokens,
                total_tokens: usage.total_tokens,
            }),
        },
        TraceEventKind::TurnFailed { turn_id, .. } => TimelineItemDto {
            kind: "turn".to_string(),
            sequence,
            timestamp,
            turn_id: Some(turn_id.clone()),
            tool_call_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_status: None,
            tool_result: None,
            inference_model: None,
            inference_usage: None,
            turn_status: Some("failed".to_string()),
            turn_model: None,
            turn_usage: None,
        },
        TraceEventKind::TurnInterrupted { turn_id, reason } => TimelineItemDto {
            kind: "turn".to_string(),
            sequence,
            timestamp,
            turn_id: Some(turn_id.clone()),
            tool_call_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_status: None,
            tool_result: Some(reason.clone()),
            inference_model: None,
            inference_usage: None,
            turn_status: Some("interrupted".to_string()),
            turn_model: None,
            turn_usage: None,
        },
        TraceEventKind::InferenceStarted { model, .. } => TimelineItemDto {
            kind: "inference".to_string(),
            sequence,
            timestamp,
            turn_id: None,
            tool_call_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_status: None,
            tool_result: None,
            inference_model: Some(model.clone()),
            inference_usage: None,
            turn_status: None,
            turn_model: None,
            turn_usage: None,
        },
        TraceEventKind::InferenceCompleted { usage, .. } => TimelineItemDto {
            kind: "inference".to_string(),
            sequence,
            timestamp,
            turn_id: None,
            tool_call_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_status: None,
            tool_result: None,
            inference_model: None,
            inference_usage: Some(UsageDto {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                cached_prompt_tokens: usage.cached_prompt_tokens,
                total_tokens: usage.total_tokens,
            }),
            turn_status: None,
            turn_model: None,
            turn_usage: None,
        },
        TraceEventKind::ToolCallStarted {
            tool_call_id,
            name,
            arguments,
            ..
        } => TimelineItemDto {
            kind: "tool_call".to_string(),
            sequence,
            timestamp,
            turn_id: None,
            tool_call_id: Some(tool_call_id.clone()),
            tool_name: Some(name.clone()),
            tool_arguments: Some(arguments.clone()),
            tool_status: Some("started".to_string()),
            tool_result: None,
            inference_model: None,
            inference_usage: None,
            turn_status: None,
            turn_model: None,
            turn_usage: None,
        },
        TraceEventKind::ToolCallApproved { tool_call_id } => TimelineItemDto {
            kind: "tool_call".to_string(),
            sequence,
            timestamp,
            turn_id: None,
            tool_call_id: Some(tool_call_id.clone()),
            tool_name: None,
            tool_arguments: None,
            tool_status: Some("approved".to_string()),
            tool_result: None,
            inference_model: None,
            inference_usage: None,
            turn_status: None,
            turn_model: None,
            turn_usage: None,
        },
        TraceEventKind::ToolCallDenied {
            tool_call_id,
            reason,
            ..
        } => TimelineItemDto {
            kind: "tool_call".to_string(),
            sequence,
            timestamp,
            turn_id: None,
            tool_call_id: Some(tool_call_id.clone()),
            tool_name: None,
            tool_arguments: None,
            tool_status: Some("denied".to_string()),
            tool_result: Some(reason.clone()),
            inference_model: None,
            inference_usage: None,
            turn_status: None,
            turn_model: None,
            turn_usage: None,
        },
        TraceEventKind::ToolCallCompleted {
            tool_call_id,
            result,
            ..
        } => TimelineItemDto {
            kind: "tool_call".to_string(),
            sequence,
            timestamp,
            turn_id: None,
            tool_call_id: Some(tool_call_id.clone()),
            tool_name: None,
            tool_arguments: None,
            tool_status: Some("completed".to_string()),
            tool_result: Some(result.clone()),
            inference_model: None,
            inference_usage: None,
            turn_status: None,
            turn_model: None,
            turn_usage: None,
        },
        TraceEventKind::ToolCallFailed {
            tool_call_id,
            error,
            ..
        } => TimelineItemDto {
            kind: "tool_call".to_string(),
            sequence,
            timestamp,
            turn_id: None,
            tool_call_id: Some(tool_call_id.clone()),
            tool_name: None,
            tool_arguments: None,
            tool_status: Some("failed".to_string()),
            tool_result: Some(error.clone()),
            inference_model: None,
            inference_usage: None,
            turn_status: None,
            turn_model: None,
            turn_usage: None,
        },
    }
}

pub fn message_content_text(content: MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text,
        MessageContent::MultiPart(parts) => {
            serde_json::to_string_pretty(&parts).unwrap_or_default()
        }
    }
}

pub fn provider_settings_to_edit(
    input: ProviderSettingsInput,
    current: &PureConfig,
) -> CommandResult<pl_core::ProviderSettingsEdit> {
    Ok(pl_core::ProviderSettingsEdit {
        default_provider: input.default_provider_id,
        providers: input
            .providers
            .into_iter()
            .map(|provider| {
                let current_token = current
                    .providers
                    .get(&provider.id)
                    .and_then(|current_provider| current_provider.bearer_token.clone());
                provider_edit(provider, current_token)
            })
            .collect::<CommandResult<Vec<_>>>()?,
        roles: input.roles.into_iter().map(role_edit).collect(),
    })
}
