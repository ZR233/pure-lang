use crate::api::studio::runtime::BridgeRuntime;
use crate::api::studio::types::{
    BridgeActiveTurn, BridgeLspHealthDto, BridgeMcpHealthDto, BridgeMcpServerDto,
    BridgeRuntimeCostAmountDto, BridgeRuntimeStatus, BridgeSessionRuntimeDto,
    BridgeSkillActivationDto, BridgeTaskAgentDto, BridgeTaskMergeDto, BridgeTaskReviewDto,
    BridgeTaskRuntimeDto, BridgeTaskWorkUnitDto, RuntimeSnapshot,
};
use anyhow::Result;
use pl_protocol::{RuntimeCostAmount, SkillActivation};
use pl_studio_runtime::{
    StudioLspHealth, StudioMcpHealth, StudioRuntimeSnapshot as CoreRuntimeSnapshot,
    StudioSessionRuntime,
};
// ── Core conversion functions ──

pub(crate) fn runtime_snapshot(snapshot: CoreRuntimeSnapshot) -> RuntimeSnapshot {
    RuntimeSnapshot {
        status: match snapshot.status {
            pl_studio_runtime::StudioRuntimeStatus::Uninitialized => {
                BridgeRuntimeStatus::Uninitialized
            }
            pl_studio_runtime::StudioRuntimeStatus::Initializing => {
                BridgeRuntimeStatus::Initializing
            }
            pl_studio_runtime::StudioRuntimeStatus::Ready => BridgeRuntimeStatus::Ready,
            pl_studio_runtime::StudioRuntimeStatus::ShuttingDown => {
                BridgeRuntimeStatus::ShuttingDown
            }
            pl_studio_runtime::StudioRuntimeStatus::Stopped => BridgeRuntimeStatus::Stopped,
            pl_studio_runtime::StudioRuntimeStatus::Failed => BridgeRuntimeStatus::Failed,
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
    let runtime = bridge.studio.session_runtime_view(session_id).await?;
    let active_skills = bridge
        .studio
        .store()
        .list_session_skill_names(session_id)
        .await?;
    Ok(BridgeSessionRuntimeDto {
        session_id: runtime.session_id,
        model: runtime.usage.model,
        context_window: runtime.usage.context_window,
        latest_context_tokens: runtime.usage.latest_context_tokens,
        prompt_tokens: runtime.usage.prompt_tokens,
        completion_tokens: runtime.usage.completion_tokens,
        cached_prompt_tokens: runtime.usage.cached_prompt_tokens,
        total_tokens: runtime.usage.total_tokens,
        estimated_costs: runtime
            .usage
            .estimated_costs
            .into_iter()
            .map(bridge_cost_amount)
            .collect(),
        has_unpriced_usage: runtime.usage.has_unpriced_usage,
        active_skills,
        active_mcp_servers: bridge.studio.mcp_runtime().available_server_names().await,
        active_lsp_servers: bridge.studio.lsp_runtime().active_server_names().await,
        task: runtime.task.map(bridge_task_runtime),
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
        task: snapshot.task.map(bridge_task_runtime),
        updated_at: snapshot.updated_at,
    }
}

fn bridge_task_runtime(task: pl_studio_runtime::StudioTaskRuntime) -> BridgeTaskRuntimeDto {
    BridgeTaskRuntimeDto {
        run_id: task.run_id,
        phase: task.phase,
        branch: task.branch,
        expected_head: task.expected_head,
        status_message: task.status_message,
        work_units: task
            .work_units
            .into_iter()
            .map(|unit| BridgeTaskWorkUnitDto {
                id: unit.id,
                title: unit.title,
                status: unit.status,
                worktree_path: unit.worktree_path,
                branch: unit.branch,
                agent_id: unit.agent_id,
            })
            .collect(),
        agents: task
            .agents
            .into_iter()
            .map(|agent| BridgeTaskAgentDto {
                agent_id: agent.agent_id,
                role: agent.role,
                status: agent.status,
                initiated_by: agent.initiated_by,
                requested_by_call_id: agent.requested_by_call_id,
                summary: agent.summary,
                error: agent.error,
                head_commit: agent.head_commit,
            })
            .collect(),
        merges: task
            .merges
            .into_iter()
            .map(|merge| BridgeTaskMergeDto {
                id: merge.id,
                agent_id: merge.agent_id,
                status: merge.status,
                merge_commit: merge.merge_commit,
                conflict_files: merge.conflict_files,
                resolution_summary: merge.resolution_summary,
            })
            .collect(),
        reviews: task
            .reviews
            .into_iter()
            .map(|review| BridgeTaskReviewDto {
                round: review.round,
                head_commit: review.head_commit,
                verdict: review.verdict,
                reviewer_agent_id: review.reviewer_agent_id,
                summary: review.summary,
                design_references: review.design_references,
            })
            .collect(),
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
