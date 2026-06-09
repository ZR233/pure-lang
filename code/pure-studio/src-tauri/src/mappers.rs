use std::collections::{BTreeMap, HashMap, HashSet};

use pl_core::{
    BuiltinMcpServerState, ConfigStore, McpAvailabilityKind, McpAvailabilitySnapshot,
    McpServerConfig, McpServerStatusKind, McpServerTransport, ModelCapabilityConfig, ModelConfig,
    ModelRole, ProjectRecord, ProviderConfig, ProviderEdit, ProviderKind, ProviderModelEdit,
    ProviderTemplateKind, PureConfig, RoleEdit, SessionRecord, SessionRuntimeRecord, SkillCatalog,
    SkillMetadata, SkillSourceKind, StudioAgentSnapshotRecord, StudioAgentTimelineEventRecord,
    StudioRuntime, TraceEvent, TraceEventKind, TurnResultStatus, builtin_mcp_server_ids,
    effective_mcp_servers, infer_provider_template_kind,
};
use pl_protocol::{Message, MessageContent, MessageRole};

use crate::dto::{
    AgentDto, AgentEventDto, ConfigDto, DiscoveredSkillsDto, KeyValueDto, McpHealthUpdateDto,
    McpServerDto, McpSettingsInput, ModelDto, PlanStateDto, ProjectDto, ProviderDto, ProviderInput,
    ProviderSettingsInput, ProviderTemplateDto, RoleDto, RoleInput, RuntimeCostAmountDto,
    RuntimeUsageDto, SessionDto, SessionRuntimeDto, SkillDto,
};
use crate::state::{CommandError, CommandResult};

#[cfg(test)]
pub fn config_dto(store: &ConfigStore) -> CommandResult<ConfigDto> {
    let config = store.load_or_default()?;
    config_dto_from_config(store, &config, &BTreeMap::new())
}

pub async fn config_dto_for_studio(studio: &StudioRuntime) -> CommandResult<ConfigDto> {
    let config = studio.config_store().load_or_default()?;
    let availability = studio.mcp_runtime().snapshots().await;
    config_dto_from_config(studio.config_store(), &config, &availability)
}

pub async fn mcp_health_update_dto(studio: &StudioRuntime) -> CommandResult<McpHealthUpdateDto> {
    let config = studio.config_store().load_or_default()?;
    let availability = studio.mcp_runtime().snapshots().await;
    Ok(McpHealthUpdateDto {
        mcp_servers: mcp_server_dtos_with_availability(&config, &availability),
        active_mcp_servers: studio.mcp_runtime().available_server_names().await,
    })
}

fn config_dto_from_config(
    store: &ConfigStore,
    config: &PureConfig,
    availability: &BTreeMap<String, McpAvailabilitySnapshot>,
) -> CommandResult<ConfigDto> {
    Ok(ConfigDto {
        toml: config.to_toml_pretty()?,
        permission_mode: config.runtime.permission_mode.label().to_string(),
        providers: provider_dtos(config),
        roles: role_dtos(config),
        templates: provider_template_dtos()?,
        mcp_servers: mcp_server_dtos_with_availability(config, availability),
        config_exists: store.config_exists(),
    })
}

pub fn discovered_skills_dto(catalog: SkillCatalog) -> DiscoveredSkillsDto {
    DiscoveredSkillsDto {
        project_dir: catalog.project_dir.to_string_lossy().to_string(),
        skills: catalog.skills.into_iter().map(skill_dto).collect(),
        warnings: catalog.warnings,
    }
}

pub fn skill_dto(skill: SkillMetadata) -> SkillDto {
    SkillDto {
        name: skill.name,
        description: skill.description,
        category: skill.category,
        platforms: skill.platforms,
        scope: skill_scope(skill.source).to_string(),
        path: skill.path.to_string_lossy().to_string(),
    }
}

fn skill_scope(source: SkillSourceKind) -> &'static str {
    match source {
        SkillSourceKind::Project => "project",
        SkillSourceKind::User => "user",
        SkillSourceKind::System => "system",
        SkillSourceKind::External => "external",
    }
}

pub async fn load_session_runtime_dto(
    studio: &StudioRuntime,
    session_id: &str,
) -> CommandResult<SessionRuntimeDto> {
    let record = studio.session_runtime(session_id).await?;
    let messages = studio.store().load_messages(session_id).await?;
    Ok(session_runtime_dto(
        record,
        active_skill_names_from_messages(&messages),
        studio.mcp_runtime().available_server_names().await,
    ))
}

pub fn session_runtime_dto(
    record: SessionRuntimeRecord,
    active_skills: Vec<String>,
    active_mcp_servers: Vec<String>,
) -> SessionRuntimeDto {
    let usage = runtime_usage_dto(pl_core::RuntimeUsageSnapshot {
        model: record.model,
        context_window: record.context_window,
        latest_context_tokens: record.latest_context_tokens,
        prompt_tokens: record.prompt_tokens,
        completion_tokens: record.completion_tokens,
        cached_prompt_tokens: record.cached_prompt_tokens,
        total_tokens: record.total_tokens,
        estimated_costs: record.estimated_costs,
        has_unpriced_usage: record.has_unpriced_usage,
        updated_at: record.updated_at,
    });
    SessionRuntimeDto {
        session_id: record.session_id,
        updated_at: usage.updated_at,
        usage,
        active_skills,
        active_mcp_servers,
    }
}

