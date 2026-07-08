use crate::api::studio::runtime::BridgeRuntime;
use crate::api::studio::types::{
    BridgeActiveTurn, BridgeLspHealthDto, BridgeMcpHealthDto, BridgeMcpServerDto,
    BridgeRuntimeCostAmountDto, BridgeRuntimeStatus, BridgeSessionRuntimeDto,
    BridgeSkillActivationDto, RuntimeSnapshot,
};
use anyhow::Result;
use pl_core::StudioRuntimeSnapshot as CoreRuntimeSnapshot;
use pl_protocol::{
    RuntimeCostAmount, SkillActivation, StudioLspHealth, StudioMcpHealth, StudioSessionRuntime,
};
// ── Core conversion functions ──

pub(crate) fn runtime_snapshot(snapshot: CoreRuntimeSnapshot) -> RuntimeSnapshot {
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

pub(crate) async fn bridge_session_runtime_view(
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

pub(crate) fn bridge_session_runtime(snapshot: StudioSessionRuntime) -> BridgeSessionRuntimeDto {
    BridgeSessionRuntimeDto {
        session_id: snapshot.session_id,
        model: snapshot.usage.model,
        context_window: snapshot.usage.context_window,
        latest_context_tokens: snapshot.usage.latest_context_tokens,
        prompt_tokens: snapshot.usage.prompt_tokens,
        completion_tokens: snapshot.usage.completion_tokens,
        cached_prompt_tokens: snapshot.usage.cached_prompt_tokens,
        total_tokens: snapshot.usage.total_tokens,
        estimated_costs: snapshot
            .usage
            .estimated_costs
            .into_iter()
            .map(bridge_cost_amount)
            .collect(),
        has_unpriced_usage: snapshot.usage.has_unpriced_usage,
        active_skills: snapshot.active_skills,
        active_mcp_servers: snapshot.active_mcp_servers,
        active_lsp_servers: snapshot.active_lsp_servers,
        updated_at: snapshot.updated_at,
    }
}

pub(crate) fn bridge_cost_amount(amount: RuntimeCostAmount) -> BridgeRuntimeCostAmountDto {
    BridgeRuntimeCostAmountDto {
        currency: amount.currency,
        amount: amount.amount,
    }
}

pub(crate) fn bridge_skill_activation(activation: SkillActivation) -> BridgeSkillActivationDto {
    BridgeSkillActivationDto {
        name: activation.name,
        source: activation.source.as_str().to_string(),
        path: activation.path,
        turn_id: activation.turn_id,
        tool_call_id: activation.tool_call_id,
        activated_at: activation.activated_at,
    }
}

pub(crate) fn bridge_mcp_health(health: StudioMcpHealth) -> BridgeMcpHealthDto {
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
                source_kind: server.source_kind,
                status_kind: server.status_kind,
                mutation_policy: server.mutation_policy,
                availability_kind: server.availability_kind,
            })
            .collect(),
    }
}

pub(crate) fn bridge_lsp_health(health: StudioLspHealth) -> BridgeLspHealthDto {
    BridgeLspHealthDto {
        active_lsp_servers: health.active_lsp_servers,
    }
}
