//! Provider-independent, data-driven token pricing. No network or wall clock reads.

use serde::{Deserialize, Serialize};

pub use pl_protocol::{
    InferenceAccounting, ModelPriceTierDto, ModelPricingDto, ModelPricingSnapshot, PricingMode,
    PricingOutcome, RuntimeCostAmount, UnpricedReason, UsageReport, UsageStatus,
};

/// One rate table selected by the complete request's input and output lengths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPriceTier {
    pub input_from: u64,
    pub input_until: Option<u64>,
    pub output_from: u64,
    pub output_until: Option<u64>,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: Option<f64>,
    pub cache_write_per_mtok: Option<f64>,
}

impl TokenPriceTier {
    /// A rate table applying to all request lengths.
    pub fn flat(
        input: f64,
        output: f64,
        cache_read: Option<f64>,
        cache_write: Option<f64>,
    ) -> Self {
        Self {
            input_from: 0,
            input_until: None,
            output_from: 0,
            output_until: None,
            input_per_mtok: input,
            output_per_mtok: output,
            cache_read_per_mtok: cache_read,
            cache_write_per_mtok: cache_write,
        }
    }

    fn applies(&self, input: u64, output: u64) -> bool {
        input >= self.input_from
            && self.input_until.is_none_or(|end| input < end)
            && output >= self.output_from
            && self.output_until.is_none_or(|end| output < end)
    }
}

/// A half-open local time interval, in minutes since midnight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPriceWindow {
    pub start_minute: u16,
    pub end_minute: u16,
}

/// One weekly adjustment, sufficient for the current peak/off-peak schedules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyPriceAdjustment {
    pub utc_offset_minutes: i16,
    /// ISO weekdays: Monday = 1, Sunday = 7.
    pub weekdays: Vec<u8>,
    pub windows: Vec<DailyPriceWindow>,
    pub multiplier: f64,
}

impl WeeklyPriceAdjustment {
    fn multiplier_at(&self, request_started_at: i64) -> f64 {
        let local = i128::from(request_started_at) + i128::from(self.utc_offset_minutes) * 60;
        let day = local.div_euclid(86_400);
        // 1970-01-01 was Thursday. Euclidean division also supports pre-epoch timestamps.
        let weekday = (day + 3).rem_euclid(7) as u8 + 1;
        let minute = (local.rem_euclid(86_400) / 60) as u16;
        if self.weekdays.contains(&weekday)
            && self
                .windows
                .iter()
                .any(|window| minute >= window.start_minute && minute < window.end_minute)
        {
            self.multiplier
        } else {
            1.0
        }
    }
}

/// Current catalog prices. An unknown price never means a free model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ModelPricing {
    #[default]
    Unknown,
    Rates {
        currency: String,
        tiers: Vec<TokenPriceTier>,
        weekly_adjustment: Option<WeeklyPriceAdjustment>,
        source: String,
        verified_at: i64,
    },
}

/// Invalid declarative prices are rejected before a request is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PricingError {
    #[error("price table needs a currency and at least one tariff")]
    EmptyTable,
    #[error("token prices must be finite and non-negative")]
    InvalidRate,
    #[error("price tier intervals must be non-empty")]
    InvalidInterval,
    #[error("price tiers overlap")]
    OverlappingTiers,
    #[error("weekly price adjustment has invalid weekdays, times, offset or multiplier")]
    InvalidSchedule,
}

