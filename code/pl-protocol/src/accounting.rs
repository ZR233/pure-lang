//! Final provider usage and the immutable estimate attached to an invocation.

use serde::{Deserialize, Serialize};

use crate::{InferenceTokenUsage, ModelPricingSnapshot, RuntimeCostAmount};

/// Provider-reported counters. Absence means unknown, not zero.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

/// Whether the provider supplied usable input/output counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageStatus {
    Missing,
    Partial,
    Reported,
    Invalid,
}

impl UsageReport {
    /// Checks relationships without changing the provider's reported values.
    pub fn status(&self) -> UsageStatus {
        if self == &Self::default() {
            return UsageStatus::Missing;
        }
        if let Some(input) = self.input_tokens {
            let read = self.cache_read_tokens.unwrap_or(0);
            let write = self.cache_write_tokens.unwrap_or(0);
            if read.checked_add(write).is_none_or(|sum| sum > input) {
                return UsageStatus::Invalid;
            }
        }
        if let (Some(output), Some(reasoning)) = (self.output_tokens, self.reasoning_tokens)
            && reasoning > output
        {
            return UsageStatus::Invalid;
        }
        match (self.input_tokens, self.output_tokens) {
            (Some(input), Some(output)) => match input.checked_add(output) {
                Some(total) if self.total_tokens.is_none_or(|reported| reported == total) => {
                    UsageStatus::Reported
                }
                _ => UsageStatus::Invalid,
            },
            _ => UsageStatus::Partial,
        }
    }

    /// A usable total for the current context; absent or inconsistent reports do not reset it.
    pub fn known_total_tokens(&self) -> Option<u64> {
        if self.status() == UsageStatus::Invalid {
            return None;
        }
        self.total_tokens
            .or_else(|| self.input_tokens?.checked_add(self.output_tokens?))
    }

    /// Projects known counters for summation. Completeness remains on this report.
    ///
    /// Unknown counters contribute nothing; this is not a replacement usage report.
    pub fn totals(&self) -> InferenceTokenUsage {
        InferenceTokenUsage {
            prompt_tokens: self.input_tokens.unwrap_or(0),
            completion_tokens: self.output_tokens.unwrap_or(0),
            cached_prompt_tokens: self.cache_read_tokens.unwrap_or(0),
            cache_write_tokens: self.cache_write_tokens.unwrap_or(0),
            reasoning_tokens: self.reasoning_tokens.unwrap_or(0),
            total_tokens: self
                .total_tokens
                .or_else(|| self.input_tokens?.checked_add(self.output_tokens?))
                .unwrap_or(0),
        }
    }
}

/// Why a complete monetary estimate cannot be produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnpricedReason {
    MissingUsage,
    InvalidUsage,
    MissingPrice,
    MissingCacheUsage,
    UnsupportedServiceTier,
}

/// Monetary estimates are independent of whether tokens were reported.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PricingOutcome {
    Disabled,
    Unpriced {
        reason: UnpricedReason,
    },
    Estimated {
        cost: RuntimeCostAmount,
        cache_savings: Option<RuntimeCostAmount>,
    },
}

impl Default for PricingOutcome {
    fn default() -> Self {
        Self::Unpriced {
            reason: UnpricedReason::MissingUsage,
        }
    }
}

/// Canonical final accounting, retained even when an invocation fails.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceAccounting {
    pub usage: UsageReport,
    pub pricing: PricingOutcome,
    pub price_snapshot: Option<ModelPricingSnapshot>,
    pub request_started_at: Option<i64>,
}

impl InferenceAccounting {
    /// Frozen known monetary contribution; disabled and unknown are not zero prices.
    pub fn estimated_costs(&self) -> Vec<RuntimeCostAmount> {
        match &self.pricing {
            PricingOutcome::Estimated { cost, .. } => vec![cost.clone()],
            PricingOutcome::Disabled | PricingOutcome::Unpriced { .. } => Vec::new(),
        }
    }

    /// Frozen cache savings, when the input breakdown and rates were available.
    pub fn estimated_cache_savings(&self) -> Vec<RuntimeCostAmount> {
        match &self.pricing {
            PricingOutcome::Estimated { cache_savings, .. } => {
                cache_savings.iter().cloned().collect()
            }
            PricingOutcome::Disabled | PricingOutcome::Unpriced { .. } => Vec::new(),
        }
    }

    /// Whether token or cache-read totals contain unreported or inconsistent contributions.
    pub fn has_incomplete_usage(&self) -> bool {
        self.usage.status() != UsageStatus::Reported || self.usage.cache_read_tokens.is_none()
    }

    pub fn has_unpriced_usage(&self) -> bool {
        matches!(self.pricing, PricingOutcome::Unpriced { .. })
    }
}

/// Explicit per-provider monetary accounting choice, frozen for each invocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PricingMode {
    #[default]
    Catalog,
    Disabled,
}
