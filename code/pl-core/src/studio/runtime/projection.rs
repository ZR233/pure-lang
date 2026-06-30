use pl_protocol::{StudioAgentSnapshot, StudioRuntimeUsage, StudioSessionRuntime};

use crate::studio::records::{AgentSnapshotRecord, SessionRuntimeRecord};

pub(super) fn studio_agent_snapshot(agent: AgentSnapshotRecord) -> StudioAgentSnapshot {
    StudioAgentSnapshot {
        id: agent.id,
        session_id: agent.session_id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status,
        summary: agent.summary,
        depth: agent.depth.max(0) as u32,
        error: agent.error,
        reason: agent.reason,
        budget_limit_kind: agent.budget_limit_kind,
        budget_usage: agent.budget_usage,
        runtime_usage: agent.runtime_usage.map(studio_runtime_usage),
        updated_at: agent.updated_at,
    }
}

pub(super) fn studio_session_runtime(
    runtime: SessionRuntimeRecord,
    active_skills: Vec<String>,
    active_mcp_servers: Vec<String>,
    active_lsp_servers: Vec<String>,
) -> StudioSessionRuntime {
    StudioSessionRuntime {
        session_id: runtime.session_id,
        usage: studio_runtime_usage(pl_protocol::RuntimeUsageSnapshot {
            model: runtime.model,
            context_window: runtime.context_window,
            latest_context_tokens: runtime.latest_context_tokens,
            prompt_tokens: runtime.prompt_tokens,
            completion_tokens: runtime.completion_tokens,
            cached_prompt_tokens: runtime.cached_prompt_tokens,
            total_tokens: runtime.total_tokens,
            estimated_costs: runtime.estimated_costs,
            has_unpriced_usage: runtime.has_unpriced_usage,
            updated_at: runtime.updated_at,
        }),
        active_skills,
        active_mcp_servers,
        active_lsp_servers,
        updated_at: runtime.updated_at,
    }
}

fn studio_runtime_usage(usage: pl_protocol::RuntimeUsageSnapshot) -> StudioRuntimeUsage {
    let cache_hit_rate = if usage.prompt_tokens == 0 {
        None
    } else {
        Some(usage.cached_prompt_tokens as f64 / usage.prompt_tokens as f64)
    };
    StudioRuntimeUsage {
        model: usage.model,
        context_window: usage.context_window,
        latest_context_tokens: usage.latest_context_tokens,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cached_prompt_tokens: usage.cached_prompt_tokens,
        total_tokens: usage.total_tokens,
        cache_hit_rate,
        estimated_costs: usage.estimated_costs,
        has_unpriced_usage: usage.has_unpriced_usage,
        updated_at: usage.updated_at,
    }
}
