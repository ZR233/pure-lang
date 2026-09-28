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
        if let Some(binding) = &attempt.usage_binding {
            summary.model.clone_from(&binding.model);
            summary.context_window = binding.context_window;
        }
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
    let total = input.saturating_add(output);
    summary.inference_count = summary.inference_count.saturating_add(1);
    summary.prompt_tokens = summary.prompt_tokens.saturating_add(input);
    summary.completion_tokens = summary.completion_tokens.saturating_add(output);
    summary.cached_prompt_tokens = summary
        .cached_prompt_tokens
        .saturating_add(usage.cache_read_tokens.unwrap_or(0));
    summary.cache_write_tokens = summary
        .cache_write_tokens
        .saturating_add(usage.cache_write_tokens.unwrap_or(0));
    summary.reasoning_tokens = summary
        .reasoning_tokens
        .saturating_add(usage.reasoning_tokens.unwrap_or(0));
    summary.total_tokens = summary.total_tokens.saturating_add(total);
    if usage.input_tokens.is_some() || usage.output_tokens.is_some() {
        summary.latest_context_tokens = total;
    }
    if summary.turn_id == turn_id {
        summary.turn_completion_tokens = summary.turn_completion_tokens.saturating_add(output);
    }
    summary.has_incomplete_usage |= usage.input_tokens.is_none() || usage.output_tokens.is_none();
    // pl-core has no provider price table; products may enrich pricing in their own writer.
    summary.has_unpriced_usage = true;
    if let (Some(input), Some(read)) = (usage.input_tokens, usage.cache_read_tokens)
        && read <= input
        && usage
            .cache_write_tokens
            .is_none_or(|write| write <= input - read)
    {
        summary.cache_input_tokens = summary.cache_input_tokens.saturating_add(input);
        summary.cache_read_tokens = summary.cache_read_tokens.saturating_add(read);
    } else {
        summary.cache_incomplete = true;
    }
}