pub fn mcp_server_dtos_with_availability(
    config: &PureConfig,
    availability: &BTreeMap<String, McpAvailabilitySnapshot>,
) -> Vec<McpServerDto> {
    effective_mcp_servers(config)
        .into_values()
        .map(|server| {
            let snapshot = availability.get(&server.id);
            let fallback =
                availability_fallback(&server.status_kind, server.status_message.clone());
            McpServerDto {
                id: server.id,
                enabled: server.config.enabled,
                transport: server.config.transport.as_str().to_string(),
                command: server.config.command.clone(),
                args: server.config.args.clone(),
                env: key_value_dtos(&server.config.env),
                cwd: server.config.cwd.clone(),
                url: server.config.url.clone(),
                bearer_token_env_var: server.config.bearer_token_env_var.clone(),
                headers: key_value_dtos(&server.config.headers),
                endpoint: server.config.endpoint_summary(),
                source_kind: server.source_kind.as_str().to_string(),
                source_label: server.source_label,
                source_detail: server.source_detail,
                status_kind: server.status_kind.as_str().to_string(),
                status_message: server.status_message,
                mutation_policy: server.mutation_policy.as_str().to_string(),
                availability_kind: snapshot
                    .map(|snapshot| snapshot.availability_kind.as_str().to_string())
                    .unwrap_or_else(|| fallback.0.as_str().to_string()),
                availability_message: snapshot
                    .and_then(|snapshot| snapshot.availability_message.clone())
                    .or(fallback.1),
                last_checked_at: snapshot.and_then(|snapshot| snapshot.last_checked_at),
                tool_count: snapshot.and_then(|snapshot| snapshot.tool_count),
            }
        })
        .collect()
}

fn availability_fallback(
    status_kind: &McpServerStatusKind,
    status_message: Option<String>,
) -> (McpAvailabilityKind, Option<String>) {
    match status_kind {
        McpServerStatusKind::Enabled => (
            McpAvailabilityKind::Checking,
            Some("MCP health check has not completed".to_string()),
        ),
        McpServerStatusKind::Disabled => (
            McpAvailabilityKind::Disabled,
            Some("MCP server is disabled in configuration".to_string()),
        ),
        McpServerStatusKind::MissingCredential => {
            (McpAvailabilityKind::MissingCredential, status_message)
        }
    }
}

fn key_value_dtos(values: &BTreeMap<String, String>) -> Vec<KeyValueDto> {
    values
        .iter()
        .map(|(key, value)| KeyValueDto {
            key: key.clone(),
            value: value.clone(),
        })
        .collect()
}

