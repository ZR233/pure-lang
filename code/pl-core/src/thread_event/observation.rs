use std::collections::BTreeMap;

use pl_protocol::{
    InteractionRequest, RuntimeCostAmount, ThreadContextCompactionItem, ThreadItem,
    ThreadItemState, ThreadNotification, ThreadNotificationEnvelope, ThreadRuntimeSnapshot,
    ThreadRuntimeUsage, ThreadSnapshot,
};
use pl_trace::AgentEvent;

use crate::ContextCompactionSnapshot;

use super::fact::{ThreadNotificationFact, project_thread_facts};
use super::projector::ThreadProjectionBatch;

#[derive(Debug, Clone)]
pub(crate) struct ObservedTurnEvent {
    pub(crate) turn_id: String,
    pub(crate) thread_id: String,
    pub(crate) observation: TurnObservation,
}

#[derive(Debug, Clone)]
pub(crate) enum TurnObservation {
    RuntimeDelta(Box<pl_protocol::AgentRuntimeDelta>),
    TodoList(pl_protocol::TodoListSnapshot),
    InteractionChanged(Box<InteractionRequest>),
    ContextCompacted {
        before_tokens: u64,
        after_tokens: u64,
        compacted_at: i64,
    },
    Diagnostic,
}

pub(crate) fn observation_from_agent_event(event: &AgentEvent) -> Option<TurnObservation> {
    match event {
        AgentEvent::AgentRuntimeUpdated { delta } => {
            Some(TurnObservation::RuntimeDelta(Box::new(delta.clone())))
        }
        AgentEvent::TodoListUpdated { snapshot } => {
            Some(TurnObservation::TodoList(snapshot.clone()))
        }
        AgentEvent::InteractionChanged { event } => Some(TurnObservation::InteractionChanged(
            Box::new(event.interaction.clone()),
        )),
        AgentEvent::TurnInterrupted { .. } | AgentEvent::Error { .. } => {
            Some(TurnObservation::Diagnostic)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done => None,
    }
}

pub(crate) fn compaction_observation(
    snapshot: &ContextCompactionSnapshot,
    compacted_at: i64,
) -> TurnObservation {
    TurnObservation::ContextCompacted {
        before_tokens: snapshot.tokens_before,
        after_tokens: snapshot
            .replacement_tokens
            .unwrap_or(snapshot.estimated_request_tokens),
        compacted_at,
    }
}

pub(crate) fn project_observation(
    thread_id: &str,
    turn_id: &str,
    revision: u64,
    current: &ThreadSnapshot,
    observation: TurnObservation,
) -> ThreadProjectionBatch {
    if let TurnObservation::InteractionChanged(interaction) = observation {
        return project_thread_facts(
            thread_id,
            current,
            vec![ThreadNotificationFact::durable(
                interaction.updated_at,
                ThreadNotification::InteractionChanged { interaction },
            )],
        );
    }
    let (emitted_at, notification) = match observation {
        TurnObservation::RuntimeDelta(delta) => {
            let emitted_at = delta.updated_at;
            (
                emitted_at,
                Some(ThreadNotification::ThreadRuntimeUpdated {
                    runtime: Box::new(runtime_snapshot(thread_id, current, *delta)),
                }),
            )
        }
        TurnObservation::TodoList(todo) => {
            let emitted_at = unix_timestamp();
            let mut runtime = current
                .runtime
                .clone()
                .unwrap_or_else(|| empty_runtime(thread_id));
            runtime.todo = Some(todo);
            runtime.updated_at = emitted_at;
            (
                emitted_at,
                Some(ThreadNotification::ThreadRuntimeUpdated {
                    runtime: Box::new(runtime),
                }),
            )
        }
        TurnObservation::ContextCompacted {
            before_tokens,
            after_tokens,
            compacted_at,
        } => (
            compacted_at,
            Some(ThreadNotification::ItemCompleted {
                item: Box::new(ThreadItem::new(
                    format!("{turn_id}:context-compaction:{compacted_at}"),
                    thread_id.to_string(),
                    turn_id.to_string(),
                    // ordinal 由 ThreadEventBus 首次应用时分配（到达序）。
                    0,
                    0,
                    compacted_at,
                    compacted_at,
                    ThreadItemState::ContextCompaction(ThreadContextCompactionItem::new(
                        before_tokens,
                        after_tokens,
                        compacted_at,
                    )),
                )),
            }),
        ),
        TurnObservation::Diagnostic => (unix_timestamp(), None),
        TurnObservation::InteractionChanged(_) => unreachable!("handled before projection"),
    };
    let notifications = notification
        .map(|notification| {
            vec![ThreadNotificationEnvelope {
                thread_id: thread_id.to_string(),
                revision: revision.saturating_add(1),
                emitted_at,
                notification,
            }]
        })
        .unwrap_or_default();
    ThreadProjectionBatch {
        through_revision: revision.saturating_add(u64::from(!notifications.is_empty())),
        notifications,
    }
}

fn runtime_snapshot(
    thread_id: &str,
    current: &ThreadSnapshot,
    delta: pl_protocol::AgentRuntimeDelta,
) -> ThreadRuntimeSnapshot {
    let prior = current.runtime.as_ref().map(|runtime| &runtime.usage);
    let prompt_tokens = prior
        .map_or(0, |usage| usage.prompt_tokens)
        .saturating_add(delta.usage.prompt_tokens);
    let cached_prompt_tokens = prior
        .map_or(0, |usage| usage.cached_prompt_tokens)
        .saturating_add(delta.usage.cached_prompt_tokens)
        .min(prompt_tokens);
    let completion_tokens = prior
        .map_or(0, |usage| usage.completion_tokens)
        .saturating_add(delta.usage.completion_tokens);
    let cache_write_tokens = prior
        .map_or(0, |usage| usage.cache_write_tokens)
        .saturating_add(delta.usage.cache_write_tokens)
        .min(prompt_tokens.saturating_sub(cached_prompt_tokens));
    let cache_miss_tokens = prompt_tokens.saturating_sub(cached_prompt_tokens);
    let reasoning_tokens = prior
        .map_or(0, |usage| usage.reasoning_tokens)
        .saturating_add(delta.usage.reasoning_tokens);
    let inference_count = prior
        .map_or(0, |usage| usage.inference_count)
        .saturating_add(delta.usage.inference_count);
    let total_tokens = prior
        .map_or(0, |usage| usage.total_tokens)
        .saturating_add(delta.usage.total_tokens);
    let previous = current.runtime.as_ref();
    let prior_turn_performance = previous.map_or((0, 0), |runtime| {
        (runtime.turn_completion_tokens, runtime.turn_decode_millis)
    });
    let (turn_completion_tokens, turn_decode_millis) = match delta.timing {
        Some(timing) if timing.has_throughput_sample() => (
            prior_turn_performance
                .0
                .saturating_add(delta.usage.completion_tokens),
            prior_turn_performance
                .1
                .saturating_add(timing.decode_millis),
        ),
        Some(_) | None => prior_turn_performance,
    };
    ThreadRuntimeSnapshot {
        thread_id: thread_id.to_string(),
        usage: ThreadRuntimeUsage {
            model: delta.model,
            context_window: delta.context_window,
            latest_context_tokens: delta.usage.total_tokens,
            prompt_tokens,
            completion_tokens,
            cached_prompt_tokens,
            cache_write_tokens,
            cache_miss_tokens,
            reasoning_tokens,
            inference_count,
            total_tokens,
            cache_hit_rate: (prompt_tokens > 0)
                .then_some((cached_prompt_tokens as f64 / prompt_tokens as f64).clamp(0.0, 1.0)),
            estimated_costs: merge_costs(
                prior.map_or(&[], |usage| usage.estimated_costs.as_slice()),
                &delta.estimated_costs,
            ),
            estimated_cache_savings: merge_costs(
                prior.map_or(&[], |usage| usage.estimated_cache_savings.as_slice()),
                &delta.estimated_cache_savings,
            ),
            has_unpriced_usage: prior.is_some_and(|usage| usage.has_unpriced_usage)
                || delta.has_unpriced_usage,
            prompt_generation: delta
                .prompt_generation
                .or_else(|| prior.and_then(|usage| usage.prompt_generation)),
            prompt_cache_policy: delta
                .prompt_cache_policy
                .or_else(|| prior.and_then(|usage| usage.prompt_cache_policy.clone())),
            prefix_changed_reason: delta
                .prefix_changed_reason
                .or_else(|| prior.and_then(|usage| usage.prefix_changed_reason)),
            updated_at: delta.updated_at,
        },
        turn_completion_tokens,
        turn_decode_millis,
        todo: previous.and_then(|runtime| runtime.todo.clone()),
        active_skills: previous.map_or_else(Vec::new, |runtime| runtime.active_skills.clone()),
        active_mcp_servers: previous
            .map_or_else(Vec::new, |runtime| runtime.active_mcp_servers.clone()),
        active_lsp_servers: previous
            .map_or_else(Vec::new, |runtime| runtime.active_lsp_servers.clone()),
        progress: previous.and_then(|runtime| runtime.progress.clone()),
        mcp_health: previous.and_then(|runtime| runtime.mcp_health.clone()),
        updated_at: delta.updated_at,
    }
}

pub(super) fn empty_runtime(thread_id: &str) -> ThreadRuntimeSnapshot {
    ThreadRuntimeSnapshot {
        thread_id: thread_id.to_string(),
        usage: ThreadRuntimeUsage {
            model: String::new(),
            context_window: None,
            latest_context_tokens: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_prompt_tokens: 0,
            cache_write_tokens: 0,
            cache_miss_tokens: 0,
            reasoning_tokens: 0,
            inference_count: 0,
            total_tokens: 0,
            cache_hit_rate: None,
            estimated_costs: Vec::new(),
            estimated_cache_savings: Vec::new(),
            has_unpriced_usage: false,
            prompt_generation: None,
            prompt_cache_policy: None,
            prefix_changed_reason: None,
            updated_at: 0,
        },
        turn_completion_tokens: 0,
        turn_decode_millis: 0,
        todo: None,
        active_skills: Vec::new(),
        active_mcp_servers: Vec::new(),
        active_lsp_servers: Vec::new(),
        progress: None,
        mcp_health: None,
        updated_at: 0,
    }
}

fn merge_costs(left: &[RuntimeCostAmount], right: &[RuntimeCostAmount]) -> Vec<RuntimeCostAmount> {
    let mut totals = BTreeMap::<String, f64>::new();
    for cost in left.iter().chain(right) {
        *totals.entry(cost.currency.clone()).or_default() += cost.amount;
    }
    totals
        .into_iter()
        .map(|(currency, amount)| RuntimeCostAmount { currency, amount })
        .collect()
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use pl_protocol::{AgentRuntimeDelta, InferenceTiming, SkillActivation, TokenUsageSnapshot};

    use super::*;

    #[test]
    fn skill_agent_event_is_not_a_second_persistent_projection_source() {
        let event = AgentEvent::SkillActivated {
            activation: activation(),
        };

        assert!(observation_from_agent_event(&event).is_none());
    }

    #[test]
    fn later_runtime_delta_preserves_active_skills() {
        let mut current = ThreadSnapshot::empty("thread-1");
        let mut runtime = empty_runtime("thread-1");
        runtime.active_skills = vec!["doc".to_string(), "pdf".to_string()];
        current.runtime = Some(runtime);

        let batch = project_observation(
            "thread-1",
            "turn-1",
            0,
            &current,
            TurnObservation::RuntimeDelta(Box::new(AgentRuntimeDelta {
                inference_id: "inference-1".to_string(),
                agent_id: "thread-1".to_string(),
                path: "thread-1".to_string(),
                parent_path: None,
                role: "planner".to_string(),
                model: "model".to_string(),
                context_window: Some(100),
                usage: TokenUsageSnapshot::default(),
                estimated_costs: Vec::new(),
                estimated_cache_savings: Vec::new(),
                has_unpriced_usage: false,
                prompt_generation: None,
                prompt_cache_policy: None,
                prefix_changed_reason: None,
                timing: None,
                updated_at: 8,
            })),
        );

        assert!(matches!(
            &batch.notifications[0].notification,
            ThreadNotification::ThreadRuntimeUpdated { runtime }
                if runtime.active_skills == ["doc", "pdf"]
        ));
    }

    #[test]
    fn runtime_snapshot_weights_turn_throughput_by_tokens_and_decode_time() {
        let current = ThreadSnapshot::empty("thread-1");
        let first = runtime_snapshot(
            "thread-1",
            &current,
            runtime_delta("inference-1", 20, 15, Some(100)),
        );
        let mut current = ThreadSnapshot::empty("thread-1");
        current.runtime = Some(first);

        let second = runtime_snapshot(
            "thread-1",
            &current,
            runtime_delta("inference-2", 30, 25, Some(400)),
        );
        assert_eq!(second.turn_completion_tokens, 50);
        assert_eq!(second.turn_decode_millis, 500);
        assert_eq!(second.usage.reasoning_tokens, 40);

        let mut current = ThreadSnapshot::empty("thread-1");
        current.runtime = Some(second);
        let without_timing = runtime_snapshot(
            "thread-1",
            &current,
            runtime_delta("inference-3", 100, 80, None),
        );
        assert_eq!(without_timing.turn_completion_tokens, 50);
        assert_eq!(without_timing.turn_decode_millis, 500);
    }

    fn runtime_delta(
        inference_id: &str,
        completion_tokens: u64,
        reasoning_tokens: u64,
        decode_millis: Option<u64>,
    ) -> AgentRuntimeDelta {
        AgentRuntimeDelta {
            inference_id: inference_id.to_string(),
            agent_id: "thread-1".to_string(),
            path: "thread-1".to_string(),
            parent_path: None,
            role: "planner".to_string(),
            model: "model".to_string(),
            context_window: Some(100),
            usage: TokenUsageSnapshot {
                completion_tokens,
                reasoning_tokens,
                inference_count: 1,
                total_tokens: completion_tokens,
                ..TokenUsageSnapshot::default()
            },
            estimated_costs: Vec::new(),
            estimated_cache_savings: Vec::new(),
            has_unpriced_usage: false,
            prompt_generation: None,
            prompt_cache_policy: None,
            prefix_changed_reason: None,
            timing: decode_millis.map(|decode_millis| InferenceTiming {
                ttft_millis: 10,
                decode_millis,
                total_millis: decode_millis + 10,
            }),
            updated_at: 8,
        }
    }

    fn activation() -> SkillActivation {
        SkillActivation {
            name: "pdf".to_string(),
            source: "system".to_string(),
            provider_id: "local-filesystem".to_string(),
            resource_base: pl_protocol::SkillActivationResourceBase::Directory {
                path: "/skills/pdf".to_string(),
            },
            turn_id: "turn-1".to_string(),
            cause: pl_protocol::SkillActivationCause::Tool {
                tool_call_id: "tool-1".to_string(),
            },
            activated_at: 7,
        }
    }
}
