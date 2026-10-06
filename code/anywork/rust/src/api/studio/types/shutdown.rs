//! FRB DTO for the Studio shutdown report.
//!
//! Mirrors the runtime protocol shape (`StudioShutdownReport`); `code` is a stable diagnostic
//! code string, not a typed error enum. Serialized camelCase with no raw JSON compatibility.

use serde::{Deserialize, Serialize};

/// Shutdown result classification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeShutdownOutcome {
    NotStarted,
    Clean,
    Degraded,
}

/// Pending reliable-persistence state observed during shutdown.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgePendingPersistence {
    Unknown,
    Pending { count: u64 },
    Drained,
}

/// One aggregated shutdown-stage diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeShutdownIssue {
    pub stage: String,
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: String,
}

/// Result of `shutdown_runtime(remaining_ms)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeShutdownReport {
    pub outcome: BridgeShutdownOutcome,
    pub issues: Vec<BridgeShutdownIssue>,
    pub persistence: BridgePendingPersistence,
}

impl BridgeShutdownReport {
    pub(crate) fn not_started() -> Self {
        Self {
            outcome: BridgeShutdownOutcome::NotStarted,
            issues: Vec::new(),
            persistence: BridgePendingPersistence::Drained,
        }
    }

    pub(crate) fn is_clean(&self) -> bool {
        matches!(self.outcome, BridgeShutdownOutcome::Clean)
    }
}
