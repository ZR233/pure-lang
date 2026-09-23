//! Thread/process persistence watermarks and queue pressure.
//!
//! These are observation facts for diagnostics and backpressure: they never carry authoritative
//! Thread state, and a missing value is unknown rather than zero.
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum HistoryFault {
    QueueFull,
    WriteFailed,
    WriterUnavailable,
    NoProgress,
    CheckpointFailed,
    BlobFailed,
}

/// One Thread's persistence watermarks and queue pressure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPersistenceSnapshot {
    pub thread_id: String,
    /// Generation required by the per-Thread manual recovery command.
    #[serde(default)]
    pub fault_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<HistoryFault>,
    /// Newest checkpoint revision admitted for publication; absent when no writer ever reported.
    ///
    /// A `None` is *unknown*, not measured zero: a Thread that only appears through a recovery
    /// diagnostic has no reporting writer, so its watermark is not a fact about it. Consumers must
    /// render unknown distinctly instead of substituting `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_dirty_revision: Option<u64>,
    /// Checkpoint revision being serialized or synced; absent when nothing is in flight or unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_saving_revision: Option<u64>,
    /// Newest checkpoint revision already published as `state.toml`; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_durable_revision: Option<u64>,
    /// Newest effect sequence admitted for the history write; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_admitted_sequence: Option<u64>,
    /// Newest effect sequence whose history write completed; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_durable_sequence: Option<u64>,
    /// Newest effect sequence admitted for the call write; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls_admitted_sequence: Option<u64>,
    /// Newest effect sequence whose call write completed; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls_durable_sequence: Option<u64>,
    /// Queued effects plus one for a retained checkpoint publication.
    pub pending_operations: u64,
    /// Encoded bytes of the queued effects.
    pub pending_bytes: u64,
    /// Age of the oldest queued operation; absent when nothing is queued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_pending_age_millis: Option<u64>,
    /// Encoded bytes of the operation currently being written.
    pub in_flight_bytes: u64,
    /// Last typed persistence error, kept until a later write succeeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Whether this Thread paused new inference admission under storage pressure.
    pub pressure_paused: bool,
}

/// Process-wide persistence pressure across every loaded Thread.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PersistenceQueueSnapshot {
    /// Call statistics may be incomplete; a missing usage value must not be read as zero.
    #[serde(default)]
    pub statistics_gap: bool,
    /// Total queued operations across all Threads.
    pub pending_operations: u64,
    /// Total queued bytes across all Threads.
    pub pending_bytes: u64,
    /// Total in-flight bytes across all Threads.
    pub in_flight_bytes: u64,
    /// Age of the oldest queued operation anywhere; absent when nothing is queued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_pending_age_millis: Option<u64>,
    /// Last typed error across Thread or call persistence, kept until a durable write succeeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Whether any Thread paused admission under storage pressure.
    pub pressure_paused: bool,
    /// Per-Thread watermarks, ordered by Thread identity.
    #[serde(default)]
    pub threads: Vec<ThreadPersistenceSnapshot>,
}
