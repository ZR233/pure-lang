//! Billing uses producer receipts and committed timestamps, never current pricing.
use anyhow::Result;
use pl_core::thread::{AttemptOutcome, ThreadEffectBatch, journal::AttemptUpdate};
use pl_protocol::InferenceBillingRecord;

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
    pl_model::runtime::model_attempt_billing(attempt, recorded_at).map_err(anyhow::Error::new)
}
