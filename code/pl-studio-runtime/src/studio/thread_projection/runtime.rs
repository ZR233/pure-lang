//! Usage and product state are decoded by their producers, never inferred from tool text.
use super::ProjectionError;
use pl_core::thread::{AttemptOutcome, ThreadSnapshot};
use pl_protocol::{
    CacheUsageSummary, InferenceAccounting, ThreadRuntimeSnapshot, ThreadRuntimeUsage, UsageReport,
};
use std::sync::Arc;

pub(in crate::studio) fn project_runtime(
    thread_id: &str,
    state: &ThreadSnapshot,
    updated_at: i64,
    summary: &pl_core::thread::UsageSummary,
) -> Result<ThreadRuntimeSnapshot, ProjectionError> {
    // Totals come from the Thread's cumulative summary, never from whatever attempts are still
    // resident: finished attempts are dropped by the commit that produced them, so aggregating
    // them here is what made a hot increment, a reconnect and a cold restore disagree.
    let usage = ThreadRuntimeUsage {
        has_incomplete_usage: summary.has_incomplete_usage,
        model: summary.model.clone(),
        context_window: summary.context_window,
        latest_context_tokens: summary.latest_context_tokens,
        prompt_tokens: summary.prompt_tokens,
        completion_tokens: summary.completion_tokens,
        cached_prompt_tokens: summary.cached_prompt_tokens,
        cache_write_tokens: summary.cache_write_tokens,
        reasoning_tokens: summary.reasoning_tokens,
        inference_count: summary.inference_count,
        total_tokens: summary.total_tokens,
        cache_usage: CacheUsageSummary {
            input_tokens: summary.cache_input_tokens,
            cache_read_tokens: summary.cache_read_tokens,
            hit_rate: (summary.cache_input_tokens > 0)
                .then(|| summary.cache_read_tokens as f64 / summary.cache_input_tokens as f64),
            has_incomplete_usage: summary.cache_incomplete,
        },
        estimated_costs: protocol_costs(&summary.estimated_costs),
        estimated_cache_savings: protocol_costs(&summary.estimated_cache_savings),
        has_unpriced_usage: summary.has_unpriced_usage,
        prompt_generation: None,
        prompt_cache_policy: None,
        prefix_changed_reason: None,
        updated_at,
    };
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
    let child = state
        .extensions
        .contains_key(crate::studio::model_route::AGENT_PROFILE_EXTENSION);
    let model_route = crate::studio::model_route::route_record(state, child)
        .map_err(|error| ProjectionError::UnsupportedOutput(error.to_string()))?
        .map(|(record, route)| pl_protocol::ThreadModelRouteSnapshot {
            provider_id: route.provider.into_string(),
            model: route.model,
            effort: route.effort.map(|effort| effort.as_str().to_owned()),
            revision: record.revision,
            available: state.model_available,
            unavailable_reason: (!state.model_available)
                .then(|| "the selected model binding is unavailable".to_string()),
        });
    Ok(ThreadRuntimeSnapshot {
        thread_id: thread_id.into(),
        model_route,
        usage,
        turn_completion_tokens: summary.turn_completion_tokens,
        turn_decode_millis: summary.turn_decode_millis,
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

/// Folds one committed effect into the Thread's cumulative accounting exactly once.
///
/// Every field is written as an absolute total and `applied_sequence` makes a repeated fold of the
/// same effect a no-op, so the live subscription and the durable writer may both fold the same
/// effect without ever double counting. Finished attempts leave the resident state in the commit
/// that produced them, so this summary — not `state.attempts` — is what survives them.
pub(in crate::studio) fn fold_effect_accounting(
    summary: &mut pl_core::thread::UsageSummary,
    effect: &pl_core::thread::ThreadEffectBatch,
) -> Result<(), ProjectionError> {
    if effect.sequence <= summary.applied_sequence {
        return Ok(());
    }
    // Fold into a copy so the sequence and the values it accounted for commit together: a failed
    // decode or a counter overflow leaves the caller's summary untouched instead of advancing
    // `applied_sequence` while only some fields were updated, which a retry would then skip.
    let mut next = summary.clone();
    fold_effect_accounting_in_place(&mut next, effect)?;
    next.applied_sequence = effect.sequence;
    *summary = next;
    Ok(())
}

/// Applies one effect to a summary that has not yet recorded its sequence.
fn fold_effect_accounting_in_place(
    summary: &mut pl_core::thread::UsageSummary,
    effect: &pl_core::thread::ThreadEffectBatch,
) -> Result<(), ProjectionError> {
    // 新的 Turn 开始时每轮计数重新累计；旧 Turn 的尝试不再改变它。
    if let Some(turn) = &effect.turn
        && summary.turn_id != turn.turn_id
    {
        summary.turn_id = turn.turn_id.clone();
        summary.turn_completion_tokens = 0;
        summary.turn_decode_millis = 0;
    }
    if let Some(update) = &effect.attempt {
        // 模型身份与容量来自 admission 时的绑定，因此在响应存在之前就已投影。
        match update.request_metadata.as_ref() {
            Some(metadata) => {
                let request = pl_model::runtime::model_request_receipt(metadata)?;
                summary.model = request.binding.requested_model;
                summary.context_window = request.binding.context_window;
            }
            None => summary.context_window = None,
        }
        let output = match &update.outcome {
            AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => {
                Some(Ok(output))
            }
            AttemptOutcome::Failed(error) => Some(Err(error.as_ref())),
            AttemptOutcome::Cancelled { result } => Some(result.as_ref().map_err(Arc::as_ref)),
            AttemptOutcome::Running => None,
            AttemptOutcome::Interrupted => {
                summary.has_incomplete_usage = true;
                summary.has_unpriced_usage = true;
                summary.cache_incomplete = true;
                None
            }
        };
        if let Some(output) = output {
            let accounting = match output {
                Ok(output) => match pl_model::runtime::model_response_receipt(output)? {
                    Some(receipt) => {
                        if summary.turn_id == update.turn_id
                            && let Some(timing) = receipt.response.timing
                        {
                            summary.turn_completion_tokens = summary
                                .turn_completion_tokens
                                .checked_add(
                                    receipt.response.accounting.usage.output_tokens.unwrap_or(0),
                                )
                                .ok_or(ProjectionError::Count)?;
                            summary.turn_decode_millis = summary
                                .turn_decode_millis
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
            add_accounting(summary, &accounting)?;
        }
    }
    for change in effect.extensions.iter() {
        let pl_core::thread::extensions::ExtensionChange::Put { record, .. } = change else {
            continue;
        };
        if record.payload.format() != "pl.studio.compaction" {
            continue;
        }
        // 辅助压缩计费按原路径各纳入一次，但不代表新的上下文长度。
        if let Some(receipt) = super::compactions::receipt(&record.payload)? {
            let latest = summary.latest_context_tokens;
            add_accounting(summary, &receipt.accounting)?;
            summary.latest_context_tokens = latest;
        }
    }
    Ok(())
}

fn protocol_costs(costs: &[pl_core::thread::UsageCost]) -> Vec<pl_protocol::RuntimeCostAmount> {
    costs
        .iter()
        .map(|cost| pl_protocol::RuntimeCostAmount {
            currency: cost.currency.clone(),
            amount: cost.amount,
        })
        .collect()
}

fn add_accounting(
    target: &mut pl_core::thread::UsageSummary,
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
    if let Some((input, read)) = valid_cache_sample(&accounting.usage) {
        target.cache_input_tokens = target
            .cache_input_tokens
            .checked_add(input)
            .ok_or(ProjectionError::Count)?;
        target.cache_read_tokens = target
            .cache_read_tokens
            .checked_add(read)
            .ok_or(ProjectionError::Count)?;
    } else {
        target.cache_incomplete = true;
    }
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

/// 提取一次 inference 的输入侧缓存样本。
///
/// 只有 input 与 cache read 同时报告、且 cache read（连同可选的 cache
/// write）不超过 input 时才有效。输出侧字段缺失或矛盾不影响缓存样本有效
/// 性，因为命中率只由输入侧累计得出。
fn valid_cache_sample(usage: &UsageReport) -> Option<(u64, u64)> {
    let input = usage.input_tokens?;
    let read = usage.cache_read_tokens?;
    if read > input {
        return None;
    }
    if let Some(write) = usage.cache_write_tokens {
        let billed = read.checked_add(write)?;
        if billed > input {
            return None;
        }
    }
    Some((input, read))
}

fn merge_costs(
    target: &mut Vec<pl_core::thread::UsageCost>,
    incoming: &[pl_protocol::RuntimeCostAmount],
) {
    for cost in incoming {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| existing.currency == cost.currency)
        {
            existing.amount += cost.amount;
        } else {
            target.push(pl_core::thread::UsageCost {
                currency: cost.currency.clone(),
                amount: cost.amount,
            });
        }
    }
    target.sort_by(|left, right| left.currency.cmp(&right.currency));
}