impl ModelPricing {
    /// Validates the tariff definition independently of provider transport.
    /// # Errors
    /// Returns invalid prices, intervals or weekly schedules.
    pub fn validate(&self) -> Result<(), PricingError> {
        let Self::Rates {
            currency,
            tiers,
            weekly_adjustment,
            ..
        } = self
        else {
            return Ok(());
        };
        if currency.trim().is_empty() || tiers.is_empty() {
            return Err(PricingError::EmptyTable);
        }
        for (index, tier) in tiers.iter().enumerate() {
            if [
                Some(tier.input_per_mtok),
                Some(tier.output_per_mtok),
                tier.cache_read_per_mtok,
                tier.cache_write_per_mtok,
            ]
            .into_iter()
            .flatten()
            .any(|rate| !rate.is_finite() || rate < 0.0)
            {
                return Err(PricingError::InvalidRate);
            }
            if tier.input_until.is_some_and(|end| end <= tier.input_from)
                || tier.output_until.is_some_and(|end| end <= tier.output_from)
            {
                return Err(PricingError::InvalidInterval);
            }
            for other in &tiers[..index] {
                let input_overlap = tier.input_until.is_none_or(|end| other.input_from < end)
                    && other.input_until.is_none_or(|end| tier.input_from < end);
                let output_overlap = tier.output_until.is_none_or(|end| other.output_from < end)
                    && other.output_until.is_none_or(|end| tier.output_from < end);
                if input_overlap && output_overlap {
                    return Err(PricingError::OverlappingTiers);
                }
            }
        }
        if let Some(rule) = weekly_adjustment
            && (rule.utc_offset_minutes.unsigned_abs() > 840
                || rule.weekdays.is_empty()
                || rule.weekdays.iter().any(|day| !(1..=7).contains(day))
                || rule.windows.is_empty()
                || rule.windows.iter().any(|window| {
                    window.start_minute >= window.end_minute || window.end_minute > 1440
                })
                || !rule.multiplier.is_finite()
                || rule.multiplier <= 0.0)
        {
            return Err(PricingError::InvalidSchedule);
        }
        Ok(())
    }

    /// Declares an official rate table checked on 2026-09-05 (UTC).
    pub fn published(currency: &str, tiers: Vec<TokenPriceTier>, source: &str) -> Self {
        Self::Rates {
            currency: currency.to_owned(),
            tiers,
            weekly_adjustment: None,
            source: source.to_owned(),
            verified_at: 1_788_566_400,
        }
    }

    /// Adds a weekly adjustment to a known rate table.
    pub fn with_weekly_adjustment(mut self, adjustment: WeeklyPriceAdjustment) -> Self {
        if let Self::Rates {
            weekly_adjustment, ..
        } = &mut self
        {
            *weekly_adjustment = Some(adjustment);
        }
        self
    }

