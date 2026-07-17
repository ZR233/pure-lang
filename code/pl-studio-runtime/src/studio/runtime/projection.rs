use crate::{
    StudioRuntimeUsage, StudioSessionRuntime, StudioTaskAgentRuntime, StudioTaskMergeRuntime,
    StudioTaskReviewRuntime, StudioTaskRuntime, StudioTaskWorkUnitRuntime,
};

use crate::studio::records::SessionRuntimeRecord;
use crate::studio::task_coordinator::{
    AgentOutcomeRecord, MergeRecord, ReviewRoundRecord, TaskRunRecord, WorkUnitRecord,
};

pub(super) fn studio_session_runtime(
    runtime: SessionRuntimeRecord,
    active_skills: Vec<String>,
    active_mcp_servers: Vec<String>,
    active_lsp_servers: Vec<String>,
    task: Option<StudioTaskRuntime>,
) -> StudioSessionRuntime {
    StudioSessionRuntime {
        session_id: runtime.session_id,
        usage: studio_runtime_usage(crate::RuntimeUsageSnapshot {
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
        task,
        updated_at: runtime.updated_at,
    }
}

pub(super) fn studio_task_runtime(
    run: TaskRunRecord,
    work_units: Vec<WorkUnitRecord>,
    agents: Vec<AgentOutcomeRecord>,
    merges: Vec<MergeRecord>,
    reviews: Vec<ReviewRoundRecord>,
) -> StudioTaskRuntime {
    StudioTaskRuntime {
        run_id: run.id,
        phase: run.phase.as_str().to_string(),
        branch: run.branch,
        expected_head: run.expected_head,
        status_message: run.status_message,
        work_units: work_units
            .into_iter()
            .map(|unit| StudioTaskWorkUnitRuntime {
                id: unit.id,
                title: unit.title,
                status: unit.status.as_str().to_string(),
                worktree_path: unit.worktree_path,
                branch: unit.branch,
                agent_id: unit.agent_id,
            })
            .collect(),
        agents: agents
            .into_iter()
            .map(|agent| StudioTaskAgentRuntime {
                agent_id: agent.agent_id,
                role: agent.role,
                status: agent.status.as_str().to_string(),
                initiated_by: agent.initiated_by,
                requested_by_call_id: agent.requested_by_call_id,
                summary: agent.summary,
                error: agent.error,
                head_commit: agent.delivery.map(|delivery| delivery.head_commit),
            })
            .collect(),
        merges: merges
            .into_iter()
            .map(|merge| StudioTaskMergeRuntime {
                id: merge.id,
                agent_id: merge.agent_id,
                status: merge.status.as_str().to_string(),
                merge_commit: merge
                    .evidence
                    .as_ref()
                    .and_then(|evidence| evidence.merge_commit.clone()),
                conflict_files: merge.conflict_files,
                resolution_summary: merge.resolution_summary,
            })
            .collect(),
        reviews: reviews
            .into_iter()
            .map(|review| StudioTaskReviewRuntime {
                round: review.round,
                head_commit: review.head_commit,
                verdict: review.verdict.as_str().to_string(),
                reviewer_agent_id: review.reviewer_agent_id,
                summary: review.summary,
                design_references: review
                    .design_references
                    .into_iter()
                    .map(|reference| format!("{}#{}", reference.path, reference.section))
                    .collect(),
            })
            .collect(),
    }
}

fn studio_runtime_usage(usage: crate::RuntimeUsageSnapshot) -> StudioRuntimeUsage {
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
