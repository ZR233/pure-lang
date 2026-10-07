//! Billing uses producer receipts and committed timestamps, never current pricing.
use anyhow::Result;
use pl_core::thread::{AttemptOutcome, ThreadEffectBatch, journal::AttemptUpdate};
use pl_protocol::{InferenceAccounting, InferenceBillingRecord};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuxiliaryBillingReceipt {
    pub billing: InferenceBillingRecord,
    pub succeeded: bool,
}

pub(in crate::studio) struct BillingFact {
    pub status: super::super::storage::calls::CallStatus,
    pub record: InferenceBillingRecord,
    pub auxiliary: bool,
}

pub(in crate::studio) fn billing_facts(commit: &ThreadEffectBatch) -> Result<Vec<BillingFact>> {
    let mut facts = Vec::new();
    for change in commit.extensions.iter() {
        if let pl_core::thread::extensions::ExtensionChange::Put { record, .. } = change
            && record.payload.format() == "pl.studio.auxiliary-billing"
        {
            anyhow::ensure!(
                record.payload.version() == 1,
                "unsupported auxiliary billing version"
            );
            let receipt: AuxiliaryBillingReceipt = serde_json::from_str(record.payload.content())?;
            facts.push(BillingFact {
                status: if receipt.succeeded {
                    super::super::storage::calls::CallStatus::Committed
                } else {
                    super::super::storage::calls::CallStatus::Failed
                },
                record: receipt.billing,
                auxiliary: true,
            });
        }
        if let pl_core::thread::extensions::ExtensionChange::Put { record, .. } = change
            && record.payload.format() == "pl.studio.compaction"
        {
            if record.payload.version() != 1 {
                anyhow::bail!("unsupported auxiliary accounting receipt");
            }
            let receipt: crate::compaction::CompactionReceipt =
                serde_json::from_str(record.payload.content())?;
            let status = if receipt.error.is_none() {
                super::super::storage::calls::CallStatus::Committed
            } else {
                super::super::storage::calls::CallStatus::Failed
            };
            let billing = InferenceBillingRecord {
                inference_id: receipt.inference_id,
                purpose: Some(receipt.binding.purpose),
                provider_instance_id: receipt.binding.provider_instance_id.clone(),
                provider: receipt.binding.provider_instance_id,
                model: receipt.model_observation.as_ref().map_or_else(
                    || receipt.binding.requested_model.clone(),
                    |value| value.sent_model.clone(),
                ),
                model_observation: receipt.model_observation,
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
            facts.push(BillingFact {
                record: billing,
                status,
                auxiliary: true,
            });
        }
    }
    if let Some(attempt) = &commit.attempt
        && let Some(record) = attempt_billing(attempt, commit.committed_at)?
    {
        facts.push(BillingFact {
            record,
            status: match &attempt.outcome {
                AttemptOutcome::Committed(_) => super::super::storage::calls::CallStatus::Committed,
                AttemptOutcome::Rejected { .. } => {
                    super::super::storage::calls::CallStatus::Rejected
                }
                AttemptOutcome::Cancelled { .. } => {
                    super::super::storage::calls::CallStatus::Cancelled
                }
                _ => super::super::storage::calls::CallStatus::Failed,
            },
            auxiliary: false,
        });
    }
    Ok(facts)
}

fn attempt_billing(
    attempt: &AttemptUpdate,
    recorded_at: i64,
) -> Result<Option<InferenceBillingRecord>> {
    let output = match &attempt.outcome {
        AttemptOutcome::Running | AttemptOutcome::Interrupted => return Ok(None),
        AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => Ok(output),
        AttemptOutcome::Failed(error) => Err(error.as_ref()),
        AttemptOutcome::Cancelled { result } => result.as_ref().map_err(std::sync::Arc::as_ref),
    };
    let request = attempt
        .request_metadata
        .as_ref()
        .map(pl_model::runtime::model_request_receipt)
        .transpose()?;
    let (binding, accounting, model, model_observation, timing, orchestration) = match output {
        Ok(output) => match pl_model::runtime::model_response_receipt(output)? {
            Some(receipt) => (
                Some(receipt.binding),
                receipt.response.accounting,
                receipt.response.model,
                receipt.response.model_observation,
                receipt.response.timing,
                receipt.response.orchestration,
            ),
            None => (
                request.as_ref().map(|request| request.binding.clone()),
                unknown(&output.usage),
                String::new(),
                None,
                None,
                Default::default(),
            ),
        },
        Err(error) => match pl_model::runtime::model_failure_receipt(error)? {
            Some(receipt) => (
                Some(receipt.binding),
                receipt.accounting,
                String::new(),
                receipt.model_observation,
                None,
                Default::default(),
            ),
            None => (
                request.as_ref().map(|request| request.binding.clone()),
                unknown(&error.usage),
                String::new(),
                None,
                None,
                Default::default(),
            ),
        },
    };
    let model = if let Some(observation) = &model_observation {
        observation.sent_model.clone()
    } else if model.is_empty() {
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
        model_observation,
        reasoning_effort: request
            .as_ref()
            .and_then(|request| request.reasoning.as_ref())
            .and_then(|reasoning| reasoning.effort.clone()),
        context_window: binding.as_ref().and_then(|binding| binding.context_window),
        accounting,
        prompt_generation: request
            .as_ref()
            .and_then(|request| request.prompt.as_ref())
            .map(|prompt| prompt.generation),
        prompt_cache_policy: request
            .as_ref()
            .and_then(|request| request.prompt.as_ref())
            .map(|prompt| prompt.prompt_cache_policy.clone()),
        prefix_changed_reason: request
            .as_ref()
            .and_then(|request| request.prompt.as_ref())
            .map(|prompt| prompt.prefix_changed_reason),
        orchestration,
        timing,
        recorded_at,
    };
    Ok(Some(billing))
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