    /// Projects every price band, including scheduled rates, for catalog consumers.
    pub fn catalog_pricing(&self) -> Option<ModelPricingDto> {
        let Self::Rates {
            currency,
            tiers,
            weekly_adjustment,
            source,
            verified_at,
        } = self
        else {
            return None;
        };
        let mut rows = Vec::new();
        for tier in tiers {
            let input_end = tier
                .input_until
                .map_or_else(|| "∞".to_owned(), |end| end.to_string());
            let output_end = tier
                .output_until
                .map_or_else(|| "∞".to_owned(), |end| end.to_string());
            let label = format!(
                "input [{}, {input_end}), output [{}, {output_end})",
                tier.input_from, tier.output_from
            );
            rows.push(ModelPriceTierDto {
                label: if weekly_adjustment.is_some() {
                    format!("{label}; off-peak")
                } else {
                    label.clone()
                },
                input_per_mtok: tier.input_per_mtok,
                output_per_mtok: tier.output_per_mtok,
                cache_read_per_mtok: tier.cache_read_per_mtok,
                cache_write_per_mtok: tier.cache_write_per_mtok,
            });
            if let Some(rule) = weekly_adjustment {
                let windows = rule
                    .windows
                    .iter()
                    .map(|window| {
                        format!(
                            "{:02}:{:02}–{:02}:{:02}",
                            window.start_minute / 60,
                            window.start_minute % 60,
                            window.end_minute / 60,
                            window.end_minute % 60
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                rows.push(ModelPriceTierDto {
                    label: format!(
                        "{label}; weekdays {:?}, {windows}, UTC{:+}:{:02}",
                        rule.weekdays,
                        rule.utc_offset_minutes / 60,
                        rule.utc_offset_minutes.unsigned_abs() % 60
                    ),
                    input_per_mtok: tier.input_per_mtok * rule.multiplier,
                    output_per_mtok: tier.output_per_mtok * rule.multiplier,
                    cache_read_per_mtok: tier
                        .cache_read_per_mtok
                        .map(|rate| rate * rule.multiplier),
                    cache_write_per_mtok: tier
                        .cache_write_per_mtok
                        .map(|rate| rate * rule.multiplier),
                });
            }
        }
        Some(ModelPricingDto {
            currency: currency.clone(),
            tiers: rows,
            source: source.clone(),
            verified_at: *verified_at,
        })
    }

    /// Estimates the entire invocation using a caller-supplied request timestamp.
    /// Missing classification produces an explicit unpriced result, never guessed zeros.
    pub fn account(
        &self,
        usage: UsageReport,
        mode: PricingMode,
        request_started_at: i64,
    ) -> InferenceAccounting {
        let mut accounting = InferenceAccounting {
            usage,
            request_started_at: Some(request_started_at),
            ..Default::default()
        };
        if mode == PricingMode::Disabled {
            accounting.pricing = PricingOutcome::Disabled;
            return accounting;
        }
        let result = self.estimate(&accounting.usage, request_started_at);
        match result {
            Ok((snapshot, cost, savings)) => {
                accounting.price_snapshot = Some(snapshot);
                accounting.pricing = PricingOutcome::Estimated {
                    cost,
                    cache_savings: savings,
                };
            }
            Err(reason) => accounting.pricing = PricingOutcome::Unpriced { reason },
        }
        accounting
    }

    fn estimate(
        &self,
        usage: &UsageReport,
        at: i64,
    ) -> Result<
        (
            ModelPricingSnapshot,
            RuntimeCostAmount,
            Option<RuntimeCostAmount>,
        ),
        UnpricedReason,
    > {
        match usage.status() {
            UsageStatus::Invalid => return Err(UnpricedReason::InvalidUsage),
            UsageStatus::Missing | UsageStatus::Partial => {
                return Err(UnpricedReason::MissingUsage);
            }
            UsageStatus::Reported => {}
        }
        let Self::Rates {
            currency,
            tiers,
            weekly_adjustment,
            ..
        } = self
        else {
            return Err(UnpricedReason::MissingPrice);
        };
        let input = usage.input_tokens.ok_or(UnpricedReason::MissingUsage)?;
        let output = usage.output_tokens.ok_or(UnpricedReason::MissingUsage)?;
        let mut matching = tiers.iter().filter(|tier| tier.applies(input, output));
        let tier = matching.next().ok_or(UnpricedReason::MissingPrice)?;
        if matching.next().is_some() {
            return Err(UnpricedReason::MissingPrice);
        }
        let multiplier = weekly_adjustment
            .as_ref()
            .map_or(1.0, |rule| rule.multiplier_at(at));
        let snapshot = ModelPricingSnapshot {
            currency: Some(currency.clone()),
            input_per_mtok: Some(tier.input_per_mtok * multiplier),
            output_per_mtok: Some(tier.output_per_mtok * multiplier),
            cache_read_per_mtok: tier.cache_read_per_mtok.map(|rate| rate * multiplier),
            cache_write_per_mtok: tier.cache_write_per_mtok.map(|rate| rate * multiplier),
        };
        let read = match usage.cache_read_tokens {
            Some(read) => read,
            None if tier
                .cache_read_per_mtok
                .is_some_and(|rate| rate != tier.input_per_mtok)
                && input > 0 =>
            {
                return Err(UnpricedReason::MissingCacheUsage);
            }
            None => 0,
        };
        // An absent write price means this provider does not charge a separate write category.
        let write = match (tier.cache_write_per_mtok, usage.cache_write_tokens) {
            (Some(_), Some(write)) => write,
            (Some(_), None) if input > read => return Err(UnpricedReason::MissingCacheUsage),
            (None, Some(write)) if write > 0 => return Err(UnpricedReason::MissingPrice),
            _ => 0,
        };
        let ordinary = input
            .checked_sub(read)
            .and_then(|value| value.checked_sub(write))
            .ok_or(UnpricedReason::InvalidUsage)?;
        let read_price = if read > 0 {
            tier.cache_read_per_mtok
                .ok_or(UnpricedReason::MissingPrice)?
        } else {
            tier.cache_read_per_mtok.unwrap_or(tier.input_per_mtok)
        };
        let amount = (ordinary as f64 * tier.input_per_mtok
            + read as f64 * read_price
            + write as f64 * tier.cache_write_per_mtok.unwrap_or(0.0)
            + output as f64 * tier.output_per_mtok)
            * multiplier
            / 1_000_000.0;
        if !amount.is_finite() || amount < 0.0 {
            return Err(UnpricedReason::MissingPrice);
        }
        let savings = usage.cache_read_tokens.map(|read| RuntimeCostAmount {
            currency: currency.clone(),
            amount: (read as f64 * (tier.input_per_mtok - read_price)
                - write as f64
                    * (tier.cache_write_per_mtok.unwrap_or(tier.input_per_mtok)
                        - tier.input_per_mtok))
                * multiplier
                / 1_000_000.0,
        });
        Ok((
            snapshot,
            RuntimeCostAmount {
                currency: currency.clone(),
                amount,
            },
            savings,
        ))
    }
}
