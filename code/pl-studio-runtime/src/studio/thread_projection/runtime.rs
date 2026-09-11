//! Usage and product state are decoded by their producers, never inferred from tool text.
use super::ProjectionError;
use pl_core::thread::{AttemptOutcome, ThreadSnapshot, journal::ThreadCommit};
use pl_protocol::{InferenceAccounting, ThreadRuntimeSnapshot, ThreadRuntimeUsage};
use std::sync::Arc;

pub(super) fn project_runtime(
    thread_id: &str,
    state: &ThreadSnapshot,
    journal: &[Arc<ThreadCommit>],
) -> Result<ThreadRuntimeSnapshot, ProjectionError> {
    let updated_at = journal
        .iter()
        .rev()
        .find(|commit| commit.sequence <= state.commit_sequence)
        .map_or(0, |commit| commit.committed_at);
    let mut usage = ThreadRuntimeUsage {
        has_incomplete_usage: false,
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
        updated_at,
    };
    let mut turn_completion_tokens: u64 = 0;
    let mut turn_decode_millis: u64 = 0;
    for attempt in state.attempts.iter() {
        if let Some(request) = attempt
            .request_metadata
            .as_ref()
            .map(pl_model::runtime::model_request_receipt)
            .transpose()?
        {
            usage.model = request.binding.requested_model;
        }
        let output = match &attempt.outcome {
            AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => {
                Some(Ok(output))
            }
            AttemptOutcome::Failed(error) => Some(Err(error.as_ref())),
            AttemptOutcome::Cancelled { result } => Some(result.as_ref().map_err(Arc::as_ref)),
            AttemptOutcome::Running => None,
            AttemptOutcome::Interrupted => {
                usage.has_incomplete_usage = true;
                usage.has_unpriced_usage = true;
                None
            }
        };
        let Some(output) = output else {
            continue;
        };
        let accounting = match output {
            Ok(output) => match pl_model::runtime::model_response_receipt(output)? {
                Some(receipt) => {
                    if state
                        .turns
                        .last()
                        .is_some_and(|turn| turn.turn_id == attempt.turn_id)
                        && let Some(timing) = receipt.response.timing
                    {
                        turn_completion_tokens = turn_completion_tokens
                            .checked_add(
                                receipt.response.accounting.usage.output_tokens.unwrap_or(0),
                            )
                            .ok_or(ProjectionError::Count)?;
                        turn_decode_millis = turn_decode_millis
                            .checked_add(timing.decode_millis)
                            .ok_or(ProjectionError::Count)?;
                    }
                    receipt.response.accounting
                }
                None => unknown_accounting(&output.usage),
            },
            Err(error) => pl_model::runtime::model_failure_receipt(error)?.map_or_else(
                || unknown_accounting(&error.usage),
                |receipt| receipt.accounting,
            ),
        };
        add_usage(&mut usage, &accounting)?;
    }
    let latest_context_tokens = usage.latest_context_tokens;
    for record in state.extensions.values() {
        if let Some(receipt) = super::compactions::receipt(&record.payload)? {
            add_usage(&mut usage, &receipt.accounting)?;
        }
    }
    usage.latest_context_tokens = latest_context_tokens;
    if !usage.has_incomplete_usage && usage.prompt_tokens > 0 {
        usage.cache_hit_rate = Some(usage.cached_prompt_tokens as f64 / usage.prompt_tokens as f64);
    }
    let todo = state
        .extensions
        .get("pl.tool.todo")
        .map(|record| {
            let call_id = state
                .deliveries
                .iter()
                .rev()
                .find(|delivery| delivery.output.payload() == &record.payload)
                .map_or_else(String::new, |delivery| delivery.call_id.clone());
            pl_tool::todo::saved_snapshot(&record.payload, call_id)
        })
        .transpose()?;
    let workflow = state
        .extensions
        .get(crate::workflow_tool::WORKFLOW_EXTENSION)
        .map(|record| {
            crate::workflow_tool::decode_workflow_state(&record.payload)
                .map(|state| pl_protocol::WorkflowRuntimeSnapshot::from(&state))
        })
        .transpose()?;
    let active_skills = state
        .extensions
        .values()
        .filter(|record| record.payload.format() == "pl.tool.skill-view")
        .map(|record| pl_tool::skill::saved_skill_name(&record.payload))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?
        .into_iter()
        .collect();
    Ok(ThreadRuntimeSnapshot {
        thread_id: thread_id.into(),
        usage,
        turn_completion_tokens,
        turn_decode_millis,
        todo,
        workflow,
        active_skills,
        active_mcp_servers: Vec::new(),
        active_lsp_servers: Vec::new(),
        progress: None,
        mcp_health: None,
        updated_at,
    })
}

fn unknown_accounting(usage: &pl_core::model::ModelUsage) -> InferenceAccounting {
    InferenceAccounting {
        usage: pl_protocol::UsageReport {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            total_tokens: usage
                .input_tokens
                .zip(usage.output_tokens)
                .and_then(|(a, b)| a.checked_add(b)),
        },
        ..Default::default()
    }
}

fn add_usage(
    target: &mut ThreadRuntimeUsage,
    accounting: &InferenceAccounting,
) -> Result<(), ProjectionError> {
    let totals = accounting.usage.totals();
    target.has_incomplete_usage |= accounting.has_incomplete_usage();
    target.has_unpriced_usage |= accounting.has_unpriced_usage();
    target.inference_count = target
        .inference_count
        .checked_add(1)
        .ok_or(ProjectionError::Count)?;
    for (current, value) in [
        (&mut target.prompt_tokens, totals.prompt_tokens),
        (&mut target.completion_tokens, totals.completion_tokens),
        (
            &mut target.cached_prompt_tokens,
            totals.cached_prompt_tokens,
        ),
        (&mut target.cache_write_tokens, totals.cache_write_tokens),
        (&mut target.reasoning_tokens, totals.reasoning_tokens),
        (&mut target.total_tokens, totals.total_tokens),
    ] {
        *current = current.checked_add(value).ok_or(ProjectionError::Count)?;
    }
    target.cache_miss_tokens = target
        .prompt_tokens
        .saturating_sub(target.cached_prompt_tokens);
    if let Some(tokens) = accounting.usage.known_total_tokens() {
        target.latest_context_tokens = tokens;
    }
    merge_costs(&mut target.estimated_costs, &accounting.estimated_costs());
    merge_costs(
        &mut target.estimated_cache_savings,
        &accounting.estimated_cache_savings(),
    );
    Ok(())
}

fn merge_costs(
    target: &mut Vec<pl_protocol::RuntimeCostAmount>,
    incoming: &[pl_protocol::RuntimeCostAmount],
) {
    for cost in incoming {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| existing.currency == cost.currency)
        {
            existing.amount += cost.amount;
        } else {
            target.push(cost.clone());
        }
    }
    target.sort_by(|left, right| left.currency.cmp(&right.currency));
}
