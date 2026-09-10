//! Billing uses producer receipts and committed timestamps, never current pricing.
use super::super::ModelPerformanceOwner;
use anyhow::Result;
use pl_core::thread::{AttemptOutcome, journal::ThreadCommit};
use pl_protocol::{InferenceAccounting, InferenceBillingRecord};

pub(super) fn record(
    owner: &ModelPerformanceOwner,
    root: &str,
    commit: &ThreadCommit,
) -> Result<()> {
    for change in commit.extensions.iter() {
        if let pl_core::thread::extensions::ExtensionChange::Put { record, .. } = change
            && record.payload.format() == "pl.studio.compaction"
        {
            if record.payload.version() != 1 {
                anyhow::bail!("unsupported auxiliary accounting receipt");
            }
            let receipt: crate::compaction::CompactionReceipt =
                serde_json::from_str(record.payload.content())?;
            let billing = InferenceBillingRecord {
                inference_id: receipt.inference_id,
                purpose: Some(receipt.binding.purpose),
                provider_instance_id: receipt.binding.provider_instance_id.clone(),
                provider: receipt.binding.provider_instance_id,
                model: receipt.binding.requested_model,
                reasoning_effort: receipt.reasoning_effort,
                context_window: receipt.context_window,
                accounting: receipt.accounting,
                prompt_generation: None,
                prompt_cache_policy: None,
                prefix_changed_reason: None,
                orchestration: Default::default(),
                timing: None,
                recorded_at: commit.committed_at,
            };
            owner.record_auxiliary_inference(root, &commit.thread_id, &billing)?;
        }
    }
    let Some(attempt) = &commit.attempt else {
        return Ok(());
    };
    let output = match &attempt.outcome {
        AttemptOutcome::Running | AttemptOutcome::Interrupted => return Ok(()),
        AttemptOutcome::Committed(output) | AttemptOutcome::Rejected(output) => Ok(output),
        AttemptOutcome::Failed(error) => Err(error.as_ref()),
        AttemptOutcome::Cancelled { result } => result.as_ref().map_err(std::sync::Arc::as_ref),
    };
    let request = attempt
        .request_metadata
        .as_ref()
        .map(pl_model::runtime::model_request_receipt)
        .transpose()?;
    let (binding, accounting, model, timing, orchestration) = match output {
        Ok(output) => match pl_model::runtime::model_response_receipt(output)? {
            Some(receipt) => (
                Some(receipt.binding),
                receipt.response.accounting,
                receipt.response.model,
                receipt.response.timing,
                receipt.response.orchestration,
            ),
            None => (
                request.as_ref().map(|request| request.binding.clone()),
                unknown(&output.usage),
                String::new(),
                None,
                Default::default(),
            ),
        },
        Err(error) => match pl_model::runtime::model_failure_receipt(error)? {
            Some(receipt) => (
                Some(receipt.binding),
                receipt.accounting,
                String::new(),
                None,
                Default::default(),
            ),
            None => (
                request.as_ref().map(|request| request.binding.clone()),
                unknown(&error.usage),
                String::new(),
                None,
                Default::default(),
            ),
        },
    };
    let model = if model.is_empty() {
        binding
            .as_ref()
            .map_or_else(String::new, |binding| binding.requested_model.clone())
    } else {
        model
    };
    let provider = binding
        .as_ref()
        .map_or_else(String::new, |binding| binding.provider_instance_id.clone());
    let billing = InferenceBillingRecord {
        inference_id: attempt.attempt_id.clone(),
        purpose: binding.as_ref().map(|binding| binding.purpose.clone()),
        provider_instance_id: provider.clone(),
        provider,
        model,
        reasoning_effort: request
            .and_then(|request| request.reasoning)
            .and_then(|reasoning| reasoning.effort),
        context_window: None,
        accounting,
        prompt_generation: None,
        prompt_cache_policy: None,
        prefix_changed_reason: None,
        orchestration,
        timing,
        recorded_at: commit.committed_at,
    };
    owner.record_inference(root, &commit.thread_id, &billing)?;
    Ok(())
}
fn unknown(usage: &pl_core::model::ModelUsage) -> InferenceAccounting {
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