fn active_skill_names_from_messages(messages: &[Message]) -> Vec<String> {
    let mut skills = Vec::new();
    let mut seen = HashSet::new();
    for message in messages {
        if message.role != MessageRole::Tool {
            continue;
        }
        if message.metadata.get("tool_name").map(String::as_str) != Some("skill_view") {
            continue;
        }
        let MessageContent::Text(content) = &message.content else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
            continue;
        };
        if value.get("success").and_then(serde_json::Value::as_bool) != Some(true) {
            continue;
        }
        let Some(name) = value
            .get("skill")
            .and_then(|skill| skill.get("name"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        if seen.insert(name.to_ascii_lowercase()) {
            skills.push(name.to_string());
        }
    }
    skills
}

pub fn runtime_usage_dto(usage: pl_core::RuntimeUsageSnapshot) -> RuntimeUsageDto {
    let cache_hit_rate = if usage.prompt_tokens == 0 {
        None
    } else {
        Some(usage.cached_prompt_tokens as f64 / usage.prompt_tokens as f64)
    };
    RuntimeUsageDto {
        model: usage.model,
        context_window: usage.context_window,
        latest_context_tokens: usage.latest_context_tokens,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cached_prompt_tokens: usage.cached_prompt_tokens,
        total_tokens: usage.total_tokens,
        cache_hit_rate,
        estimated_costs: usage
            .estimated_costs
            .into_iter()
            .map(|cost| RuntimeCostAmountDto {
                currency: cost.currency,
                amount: cost.amount,
            })
            .collect(),
        has_unpriced_usage: usage.has_unpriced_usage,
        updated_at: usage.updated_at,
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
        base_url: provider.base_url.clone(),
        bearer_token: String::new(),
        has_bearer_token: provider
            .bearer_token
            .as_ref()
            .is_some_and(|token| !token.trim().is_empty()),
        default_model: provider.default_model.clone(),
        model_count: models.len().to_string(),
        updated_at: "Loaded".to_string(),
        provider_kind: provider_kind_name(provider.provider_kind).to_string(),
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
        base_url: info.base_url,
        default_model: info.default_model,
        provider_kind: provider_kind_name(info.provider_kind).to_string(),
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

fn provider_kind_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAi => "open_ai",
        ProviderKind::DeepSeek => "deep_seek",
        ProviderKind::Zhipu => "zhipu",
    }
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

pub fn agent_event_dtos(events: Vec<StudioAgentTimelineEventRecord>) -> Vec<AgentEventDto> {
    events.into_iter().map(agent_event_dto).collect()
}

pub fn agent_event_dto(event: StudioAgentTimelineEventRecord) -> AgentEventDto {
    AgentEventDto {
        event_id: event.event_id,
        session_id: event.session_id,
        sequence: event.sequence,
        kind: event.kind,
        agent_id: event.agent_id,
        path: event.path,
        parent_path: event.parent_path,
        payload: serde_json::from_str(&event.payload_json).unwrap_or(serde_json::Value::Null),
        created_at: event.created_at,
    }
}

pub fn agent_dtos(agents: Vec<StudioAgentSnapshotRecord>) -> Vec<AgentDto> {
    agents.into_iter().map(agent_dto).collect()
}

pub fn agent_dto(agent: StudioAgentSnapshotRecord) -> AgentDto {
    AgentDto {
        id: agent.id,
        session_id: agent.session_id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status.as_str().to_string(),
        summary: agent.summary,
        depth: agent.depth,
        reason: agent.reason,
        error: agent.error,
        budget_limit_kind: agent
            .budget_limit_kind
            .map(|kind| kind.as_str().to_string()),
        budget_usage: agent.budget_usage.map(|usage| crate::dto::BudgetUsageDto {
            model_steps: usage.model_steps,
            tool_calls: usage.tool_calls,
            wait_calls: usage.wait_calls,
            elapsed_ms: usage.elapsed_ms,
        }),
        runtime_usage: agent.runtime_usage.map(runtime_usage_dto),
        updated_at: agent.updated_at,
    }
}

pub fn turn_result_status_label(status: TurnResultStatus) -> &'static str {
    match status {
        TurnResultStatus::Completed => "completed",
        TurnResultStatus::Aborted => "aborted",
        TurnResultStatus::Errored => "errored",
    }
}

pub fn timeline_events_to_items(events: &[TraceEvent]) -> Vec<pl_protocol::TimelineItem> {
    let mut items = std::collections::HashMap::new();
    for event in events {
        match &event.kind {
            TraceEventKind::TimelineItemStarted { item } => upsert_timeline_item(&mut items, item),
            TraceEventKind::TimelineItemCompleted { item } => {
                upsert_timeline_item(&mut items, item)
            }
            TraceEventKind::TimelineItemFailed { item, error } => {
                let mut failed = item.clone();
                if failed.content.trim().is_empty() {
                    failed.content = error.clone();
                }
                upsert_timeline_item(&mut items, &failed)
            }
            TraceEventKind::TimelineItemDelta { event } => {
                let entry = items.entry(event.item_id.clone()).or_insert_with(|| {
                    pl_protocol::TimelineItem {
                        turn_id: event.turn_id.clone(),
                        item_id: event.item_id.clone(),
                        sequence: event.sequence,
                        kind: event.kind,
                        status: event.status,
                        created_at: event.created_at,
                        updated_at: event.updated_at,
                        role: None,
                        content: String::new(),
                        thinking_chunks: Vec::new(),
                        tool: None,
                        agent: None,
                        inference: None,
                        usage: None,
                    }
                });
                entry.status = event.status;
                entry.updated_at = event.updated_at;
                match &event.delta {
                    pl_protocol::TimelineDelta::Text { delta } => {
                        entry.content.push_str(delta);
                    }
                    pl_protocol::TimelineDelta::Plan { delta } => {
                        entry.content.push_str(delta);
                    }
                    pl_protocol::TimelineDelta::Thinking { chunk_index, delta } => {
                        match entry
                            .thinking_chunks
                            .iter_mut()
                            .find(|chunk| chunk.chunk_index == *chunk_index)
                        {
                            Some(chunk) => chunk.content.push_str(delta),
                            None => {
                                entry
                                    .thinking_chunks
                                    .push(pl_protocol::TimelineThinkingChunk {
                                        chunk_index: *chunk_index,
                                        content: delta.clone(),
                                    })
                            }
                        }
                    }
                    pl_protocol::TimelineDelta::ToolArguments { delta } => {
                        let tool = entry
                            .tool
                            .get_or_insert_with(|| blank_timeline_tool_item(&event.item_id));
                        tool.arguments.push_str(delta);
                    }
                    pl_protocol::TimelineDelta::ToolResult { delta } => {
                        let tool = entry
                            .tool
                            .get_or_insert_with(|| blank_timeline_tool_item(&event.item_id));
                        let result = tool.result.get_or_insert_with(String::new);
                        result.push_str(delta);
                    }
                }
            }
            TraceEventKind::PlanLifecycleChanged { .. } => {}
        }
    }
    let mut items = items.into_values().collect::<Vec<_>>();
    items.sort_by_key(|item| item.sequence);
    items
}

pub fn plan_lifecycle_events_to_states(events: &[TraceEvent]) -> Vec<PlanStateDto> {
    let mut latest: HashMap<String, (u64, PlanStateDto)> = HashMap::new();
    for trace in events {
        let TraceEventKind::PlanLifecycleChanged { event } = &trace.kind else {
            continue;
        };
        let dto = PlanStateDto {
            plan_id: event.plan_id.clone(),
            state: event.state.as_str().to_string(),
            turn_id: event.turn_id.clone(),
            reason: event.reason.clone(),
            updated_at: event.updated_at,
        };
        match latest.get(&event.plan_id) {
            Some((sequence, _)) if *sequence > trace.sequence => {}
            _ => {
                latest.insert(event.plan_id.clone(), (trace.sequence, dto));
            }
        }
    }
    let mut states = latest
        .into_values()
        .map(|(_, state)| state)
        .collect::<Vec<_>>();
    states.sort_by(|left, right| left.plan_id.cmp(&right.plan_id));
    states
}

fn upsert_timeline_item(
    items: &mut std::collections::HashMap<String, pl_protocol::TimelineItem>,
    item: &pl_protocol::TimelineItem,
) {
    let mut next = item.clone();
    if let Some(existing) = items.get(&item.item_id) {
        next.sequence = existing.sequence;
        next.created_at = existing.created_at;
        if next.content.is_empty() {
            next.content = existing.content.clone();
        }
        if next.thinking_chunks.is_empty() {
            next.thinking_chunks = existing.thinking_chunks.clone();
        }
        next.tool = merge_timeline_tool_item(existing.tool.clone(), next.tool);
        next.agent = next.agent.or_else(|| existing.agent.clone());
        next.inference = next.inference.or_else(|| existing.inference.clone());
        next.usage = next.usage.or_else(|| existing.usage.clone());
    }
    items.insert(item.item_id.clone(), next);
}

fn blank_timeline_tool_item(item_id: &str) -> pl_protocol::TimelineToolItem {
    pl_protocol::TimelineToolItem {
        tool_call_id: item_id.to_string(),
        call_id: None,
        provider_item_id: None,
        name: String::new(),
        arguments: String::new(),
        result: None,
        exit_code: None,
        timed_out: false,
        working_directory: None,
        denial_reason: None,
    }
}

fn merge_timeline_tool_item(
    existing: Option<pl_protocol::TimelineToolItem>,
    incoming: Option<pl_protocol::TimelineToolItem>,
) -> Option<pl_protocol::TimelineToolItem> {
    match (existing, incoming) {
        (None, None) => None,
        (Some(tool), None) | (None, Some(tool)) => Some(tool),
        (Some(existing), Some(mut incoming)) => {
            if incoming.name.is_empty() {
                incoming.name = existing.name;
            }
            if incoming.arguments.is_empty() {
                incoming.arguments = existing.arguments;
            }
            if incoming.result.is_none() {
                incoming.result = existing.result;
            }
            if incoming.exit_code.is_none() {
                incoming.exit_code = existing.exit_code;
            }
            incoming.timed_out = incoming.timed_out || existing.timed_out;
            if incoming.working_directory.is_none() {
                incoming.working_directory = existing.working_directory;
            }
            if incoming.denial_reason.is_none() {
                incoming.denial_reason = existing.denial_reason;
            }
            if incoming.call_id.is_none() {
                incoming.call_id = existing.call_id;
            }
            if incoming.provider_item_id.is_none() {
                incoming.provider_item_id = existing.provider_item_id;
            }
            Some(incoming)
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

pub fn mcp_settings_to_servers(
    input: McpSettingsInput,
) -> CommandResult<BTreeMap<String, McpServerConfig>> {
    let mut servers = BTreeMap::new();
    for server in input.servers {
        let id = non_empty(server.id, "mcp server id")?;
        if builtin_mcp_server_ids().contains(&id.as_str()) {
            continue;
        }
        if servers.contains_key(&id) {
            return Err(CommandError::from_display(format!(
                "duplicate mcp server id: {id}"
            )));
        }
        let transport = match server.transport.as_str() {
            "stdio" => McpServerTransport::Stdio,
            "streamableHttp" => McpServerTransport::StreamableHttp,
            value => {
                return Err(CommandError::from_display(format!(
                    "unsupported mcp transport: {value}"
                )));
            }
        };
        servers.insert(
            id,
            McpServerConfig {
                enabled: server.enabled,
                transport,
                command: optional_non_empty(server.command),
                args: server
                    .args
                    .into_iter()
                    .map(|arg| arg.trim().to_string())
                    .filter(|arg| !arg.is_empty())
                    .collect(),
                env: key_value_map(server.env, "env")?,
                cwd: optional_non_empty(server.cwd),
                url: optional_non_empty(server.url),
                bearer_token_env_var: optional_non_empty(server.bearer_token_env_var),
                headers: key_value_map(server.headers, "headers")?,
            },
        );
    }
    Ok(servers)
}

pub fn mcp_settings_to_builtin_states(
    input: &McpSettingsInput,
    current: &PureConfig,
) -> BTreeMap<String, BuiltinMcpServerState> {
    let mut states = current.builtin_mcp_servers.clone();
    for server in &input.servers {
        let id = server.id.trim();
        if builtin_mcp_server_ids().contains(&id) {
            states.insert(
                id.to_string(),
                BuiltinMcpServerState {
                    enabled: server.enabled,
                },
            );
        }
    }
    states
}

fn key_value_map(values: Vec<KeyValueDto>, label: &str) -> CommandResult<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    for value in values {
        let key = value.key.trim().to_string();
        if key.is_empty() && value.value.trim().is_empty() {
            continue;
        }
        if key.is_empty() {
            return Err(CommandError::from_display(format!(
                "{label} key is required"
            )));
        }
        if map.insert(key.clone(), value.value).is_some() {
            return Err(CommandError::from_display(format!(
                "duplicate {label} key: {key}"
            )));
        }
    }
    Ok(map)
}

fn non_empty(value: String, label: &str) -> CommandResult<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Err(CommandError::from_display(format!("{label} is required")));
    }
    Ok(trimmed)
}

fn optional_non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use pl_core::{
        ConfigPaths, ConfigStore, McpServerConfig, PermissionMode, PureConfig,
        RuntimeUsageSnapshot, SessionRuntimeRecord, StudioAgentSnapshotRecord,
    };
    use pl_protocol::{
        AgentStatus, PlanLifecycleEvent, PlanLifecycleState, RuntimeCostAmount, TimelineDelta,
        TimelineItem, TimelineItemDeltaEvent, TimelineItemKind, TimelineItemStatus,
        TimelineTextRole, TimelineToolItem, TraceEvent, TraceEventKind,
    };
    use pretty_assertions::assert_eq;

    use crate::dto::McpServerInput;

    use super::*;

    #[test]
    fn config_dto_exposes_permission_mode() {
        let home = std::env::temp_dir().join(format!(
            "pure-studio-config-dto-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ConfigStore::new(ConfigPaths::from_home(&home));
        let mut config = PureConfig::default_config();
        config.runtime.permission_mode = PermissionMode::FullAccess;
        config.mcp_servers.insert(
            "filesystem".to_string(),
            McpServerConfig {
                command: Some("npx".to_string()),
                args: vec!["-y".to_string(), "mcp-server".to_string()],
                ..Default::default()
            },
        );
        store.save(&config).unwrap();

        let dto = config_dto(&store).unwrap();

        assert_eq!(dto.permission_mode, "full-access");
        assert_eq!(dto.mcp_servers.len(), 5);
        assert_eq!(dto.mcp_servers[0].id, "filesystem");
        assert_eq!(dto.mcp_servers[0].transport, "stdio");
        assert_eq!(dto.mcp_servers[0].source_kind, "user");
        let zhipu_search = dto
            .mcp_servers
            .iter()
            .find(|server| server.id == "zhipu_search")
            .unwrap();
        assert_eq!(zhipu_search.source_kind, "builtIn");
        assert_eq!(zhipu_search.status_kind, "missingCredential");
        assert_eq!(zhipu_search.mutation_policy, "lockedIdentity");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn provider_template_dtos_include_zhipu_coding_plan() {
        let templates = provider_template_dtos().unwrap();
        let zhipu = templates
            .iter()
            .find(|template| template.id == "zhipu")
            .unwrap();
        let coding_plan = templates
            .iter()
            .find(|template| template.id == "zhipu-coding-plan")
            .unwrap();

        assert_eq!(coding_plan.name, "Zhipu Coding Plan");
        assert_eq!(
            coding_plan.base_url,
            "https://open.bigmodel.cn/api/coding/paas/v4"
        );
        assert_eq!(coding_plan.provider_kind, "zhipu");
        assert_eq!(
            coding_plan
                .default_models
                .iter()
                .map(|model| model.slug.as_str())
                .collect::<Vec<_>>(),
            zhipu
                .default_models
                .iter()
                .map(|model| model.slug.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn mcp_settings_to_servers_ignores_builtin_servers_from_client_payload() {
        let input = McpSettingsInput {
            servers: vec![
                McpServerInput {
                    id: "zhipu_search".to_string(),
                    enabled: true,
                    transport: "streamableHttp".to_string(),
                    command: None,
                    args: Vec::new(),
                    env: Vec::new(),
                    cwd: None,
                    url: Some("https://open.bigmodel.cn/api/mcp/web_search_prime/mcp".to_string()),
                    bearer_token_env_var: None,
                    headers: Vec::new(),
                },
                McpServerInput {
                    id: "github".to_string(),
                    enabled: true,
                    transport: "streamableHttp".to_string(),
                    command: None,
                    args: Vec::new(),
                    env: Vec::new(),
                    cwd: None,
                    url: Some("https://example.com/mcp".to_string()),
                    bearer_token_env_var: None,
                    headers: Vec::new(),
                },
            ],
        };
        let states = mcp_settings_to_builtin_states(&input, &PureConfig::default_config());
        let servers = mcp_settings_to_servers(input).unwrap();

        assert!(!servers.contains_key("zhipu_search"));
        assert!(servers.contains_key("github"));
        assert_eq!(states["zhipu_search"].enabled, true);
    }

    #[test]
    fn discovered_skills_dto_maps_scopes_and_warnings() {
        let dto = discovered_skills_dto(SkillCatalog {
            project_dir: PathBuf::from("C:/work/app/skills"),
            skills: vec![
                test_skill("project-skill", SkillSourceKind::Project),
                test_skill("user-skill", SkillSourceKind::User),
                test_skill("system-skill", SkillSourceKind::System),
                test_skill("external-skill", SkillSourceKind::External),
            ],
            warnings: vec!["bad skill".to_string()],
        });

        assert_eq!(dto.project_dir, "C:/work/app/skills");
        assert_eq!(
            dto.skills
                .iter()
                .map(|skill| skill.scope.as_str())
                .collect::<Vec<_>>(),
            vec!["project", "user", "system", "external"],
        );
        assert_eq!(dto.warnings, vec!["bad skill"]);
    }

    fn test_skill(name: &str, source: SkillSourceKind) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: format!("{name} description"),
            category: Some("demo".to_string()),
            platforms: vec!["windows".to_string()],
            source,
            path: PathBuf::from(format!("C:/skills/{name}")),
        }
    }

    #[test]
    fn session_runtime_dto_exposes_nested_runtime_usage_and_costs() {
        let dto = session_runtime_dto(
            SessionRuntimeRecord {
                session_id: "session-1".to_string(),
                model: "model-a".to_string(),
                context_window: Some(1_000_000),
                latest_context_tokens: 42_000,
                prompt_tokens: 100_000,
                completion_tokens: 20_000,
                cached_prompt_tokens: 25_000,
                total_tokens: 120_000,
                currency: None,
                estimated_cost: None,
                estimated_costs: vec![
                    RuntimeCostAmount {
                        currency: "CNY".to_string(),
                        amount: 0.12,
                    },
                    RuntimeCostAmount {
                        currency: "USD".to_string(),
                        amount: 0.03,
                    },
                ],
                has_unpriced_usage: true,
                updated_at: 99,
            },
            vec!["skill-creator".to_string()],
            vec!["github".to_string()],
        );

        assert_eq!(dto.session_id, "session-1");
        assert_eq!(dto.updated_at, 99);
        assert_eq!(dto.active_skills, vec!["skill-creator"]);
        assert_eq!(dto.active_mcp_servers, vec!["github"]);
        assert_eq!(dto.usage.model, "model-a");
        assert_eq!(dto.usage.context_window, Some(1_000_000));
        assert_eq!(dto.usage.cache_hit_rate, Some(0.25));
        assert!(dto.usage.has_unpriced_usage);
        assert_eq!(
            dto.usage
                .estimated_costs
                .iter()
                .map(|cost| cost.currency.as_str())
                .collect::<Vec<_>>(),
            vec!["CNY", "USD"],
        );
    }

    #[test]
    fn active_skill_names_from_messages_extracts_successful_skill_view_results() {
        let messages = vec![
            tool_result_message(
                "skill_view",
                r#"{
                    "success": true,
                    "skill": {"name": "skill-creator"},
                    "filePath": "SKILL.md",
                    "content": "body"
                }"#,
            ),
            tool_result_message(
                "skill_view",
                r#"{
                    "success": true,
                    "skill": {"name": "subagent-workflow"},
                    "filePath": "references/example.md",
                    "content": "reference"
                }"#,
            ),
        ];

        let skills = active_skill_names_from_messages(&messages);

        assert_eq!(skills, vec!["skill-creator", "subagent-workflow"]);
    }

    #[test]
    fn active_skill_names_from_messages_dedupes_and_ignores_non_active_results() {
        let messages = vec![
            tool_result_message(
                "skill_view",
                r#"{"success":true,"skill":{"name":"skill-creator"},"content":"body"}"#,
            ),
            tool_result_message(
                "skill_view",
                r#"{"success":true,"skill":{"name":"Skill-Creator"},"content":"body again"}"#,
            ),
            tool_result_message("skills_list", r#"{"success":true,"skills":[]}"#),
            tool_result_message(
                "skill_view",
                r#"{"success":false,"skill":{"name":"failed-skill"}}"#,
            ),
            tool_result_message("skill_view", "not json"),
            assistant_message("plain answer"),
        ];

        let skills = active_skill_names_from_messages(&messages);

        assert_eq!(skills, vec!["skill-creator"]);
    }

    fn tool_result_message(tool_name: &str, content: &str) -> Message {
        let mut metadata = HashMap::new();
        metadata.insert("tool_name".to_string(), tool_name.to_string());
        Message {
            role: MessageRole::Tool,
            content: MessageContent::Text(content.to_string()),
            reasoning_content: None,
            metadata,
        }
    }

    fn assistant_message(content: &str) -> Message {
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text(content.to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn agent_dto_includes_runtime_usage() {
        let dto = agent_dto(StudioAgentSnapshotRecord {
            id: "agent-1".to_string(),
            session_id: "session-1".to_string(),
            path: "/root/research".to_string(),
            parent_path: Some("/root".to_string()),
            role: "executor".to_string(),
            task: "research".to_string(),
            status: AgentStatus::Completed,
            summary: Some("done".to_string()),
            depth: 1,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            runtime_usage: Some(RuntimeUsageSnapshot {
                model: "model-b".to_string(),
                context_window: Some(400_000),
                latest_context_tokens: 12_000,
                prompt_tokens: 24_000,
                completion_tokens: 2_000,
                cached_prompt_tokens: 6_000,
                total_tokens: 26_000,
                estimated_costs: vec![RuntimeCostAmount {
                    currency: "USD".to_string(),
                    amount: 0.04,
                }],
                has_unpriced_usage: false,
                updated_at: 123,
            }),
            updated_at: 124,
        });

        let usage = dto.runtime_usage.expect("runtime usage");
        assert_eq!(dto.id, "agent-1");
        assert_eq!(usage.model, "model-b");
        assert_eq!(usage.cache_hit_rate, Some(0.25));
        assert_eq!(usage.estimated_costs[0].currency, "USD");
        assert!((usage.estimated_costs[0].amount - 0.04).abs() < 0.000_001);
    }

    #[test]
    fn timeline_events_to_items_folds_start_delta_and_completed_snapshot() {
        let started = TimelineItem::text(
            "turn-1",
            "turn-1-assistant",
            1,
            TimelineTextRole::Assistant,
            "",
            TimelineItemStatus::Streaming,
            10,
        );
        let completed = TimelineItem::text(
            "turn-1",
            "turn-1-assistant",
            3,
            TimelineTextRole::Assistant,
            "hello world",
            TimelineItemStatus::Completed,
            12,
        );
        let events = vec![
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 1,
                timestamp: 10,
                kind: TraceEventKind::TimelineItemStarted { item: started },
            },
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 2,
                timestamp: 11,
                kind: TraceEventKind::TimelineItemDelta {
                    event: TimelineItemDeltaEvent {
                        turn_id: "turn-1".to_string(),
                        item_id: "turn-1-assistant".to_string(),
                        sequence: 2,
                        kind: TimelineItemKind::Text,
                        status: TimelineItemStatus::Streaming,
                        created_at: 10,
                        updated_at: 11,
                        delta: TimelineDelta::Text {
                            delta: "hello".to_string(),
                        },
                    },
                },
            },
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 3,
                timestamp: 12,
                kind: TraceEventKind::TimelineItemCompleted { item: completed },
            },
        ];

        let items = timeline_events_to_items(&events);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item_id, "turn-1-assistant");
        assert_eq!(items[0].sequence, 1);
        assert_eq!(items[0].content, "hello world");
        assert_eq!(items[0].status, TimelineItemStatus::Completed);
    }

    #[test]
    fn timeline_events_to_items_ignores_plan_lifecycle_events() {
        let events = vec![TraceEvent {
            session_id: "session-1".to_string(),
            sequence: 1,
            timestamp: 10,
            kind: TraceEventKind::PlanLifecycleChanged {
                event: PlanLifecycleEvent {
                    plan_id: "turn-1-plan".to_string(),
                    state: PlanLifecycleState::Dismissed,
                    turn_id: None,
                    reason: Some("dismissed".to_string()),
                    updated_at: 10,
                },
            },
        }];

        assert!(timeline_events_to_items(&events).is_empty());
    }

    #[test]
    fn plan_lifecycle_events_to_states_keeps_latest_per_plan() {
        let events = vec![
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 1,
                timestamp: 10,
                kind: TraceEventKind::PlanLifecycleChanged {
                    event: PlanLifecycleEvent {
                        plan_id: "turn-1-plan".to_string(),
                        state: PlanLifecycleState::Accepted,
                        turn_id: None,
                        reason: None,
                        updated_at: 10,
                    },
                },
            },
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 2,
                timestamp: 11,
                kind: TraceEventKind::PlanLifecycleChanged {
                    event: PlanLifecycleEvent {
                        plan_id: "turn-1-plan".to_string(),
                        state: PlanLifecycleState::ImplementationFailed,
                        turn_id: Some("turn-2".to_string()),
                        reason: Some("provider error".to_string()),
                        updated_at: 11,
                    },
                },
            },
        ];

        let states = plan_lifecycle_events_to_states(&events);

        assert_eq!(states.len(), 1);
        assert_eq!(states[0].plan_id, "turn-1-plan");
        assert_eq!(states[0].state, "implementationFailed");
        assert_eq!(states[0].turn_id.as_deref(), Some("turn-2"));
        assert_eq!(states[0].reason.as_deref(), Some("provider error"));
    }

    #[test]
    fn timeline_events_to_items_preserves_tool_delta_before_start() {
        let started = TimelineItem {
            turn_id: "turn-1".to_string(),
            item_id: "turn-1-call-1".to_string(),
            sequence: 9,
            kind: TimelineItemKind::Tool,
            status: TimelineItemStatus::Streaming,
            created_at: 9,
            updated_at: 9,
            role: None,
            content: String::new(),
            thinking_chunks: Vec::new(),
            tool: Some(TimelineToolItem {
                tool_call_id: "turn-1-call-1".to_string(),
                call_id: Some("call-1".to_string()),
                provider_item_id: Some("provider-1".to_string()),
                name: "read_file".to_string(),
                arguments: String::new(),
                result: None,
                exit_code: None,
                timed_out: false,
                working_directory: None,
                denial_reason: None,
            }),
            agent: None,
            inference: None,
            usage: None,
        };
        let completed = TimelineItem {
            status: TimelineItemStatus::Completed,
            updated_at: 11,
            ..started.clone()
        };
        let events = vec![
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 10,
                timestamp: 10,
                kind: TraceEventKind::TimelineItemDelta {
                    event: TimelineItemDeltaEvent {
                        turn_id: "turn-1".to_string(),
                        item_id: "turn-1-call-1".to_string(),
                        sequence: 10,
                        kind: TimelineItemKind::Tool,
                        status: TimelineItemStatus::Streaming,
                        created_at: 10,
                        updated_at: 10,
                        delta: TimelineDelta::ToolArguments {
                            delta: "{\"path\":\"a.ts\"".to_string(),
                        },
                    },
                },
            },
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 9,
                timestamp: 9,
                kind: TraceEventKind::TimelineItemStarted { item: started },
            },
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 11,
                timestamp: 11,
                kind: TraceEventKind::TimelineItemCompleted { item: completed },
            },
        ];

        let items = timeline_events_to_items(&events);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].sequence, 10);
        let tool = items[0].tool.as_ref().expect("tool item");
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.arguments, "{\"path\":\"a.ts\"");
        assert_eq!(tool.call_id.as_deref(), Some("call-1"));
        assert_eq!(tool.provider_item_id.as_deref(), Some("provider-1"));
    }

    #[test]
    fn timeline_events_to_items_preserves_tool_result_delta_before_start() {
        let completed = TimelineItem {
            turn_id: "turn-1".to_string(),
            item_id: "turn-1-call-1".to_string(),
            sequence: 11,
            kind: TimelineItemKind::Tool,
            status: TimelineItemStatus::Completed,
            created_at: 11,
            updated_at: 11,
            role: None,
            content: String::new(),
            thinking_chunks: Vec::new(),
            tool: Some(TimelineToolItem {
                tool_call_id: "turn-1-call-1".to_string(),
                call_id: Some("call-1".to_string()),
                provider_item_id: Some("provider-1".to_string()),
                name: "read_file".to_string(),
                arguments: "{\"path\":\"a.ts\"}".to_string(),
                result: None,
                exit_code: None,
                timed_out: false,
                working_directory: None,
                denial_reason: None,
            }),
            agent: None,
            inference: None,
            usage: None,
        };
        let events = vec![
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 10,
                timestamp: 10,
                kind: TraceEventKind::TimelineItemDelta {
                    event: TimelineItemDeltaEvent {
                        turn_id: "turn-1".to_string(),
                        item_id: "turn-1-call-1".to_string(),
                        sequence: 10,
                        kind: TimelineItemKind::Tool,
                        status: TimelineItemStatus::Streaming,
                        created_at: 10,
                        updated_at: 10,
                        delta: TimelineDelta::ToolResult {
                            delta: "partial result".to_string(),
                        },
                    },
                },
            },
            TraceEvent {
                session_id: "session-1".to_string(),
                sequence: 11,
                timestamp: 11,
                kind: TraceEventKind::TimelineItemCompleted { item: completed },
            },
        ];

        let items = timeline_events_to_items(&events);

        assert_eq!(items.len(), 1);
        let tool = items[0].tool.as_ref().expect("tool item");
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.arguments, "{\"path\":\"a.ts\"}");
        assert_eq!(tool.result.as_deref(), Some("partial result"));
        assert_eq!(tool.call_id.as_deref(), Some("call-1"));
        assert_eq!(tool.provider_item_id.as_deref(), Some("provider-1"));
    }

    #[test]
    fn timeline_events_to_items_keeps_failed_error_when_content_is_empty() {
        let failed = TimelineItem {
            turn_id: "turn-1".to_string(),
            item_id: "turn-1-turn".to_string(),
            sequence: 1,
            kind: TimelineItemKind::Turn,
            status: TimelineItemStatus::Failed,
            created_at: 10,
            updated_at: 10,
            role: None,
            content: String::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        };
        let events = vec![TraceEvent {
            session_id: "session-1".to_string(),
            sequence: 1,
            timestamp: 10,
            kind: TraceEventKind::TimelineItemFailed {
                item: failed,
                error: "LLM provider error: missing API key".to_string(),
            },
        }];

        let items = timeline_events_to_items(&events);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item_id, "turn-1-turn");
        assert_eq!(items[0].status, TimelineItemStatus::Failed);
        assert_eq!(items[0].content, "LLM provider error: missing API key");
    }
}
