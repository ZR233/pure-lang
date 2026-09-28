//! Portable cumulative accounting for the plain SQLite Thread store.
//!
//! The model adapter freezes the selected model and context capacity before dispatch. Terminal
//! attempts carry service-reported token counters. This fold uses only those core facts, so a
//! product can read the same absolute totals from a hot snapshot and a restored checkpoint.

use super::{AttemptOutcome, ThreadEffectBatch, UsageSummary};
use crate::model::ModelUsage;

pub(crate) fn fold_effect(summary: &mut UsageSummary, effect: &ThreadEffectBatch) {
    if effect.sequence <= summary.applied_sequence {
        return;
    }
    if let Some(turn) = &effect.turn
        && summary.turn_id != turn.turn_id
    {
        summary.turn_id.clone_from(&turn.turn_id);
        summary.turn_completion_tokens = 0;
        summary.turn_decode_millis = 0;
    }
    if let Some(attempt) = &effect.attempt {
        summary.model = attempt
            .usage_binding
            .as_ref()
            .map_or_else(String::new, |binding| binding.model.clone());
        summary.context_window = attempt
            .usage_binding
            .as_ref()
            .and_then(|binding| binding.context_window);
        match &attempt.outcome {
            AttemptOutcome::Running => {}
            AttemptOutcome::Interrupted => {
                summary.has_incomplete_usage = true;
                summary.has_unpriced_usage = true;
                summary.cache_incomplete = true;
            }
            AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => {
                add_usage(summary, &output.usage, &attempt.turn_id);
            }
            AttemptOutcome::Failed(error) => {
                add_usage(summary, &error.usage, &attempt.turn_id);
            }
            AttemptOutcome::Cancelled { result } => match result {
                Ok(output) => add_usage(summary, &output.usage, &attempt.turn_id),
                Err(error) => add_usage(summary, &error.usage, &attempt.turn_id),
            },
        }
    }
    summary.applied_sequence = effect.sequence;
}

fn add_usage(summary: &mut UsageSummary, usage: &ModelUsage, turn_id: &str) {
    let input = usage.input_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    let derived_total = usage
        .input_tokens
        .zip(usage.output_tokens)
        .and_then(|(input, output)| input.checked_add(output));
    let total = usage.total_tokens.or(derived_total);
    let invalid_cache = usage.input_tokens.is_some_and(|input| {
        usage
            .cache_read_tokens
            .unwrap_or(0)
            .checked_add(usage.cache_write_tokens.unwrap_or(0))
            .is_none_or(|cached| cached > input)
    });
    let invalid_reasoning = usage
        .output_tokens
        .zip(usage.reasoning_tokens)
        .is_some_and(|(output, reasoning)| reasoning > output);
    let reported_sides = usage.input_tokens.is_some() && usage.output_tokens.is_some();
    let invalid_total = reported_sides
        && (derived_total.is_none()
            || usage
                .total_tokens
                .is_some_and(|reported| Some(reported) != derived_total));
    let valid = !invalid_total && !invalid_cache && !invalid_reasoning;
    let complete = reported_sides && valid;
    summary.has_incomplete_usage |= !complete || usage.cache_read_tokens.is_none();
    add_counter(
        &mut summary.inference_count,
        1,
        &mut summary.has_incomplete_usage,
    );
    add_counter(
        &mut summary.prompt_tokens,
        input,
        &mut summary.has_incomplete_usage,
    );
    add_counter(
        &mut summary.completion_tokens,
        output,
        &mut summary.has_incomplete_usage,
    );
    add_counter(
        &mut summary.cached_prompt_tokens,
        usage.cache_read_tokens.unwrap_or(0),
        &mut summary.has_incomplete_usage,
    );
    add_counter(
        &mut summary.cache_write_tokens,
        usage.cache_write_tokens.unwrap_or(0),
        &mut summary.has_incomplete_usage,
    );
    add_counter(
        &mut summary.reasoning_tokens,
        usage.reasoning_tokens.unwrap_or(0),
        &mut summary.has_incomplete_usage,
    );
    if let Some(total) = total {
        add_counter(
            &mut summary.total_tokens,
            total,
            &mut summary.has_incomplete_usage,
        );
        if valid {
            summary.latest_context_tokens = total;
        }
    }
    if summary.turn_id == turn_id {
        add_counter(
            &mut summary.turn_completion_tokens,
            output,
            &mut summary.has_incomplete_usage,
        );
    }
    // pl-core has no provider price table; products may enrich pricing in their own writer.
    summary.has_unpriced_usage = true;
    if let (Some(input), Some(read)) = (usage.input_tokens, usage.cache_read_tokens)
        && read <= input
        && usage
            .cache_write_tokens
            .is_none_or(|write| write <= input - read)
    {
        add_counter(
            &mut summary.cache_input_tokens,
            input,
            &mut summary.cache_incomplete,
        );
        add_counter(
            &mut summary.cache_read_tokens,
            read,
            &mut summary.cache_incomplete,
        );
    } else {
        summary.cache_incomplete = true;
    }
    summary.has_incomplete_usage |= summary.cache_incomplete;
}

fn add_counter(target: &mut u64, increment: u64, incomplete: &mut bool) {
    match target.checked_add(increment) {
        Some(value) => *target = value,
        None => *incomplete = true,
    }
}
