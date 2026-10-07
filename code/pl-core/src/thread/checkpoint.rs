//! Direct restart state for one Thread owner.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::extensions::ExtensionMutation;
use super::inbox::InboxRecord;
use super::input::InputRecord;
use super::interactions::{InteractionRecord, InteractionResponse, InteractionState};
use super::permissions::PermissionRecord;
use super::{
    AttemptOutcome, AttemptStatus, RequestAttempt, RuntimeFact, ThreadError, ThreadSnapshot,
    ToolDelivery, attempt_outcome_usage, recovery,
};
use crate::context::{ContextContent, ContextRecord, ContextSnapshot, OpaquePayload};
use crate::model::ModelToolDeclaration;
use crate::tool::{ToolControl, ToolOutput};

/// Bodies larger than this leave the checkpoint file and are named by a blob reference instead.
///
/// The threshold is checkpoint-owner policy, not a context limit. Hosts that store external
/// checkpoint bodies can use this threshold; Studio stores current context records incrementally
/// in its session database and only reads external bodies at the legacy migration boundary.
pub const CHECKPOINT_BODY_THRESHOLD_BYTES: usize = 64 * 1024;

/// A versioned, integrity-checked reference to one body the session blob store owns.
///
/// The reference carries the content address of the exact bytes and their length, so a loader
/// either restores the original body or fails closed; it never substitutes truncated or replaced
/// content. Physical paths stay with the blob store, which derives them from this digest, so the
/// reference stays valid when a session directory is relocated.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointBodyReference {
    /// Reference-format version; a loader rejects any version it does not implement.
    pub reference_version: u32,
    /// Content address of the exact bytes: `sha256:<64 hex digits>`.
    pub digest: String,
    /// Exact length of the referenced bytes.
    pub byte_len: u64,
}

impl CheckpointBodyReference {
    /// Reference format written by this build.
    pub const VERSION: u32 = 1;

    /// Builds the reference for one body from the exact bytes being externalized.
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            reference_version: Self::VERSION,
            digest: crate::context::content_hash(bytes),
            byte_len: bytes.len() as u64,
        }
    }

    /// Returns the content address a loader reads the body by.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Returns the exact length the loader must observe.
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Validates the reference metadata without reading the blob.
    ///
    /// # Errors
    /// Rejects an unimplemented reference version or a missing content address.
    pub fn validate(&self) -> Result<(), CheckpointBodyError> {
        if self.reference_version != Self::VERSION {
            return Err(CheckpointBodyError::UnsupportedReferenceVersion(
                self.reference_version,
            ));
        }
        let Some(hex) = self.digest.strip_prefix("sha256:") else {
            return Err(CheckpointBodyError::InvalidReference);
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CheckpointBodyError::InvalidReference);
        }
        if self.byte_len == 0 {
            return Err(CheckpointBodyError::InvalidReference);
        }
        Ok(())
    }

    /// Verifies the exact bytes a loader read for this reference.
    ///
    /// # Errors
    /// Rejects replaced or truncated bytes, and malformed reference metadata.
    pub fn verify(&self, bytes: &[u8]) -> Result<(), CheckpointBodyError> {
        self.validate()?;
        if bytes.len() as u64 != self.byte_len || crate::context::content_hash(bytes) != self.digest
        {
            return Err(CheckpointBodyError::ContentMismatch);
        }
        Ok(())
    }
}

/// Which saved slot an externalized body came from, and therefore how it is restored.
///
/// The slot, not the digest, is the body's identity: two slots may legitimately hold the same
/// bytes, and a loader must refill each of them from the reference the manifest names for it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    tag = "kind",
    rename_all_fields = "camelCase"
)]
pub enum CheckpointBodySlot {
    /// One [`ContextContent`] of a current-context record.
    ContextContent {
        record_id: String,
        content_index: usize,
    },
    /// The arguments of one framework tool call in a current-context record.
    ///
    /// The call identity stays framework-owned, but its arguments are producer content and can be as
    /// large as any other body.
    ContextToolCall {
        record_id: String,
        call_index: usize,
    },
    /// The producer payload of one unconsumed input.
    InputPayload { ordinal: u64 },
    /// One content item of one unconsumed input.
    InputContent { ordinal: u64, content_index: usize },
    /// The producer payload of one unconsumed inbox message.
    InboxPayload { sequence: u64 },
    /// One content item of one unconsumed inbox message.
    InboxContent { sequence: u64, content_index: usize },
    /// The producer payload of one pending interaction request.
    InteractionRequest { id: String },
    /// The producer payload of one resolved interaction response.
    InteractionResponsePayload { id: String },
    /// One content item of one resolved interaction response.
    InteractionResponseContent { id: String, content_index: usize },
    /// One proposed application record carried by an interaction resolution.
    InteractionMutationPayload { id: String, mutation_index: usize },
    /// The pending approval prompt of one live tool call.
    PermissionPayload { id: String },
    /// The exact host decision material of one live permission record.
    PermissionResponse { id: String },
    /// The producer payload of a pending tool delivery.
    DeliveryPayload { call_id: String },
    /// A model-visible projection item of a pending tool delivery result.
    DeliveryOutputContext {
        call_id: String,
        content_index: usize,
    },
    /// A model-visible projection item a pending tool delivery still has to deliver.
    DeliveryDeliveredContext {
        call_id: String,
        content_index: usize,
    },
    /// The interaction request of a pending tool delivery that awaits its host answer.
    DeliveryInteraction { call_id: String },
    /// The payload of one current application record.
    ExtensionPayload { id: String },
    /// One content item of the current facts of a stable host source.
    RuntimeFactContent {
        source_id: String,
        content_index: usize,
    },
    /// The producer declaration of one discovered tool.
    ToolDeclaration { tool_index: usize },
    /// The Thread's current private context.
    PrivateContext,
}

/// Original content kind of an externalized body, so the loader restores the same variant.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    tag = "kind",
    rename_all_fields = "camelCase"
)]
pub enum CheckpointBodyKind {
    /// A [`ContextContent::Text`] body.
    Text,
    /// An opaque body, with the producer-owned format and version it was frozen with.
    Opaque { format: String, version: u32 },
}

/// One body the checkpoint no longer inlines, and the slot it must be restored into.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointExternalBody {
    pub slot: CheckpointBodySlot,
    pub body: CheckpointBodyKind,
    pub reference: CheckpointBodyReference,
}

/// One body a publisher must make durable before it may write a checkpoint that names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedCheckpointBody {
    pub entry: CheckpointExternalBody,
    pub bytes: Vec<u8>,
}

/// An externalized body could not be produced or restored exactly.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckpointBodyError {
    #[error("checkpoint body reference version {0} is not supported")]
    UnsupportedReferenceVersion(u32),
    #[error("checkpoint body reference needs a SHA-256 digest and a nonzero length")]
    InvalidReference,
    #[error("checkpoint body bytes differ from their recorded digest or length")]
    ContentMismatch,
    #[error("checkpoint body {0} is not pending in this checkpoint")]
    UnknownReference(String),
    #[error("checkpoint body slot does not match the saved state: {0}")]
    SlotMismatch(String),
    #[error("checkpoint body cannot be restored as its recorded content kind")]
    InvalidBody,
}

/// A schema-1/2 checkpoint envelope could not be converted to the current resident form.
#[derive(Debug, thiserror::Error)]
pub enum CheckpointLegacyError {
    #[error("legacy attempt body references an absent attempt")]
    InvalidAttemptSlot,
    #[error("checkpoint schema {0} is not a convertible legacy version")]
    UnsupportedVersion(u32),
    #[error("legacy checkpoint still names external bodies; materialize them before converting")]
    ExternalBodiesPending,
    #[error("invalid legacy checkpoint encoding: {0}")]
    Encoding(#[from] serde_json::Error),
}

/// One body a schema-1/2 checkpoint externalized from a resident attempt.
///
/// Schema 3 no longer keeps an attempt's frozen request snapshot, but a legacy manifest still names
/// the exact slots those bodies used. The slot — including its `attempt_index` — is decoded here
/// before anything is dropped, so the old layout is reified with its original indices instead of
/// being filtered positionally and losing which attempt a body belonged to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    tag = "kind",
    rename_all_fields = "camelCase"
)]
pub enum LegacyCheckpointBodySlot {
    /// The provider request metadata of one resident attempt.
    AttemptMetadata { attempt_index: usize },
    /// The frozen tool projection of one resident attempt.
    AttemptToolProjection { attempt_index: usize },
    /// The producer declaration of one tool offered to one resident attempt.
    AttemptToolDeclaration {
        attempt_index: usize,
        tool_index: usize,
    },
    /// One content item of one resident attempt's frozen model input.
    AttemptInputContent {
        attempt_index: usize,
        record_id: String,
        content_index: usize,
    },
    /// One tool call argument of one resident attempt's frozen model input.
    AttemptInputToolCall {
        attempt_index: usize,
        record_id: String,
        call_index: usize,
    },
    /// One content item of one resident attempt's produced step.
    AttemptOutputContent {
        attempt_index: usize,
        content_index: usize,
    },
    /// One tool call argument of one resident attempt's produced step.
    AttemptOutputToolCall {
        attempt_index: usize,
        call_index: usize,
    },
    /// The private context proposed by one resident attempt.
    AttemptOutputPrivateContext { attempt_index: usize },
    /// The provider-owned failure details of one resident attempt's error outcome.
    AttemptErrorDetails { attempt_index: usize },
    /// The diagnostic source-chain text of one resident attempt's error outcome.
    AttemptErrorSource { attempt_index: usize },
}

impl LegacyCheckpointBodySlot {
    /// Index of the resident attempt this externalized body belonged to.
    pub fn attempt_index(&self) -> usize {
        match self {
            Self::AttemptMetadata { attempt_index }
            | Self::AttemptToolProjection { attempt_index }
            | Self::AttemptToolDeclaration { attempt_index, .. }
            | Self::AttemptInputContent { attempt_index, .. }
            | Self::AttemptInputToolCall { attempt_index, .. }
            | Self::AttemptOutputContent { attempt_index, .. }
            | Self::AttemptOutputToolCall { attempt_index, .. }
            | Self::AttemptOutputPrivateContext { attempt_index }
            | Self::AttemptErrorDetails { attempt_index }
            | Self::AttemptErrorSource { attempt_index } => *attempt_index,
        }
    }
}

/// One externalized body a schema-1/2 checkpoint named for a resident attempt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyCheckpointExternalBody {
    pub slot: LegacyCheckpointBodySlot,
    pub body: CheckpointBodyKind,
    pub reference: CheckpointBodyReference,
}

/// One resident attempt exactly as schema-1/2 checkpoints stored it: identity, frozen input, tool
/// plan and complete outcome.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyRequestAttempt {
    #[serde(default)]
    pub request_metadata: Option<OpaquePayload>,
    #[serde(default)]
    pub usage_binding: Option<crate::model::ModelUsageBinding>,
    #[serde(default)]
    pub tool_projection: Option<OpaquePayload>,
    pub turn_id: String,
    pub attempt_id: String,
    #[serde(default)]
    pub retry_of: Option<String>,
    pub input: ContextSnapshot,
    pub tools: Arc<[ModelToolDeclaration]>,
    pub outcome: AttemptOutcome,
    #[serde(default)]
    pub input_estimate: Option<crate::model::TokenEstimate>,
}

impl LegacyRequestAttempt {
    /// Reduces a complete legacy attempt to its lightweight resident form.
    ///
    /// Only identity, the frozen `input_revision`, the status derived from the outcome and the
    /// observed usage survive: the frozen request snapshot is the matching effect batch's copy, so
    /// it is intentionally dropped here.
    fn into_current(self) -> RequestAttempt {
        RequestAttempt {
            turn_id: self.turn_id,
            attempt_id: self.attempt_id,
            retry_of: self.retry_of,
            input_revision: self.input.revision,
            status: AttemptStatus::from_outcome(&self.outcome),
            usage: attempt_outcome_usage(&self.outcome).cloned(),
            facts: None,
        }
    }
}

/// A schema-1/2 checkpoint reified in its original layout.
///
/// Decoding keeps the legacy attempt list and the attempt-slot manifest intact: an attempt body is
/// never dropped by position, so the exact slot-to-body mapping the old checkpoint stored is
/// preserved before anything is converted. A blob-owning caller materializes every body the live
/// context still needs through [`Self::pending_body`]/[`Self::materialize_body`] and then converts
/// with [`Self::into_current`]. An attempt's frozen request snapshot no longer has a resident slot,
/// so it is dropped together with its manifest entry; the conversion still refuses a manifest that
/// names a current-context body nobody materialized, so the live context can never lose its text.
#[derive(Debug, Clone)]
pub struct LegacyThreadCheckpoint {
    checkpoint: ThreadCheckpoint,
    attempts: Vec<LegacyRequestAttempt>,
    attempt_bodies: Vec<LegacyCheckpointExternalBody>,
}

impl LegacyThreadCheckpoint {
    /// Reifies one schema-1/2 JSON envelope without converting it.
    ///
    /// # Errors
    /// Rejects a non-legacy schema version and malformed JSON.
    pub fn decode_json(envelope: &str) -> Result<Self, CheckpointLegacyError> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Header {
            schema_version: u32,
        }
        let header: Header = serde_json::from_str(envelope)?;
        if !ThreadCheckpoint::is_legacy_schema(header.schema_version) {
            return Err(CheckpointLegacyError::UnsupportedVersion(
                header.schema_version,
            ));
        }
        if header.schema_version == 3 {
            let checkpoint: ThreadCheckpoint = serde_json::from_str(envelope)?;
            return Ok(Self {
                checkpoint,
                attempts: Vec::new(),
                attempt_bodies: Vec::new(),
            });
        }
        let mut root: serde_json::Value = serde_json::from_str(envelope)?;
        let mut attempts = Vec::new();
        let mut attempt_bodies = Vec::new();
        if let Some(object) = root.as_object_mut() {
            // Split the manifest by decoding each entry's slot into the legacy attempt vocabulary.
            // An entry that is not one of those slots is left untouched and parsed as a current
            // slot by the inner checkpoint, so a current-context body is never mistaken for a
            // droppable attempt body.
            let mut current_entries = Vec::new();
            if let Some(serde_json::Value::Array(entries)) = object.remove("externalBodies") {
                for entry in entries {
                    match serde_json::from_value::<LegacyCheckpointExternalBody>(entry.clone()) {
                        Ok(attempt_entry) => attempt_bodies.push(attempt_entry),
                        Err(_) => current_entries.push(entry),
                    }
                }
            }
            if !current_entries.is_empty() {
                object.insert(
                    "externalBodies".to_owned(),
                    serde_json::Value::Array(current_entries),
                );
            }
            if let Some(serde_json::Value::Object(state)) = object.get_mut("state") {
                let raw = state
                    .remove("attempts")
                    .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
                state.insert("attempts".to_owned(), serde_json::Value::Array(Vec::new()));
                attempts = serde_json::from_value(raw)?;
            }
        }
        if attempt_bodies
            .iter()
            .any(|entry: &LegacyCheckpointExternalBody| {
                entry.slot.attempt_index() >= attempts.len()
            })
        {
            return Err(CheckpointLegacyError::InvalidAttemptSlot);
        }
        let mut checkpoint: ThreadCheckpoint = serde_json::from_value(root)?;
        checkpoint.schema_version = ThreadCheckpoint::SCHEMA_VERSION;
        Ok(Self {
            checkpoint,
            attempts,
            attempt_bodies,
        })
    }

    /// Whether every current-context body the legacy checkpoint named has been materialized.
    pub fn is_materialized(&self) -> bool {
        self.checkpoint.is_materialized()
    }

    /// The first current-context body still pending materialization, in manifest order.
    pub fn pending_body(&self) -> Option<&CheckpointExternalBody> {
        self.checkpoint.pending_body()
    }

    /// Restores one current-context body from the exact bytes its owner read.
    ///
    /// # Errors
    /// Fails closed exactly like [`ThreadCheckpoint::materialize_body`].
    pub fn materialize_body(
        &mut self,
        reference: &CheckpointBodyReference,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        self.checkpoint.materialize_body(reference, bytes)
    }

    /// Frozen request snapshots the conversion drops, so a caller can inspect what a legacy
    /// checkpoint carried beyond its current context.
    pub fn dropped_attempt_bodies(&self) -> &[LegacyCheckpointExternalBody] {
        &self.attempt_bodies
    }

    /// Converts the reified legacy checkpoint into the current schema-4 resident form.
    ///
    /// # Errors
    /// Rejects a still-pending current-context body: converting would drop body text the live
    /// context still needs. Attempt bodies are dropped without error because the committed effect
    /// batch is their authoritative copy.
    pub fn into_current(self) -> Result<ThreadCheckpoint, CheckpointLegacyError> {
        let Self {
            mut checkpoint,
            attempts,
            attempt_bodies: _,
        } = self;
        if !checkpoint.external_bodies.is_empty() {
            return Err(CheckpointLegacyError::ExternalBodiesPending);
        }
        // The legacy envelope already stored `liveCalls`; attempt identity is rebuilt from the
        // attempts it carried so a retry can never re-admit an already-observed identity.
        for attempt in &attempts {
            checkpoint
                .state
                .attempt_ids
                .insert(attempt.attempt_id.clone(), attempt.turn_id.clone());
            if let AttemptOutcome::Committed(output) = &attempt.outcome {
                for call in output.tool_calls.iter() {
                    checkpoint
                        .state
                        .live_calls
                        .insert(call.call_id.clone(), attempt.turn_id.clone());
                }
            }
        }
        if checkpoint.schema_version != 3 {
            checkpoint.state.attempts = attempts
                .into_iter()
                .map(LegacyRequestAttempt::into_current)
                .collect::<Vec<_>>()
                .into();
        }
        checkpoint.schema_version = ThreadCheckpoint::SCHEMA_VERSION;
        checkpoint.state.last_attempt_usage_origin = None;
        let sources = checkpoint
            .state
            .runtime_facts
            .iter()
            .map(|fact| fact.source_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for record in std::sync::Arc::make_mut(&mut checkpoint.state.context.records) {
            if let crate::context::ContextSource::Runtime { source_id } = &record.source
                && record.id.starts_with("inbox:")
            {
                record.source = crate::context::ContextSource::AgentMessage {
                    source_id: source_id.clone(),
                    purpose: crate::context::AgentMessageKind::Unclassified,
                };
            }
            if let crate::context::ContextSource::Runtime { source_id } = &record.source
                && record.id.starts_with("runtime:")
                && sources.contains(source_id.as_str())
            {
                record.source = crate::context::ContextSource::RuntimeFact {
                    source_id: source_id.clone(),
                };
            }
        }
        Ok(checkpoint)
    }
}

/// Versioned current-state checkpoint. History is referenced only by its durable fence.
///
/// The restart form carries the facts needed to resume current logical execution and nothing else:
/// its state is pruned of finished attempts/turns, delivered results, replaced context versions,
/// completed interactions and exported effect deltas, all of which stay reachable through the
/// durable history that `history_fence` points at. [`Self::capture_transfer`] is the same envelope
/// paired with one committed effect, keeping that commit's own facts so a writer can project the
/// effect; it must be reduced with [`Self::pruned`] before it is published.
///
/// A checkpoint is a bounded logical state, not a second copy of every body: [`Self::pruned`] is
/// externalized before publication, so only small bodies stay inline in the file. The manifest
/// below is the only place an externalized body is named, and the live owner keeps the complete
/// state in memory throughout.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadCheckpoint {
    pub schema_version: u32,
    pub thread_id: String,
    pub state_revision: u64,
    pub history_fence: u64,
    pub saved_at: i64,
    /// Bodies that left `state` for the session blob store, in externalization order.
    ///
    /// The current schema only names bodies that still have a resident slot. A schema-1 checkpoint
    /// written before externalization existed has no such field and keeps every body inline; a
    /// schema-2 checkpoint could still name an attempt's frozen input or produced step. Both old
    /// versions are converted one-way through [`ThreadCheckpoint::decode_legacy`]. A blob-owning
    /// caller materializes the current-context bodies first through [`LegacyThreadCheckpoint`]; a
    /// manifest that still names one is refused rather than silently dropping the body text, while
    /// an attempt body is dropped because the committed effect batch is its authoritative copy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_bodies: Vec<CheckpointExternalBody>,
    pub state: ThreadSnapshot,
}

impl ThreadCheckpoint {
    /// Current checkpoint schema. Schema 4 adds typed host facts, message purpose and usage provenance.
    /// The lightweight resident attempt shape introduced by schema 3 still excludes the
    /// frozen input context and the complete attempt outcome are no longer part of a checkpoint,
    /// because the matching [`super::ThreadEffectBatch`] is their only durable copy.
    pub const SCHEMA_VERSION: u32 = 4;

    /// Schema versions this build no longer interprets in its normal path, but can still convert
    /// through [`Self::decode_legacy`].
    ///
    /// Schema 1 inlines every body; schema 2 may name external bodies instead of inlining them.
    pub const LEGACY_SCHEMA_VERSIONS: [u32; 3] = [1, 2, 3];

    /// Whether this build interprets `schema_version` without loss.
    pub fn supports_schema(schema_version: u32) -> bool {
        schema_version == Self::SCHEMA_VERSION
    }

    /// Whether this build can still convert `schema_version` through the explicit legacy path.
    pub fn is_legacy_schema(schema_version: u32) -> bool {
        Self::LEGACY_SCHEMA_VERSIONS.contains(&schema_version)
    }

    /// Converts a schema-1/2 checkpoint envelope into the current schema-4 resident form.
    ///
    /// The conversion is one-way and explicit: the normal reader ([`Self::supports_schema`]) rejects
    /// the old versions so it can never mistake an unread layout for the current one. The caller is
    /// the owning storage, which must have already materialized every body it still needs: a legacy
    /// checkpoint whose `externalBodies` still names a *current-context* body is refused here rather
    /// than silently dropping the slot-to-body mapping, because the exact bytes belong to a blob
    /// store this crate does not own. A blob-owning caller materializes first through
    /// [`LegacyThreadCheckpoint`] and then converts. The complete frozen request snapshots a legacy
    /// checkpoint carried are dropped on purpose — the committed effect batch is their authoritative
    /// copy and schema 3 no longer has a resident slot for them.
    ///
    /// # Errors
    /// Rejects a non-legacy version, a non-empty current-context manifest, and malformed JSON.
    pub fn decode_legacy(envelope: &str) -> Result<Self, CheckpointLegacyError> {
        LegacyThreadCheckpoint::decode_json(envelope)?.into_current()
    }

    /// Captures the restart DTO: current facts only, with history referenced by its fence.
    ///
    /// This is the current recovery DTO, so it is the pruned form of
    /// [`Self::capture_transfer`].
    pub fn capture(thread_id: String, history_fence: u64, state: ThreadSnapshot) -> Self {
        Self::capture_transfer(thread_id, history_fence, state).pruned()
    }

    /// Captures the state paired with one committed effect for its history and call writers.
    ///
    /// Unlike the restart DTO this keeps the facts the matching effect still references — a
    /// just-consumed input, a just-finished Turn or attempt, a just-delivered result — so a writer
    /// projects the effect from the state that commit produced instead of reading back a pruned
    /// checkpoint. Only the current commit's facts are retained, so the write queue stays bounded.
    /// Callers persist [`Self::pruned`] in their atomic checkpoint transaction.
    pub fn capture_transfer(
        thread_id: String,
        history_fence: u64,
        mut state: ThreadSnapshot,
    ) -> Self {
        state.clear_ephemeral();
        Self {
            schema_version: Self::SCHEMA_VERSION,
            state_revision: state.commit_sequence,
            thread_id,
            history_fence,
            saved_at: crate::time::unix_seconds(),
            external_bodies: Vec::new(),
            state,
        }
    }

    /// Returns the restart DTO: terminal facts and exported history dropped.
    pub fn pruned(&self) -> Self {
        let mut pruned = self.clone();
        pruned.state.clear_ephemeral();
        pruned.state.retain_live_facts();
        pruned
    }

    /// The first body reference that still has to be materialized, in manifest order.
    ///
    /// A loader resolves references one at a time so it can read each blob, verify it and refill
    /// the slot before it ever hands the checkpoint to an owner.
    pub fn pending_body(&self) -> Option<&CheckpointExternalBody> {
        self.external_bodies.first()
    }

    /// Whether every referenced body has been materialized back into `state`.
    pub fn is_materialized(&self) -> bool {
        self.external_bodies.is_empty()
    }

    /// Replaces oversized bodies with versioned blob references and returns the exact bytes the
    /// publisher must make durable before it may write a checkpoint that names them.
    ///
    /// Every payload-bearing body a saved state can carry is covered: current-context content and
    /// tool-call arguments, pending inputs and inbox messages, pending or resolved interactions and
    /// the mutations they propose, live permission prompts and decisions, undelivered tool results,
    /// runtime facts, application records, tool declarations and the current private context. A
    /// resident attempt keeps only its identity, `input_revision`, status and usage, so it names no
    /// body at all: an attempt's frozen input, tool plan and produced step are the matching
    /// [`super::ThreadEffectBatch`]'s copy. No field keeps an unbounded body just because it is
    /// nested.
    ///
    /// This is a pure transformation: the receiver keeps its complete bodies, so the live owner and
    /// the next model request are unaffected. Only bodies above `threshold` leave the file; a small
    /// checkpoint stays inline and readable, and a body is never copied into a second field — it is
    /// named once, by [`Self::external_bodies`].
    pub fn externalize_bodies(&self, threshold: usize) -> (Self, Vec<ExtractedCheckpointBody>) {
        let mut extracted = Vec::new();
        let mut externalized = self.clone();
        externalized.schema_version = Self::SCHEMA_VERSION;

        let records = externalize_records(
            &externalized.state.context.records,
            threshold,
            &mut extracted,
        );
        externalized.state.context.records = records.into();

        let deliveries = externalized
            .state
            .deliveries
            .iter()
            .map(|delivery| externalize_delivery(delivery, threshold, &mut extracted))
            .collect::<Vec<_>>();
        externalized.state.deliveries = deliveries.into();

        let facts = externalized
            .state
            .runtime_facts
            .iter()
            .map(|fact| externalize_fact(fact, threshold, &mut extracted))
            .collect::<Vec<_>>();
        externalized.state.runtime_facts = facts.into();

        for (id, record) in externalized.state.extensions.iter_mut() {
            record.payload = externalize_payload(
                &record.payload,
                CheckpointBodySlot::ExtensionPayload { id: id.clone() },
                threshold,
                &mut extracted,
            );
        }

        let inputs = externalized
            .state
            .inputs
            .iter()
            .map(|record| externalize_input(record, threshold, &mut extracted))
            .collect::<Vec<_>>();
        externalized.state.inputs = inputs.into();

        let inbox = externalized
            .state
            .inbox
            .iter()
            .map(|record| externalize_inbox(record, threshold, &mut extracted))
            .collect::<Vec<_>>();
        externalized.state.inbox = inbox.into();

        let interactions = externalized
            .state
            .interactions
            .iter()
            .map(|(id, record)| {
                (
                    id.clone(),
                    externalize_interaction(id, record, threshold, &mut extracted),
                )
            })
            .collect::<BTreeMap<_, _>>();
        externalized.state.interactions = interactions;

        let permissions = externalized
            .state
            .permissions
            .iter()
            .map(|(id, record)| {
                (
                    id.clone(),
                    externalize_permission(id, record, threshold, &mut extracted),
                )
            })
            .collect::<BTreeMap<_, _>>();
        externalized.state.permissions = permissions;

        let discovered = externalized
            .state
            .discovered_tools
            .iter()
            .enumerate()
            .map(|(tool_index, tool)| {
                externalize_tool(
                    tool,
                    CheckpointBodySlot::ToolDeclaration { tool_index },
                    threshold,
                    &mut extracted,
                )
            })
            .collect::<Vec<_>>();
        externalized.state.discovered_tools = discovered.into();

        if let Some(payload) = externalized.state.private_context.take() {
            externalized.state.private_context = Some(externalize_payload(
                &payload,
                CheckpointBodySlot::PrivateContext,
                threshold,
                &mut extracted,
            ));
        }

        // References the receiver already carried stay pending: the slots they cover are already
        // placeholders, so nothing is extracted twice and re-externalizing never drops a reference.
        let mut manifest = self.external_bodies.clone();
        manifest.extend(extracted.iter().map(|body| body.entry.clone()));
        externalized.external_bodies = manifest;
        (externalized, extracted)
    }

    /// Restores one referenced body from the exact bytes its owner read for `reference`.
    ///
    /// # Errors
    /// Fails closed when the bytes do not match the recorded digest and length, when the reference
    /// is not pending, or when the saved slot does not hold the placeholder the manifest implies.
    /// The entry is removed only after the slot was refilled, so a retry after a transient read
    /// failure still sees the same pending work.
    pub fn materialize_body(
        &mut self,
        reference: &CheckpointBodyReference,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        reference.verify(bytes)?;
        let position = self
            .external_bodies
            .iter()
            .position(|entry| entry.reference == *reference)
            .ok_or_else(|| CheckpointBodyError::UnknownReference(reference.digest.clone()))?;
        let entry = self.external_bodies[position].clone();
        self.restore_body(&entry, bytes)?;
        self.external_bodies.remove(position);
        Ok(())
    }

    /// Refills the exact slot one manifest entry names.
    fn restore_body(
        &mut self,
        entry: &CheckpointExternalBody,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        match &entry.slot {
            CheckpointBodySlot::ContextContent {
                record_id,
                content_index,
            } => {
                let mut records = self
                    .state
                    .context
                    .records
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>();
                let record = records
                    .iter_mut()
                    .find(|record| &record.id == record_id)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(record_id.clone()))?;
                let slot = record
                    .content
                    .get_mut(*content_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(record_id.clone()))?;
                if !content_is_placeholder(slot, &entry.body) {
                    return Err(CheckpointBodyError::SlotMismatch(record_id.clone()));
                }
                *slot = content_from_body(&entry.body, bytes)?;
                self.state.context.records = records.into();
                Ok(())
            }
            CheckpointBodySlot::ContextToolCall {
                record_id,
                call_index,
            } => self.restore_record_call(record_id, *call_index, entry, bytes),
            CheckpointBodySlot::InputPayload { ordinal }
            | CheckpointBodySlot::InputContent { ordinal, .. } => {
                self.restore_input(*ordinal, entry, bytes)
            }
            CheckpointBodySlot::InboxPayload { sequence }
            | CheckpointBodySlot::InboxContent { sequence, .. } => {
                self.restore_inbox(*sequence, entry, bytes)
            }
            CheckpointBodySlot::InteractionRequest { id }
            | CheckpointBodySlot::InteractionResponsePayload { id }
            | CheckpointBodySlot::InteractionResponseContent { id, .. }
            | CheckpointBodySlot::InteractionMutationPayload { id, .. } => {
                self.restore_interaction(id, entry, bytes)
            }
            CheckpointBodySlot::PermissionPayload { id }
            | CheckpointBodySlot::PermissionResponse { id } => {
                self.restore_permission(id, entry, bytes)
            }
            CheckpointBodySlot::ToolDeclaration { tool_index } => {
                self.restore_discovered_tool(*tool_index, entry, bytes)
            }
            CheckpointBodySlot::DeliveryPayload { call_id } => {
                let body = DeliveryBody::Payload(payload_from_body(&entry.body, bytes)?);
                self.apply_delivery_body(call_id, &entry.body, body)
            }
            CheckpointBodySlot::DeliveryOutputContext {
                call_id,
                content_index,
            } => self.apply_delivery_body(
                call_id,
                &entry.body,
                DeliveryBody::OutputContext {
                    content_index: *content_index,
                    content: content_from_body(&entry.body, bytes)?,
                },
            ),
            CheckpointBodySlot::DeliveryDeliveredContext {
                call_id,
                content_index,
            } => self.apply_delivery_body(
                call_id,
                &entry.body,
                DeliveryBody::DeliveredContext {
                    content_index: *content_index,
                    content: content_from_body(&entry.body, bytes)?,
                },
            ),
            CheckpointBodySlot::DeliveryInteraction { call_id } => {
                let body = DeliveryBody::Interaction(payload_from_body(&entry.body, bytes)?);
                self.apply_delivery_body(call_id, &entry.body, body)
            }
            CheckpointBodySlot::ExtensionPayload { id } => {
                let record = self
                    .state
                    .extensions
                    .get_mut(id)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(id.clone()))?;
                if !record.payload.content().is_empty()
                    || !opaque_matches_kind(&record.payload, &entry.body)
                {
                    return Err(CheckpointBodyError::SlotMismatch(id.clone()));
                }
                record.payload = payload_from_body(&entry.body, bytes)?;
                Ok(())
            }
            CheckpointBodySlot::RuntimeFactContent {
                source_id,
                content_index,
            } => {
                let mut facts = self.state.runtime_facts.iter().cloned().collect::<Vec<_>>();
                let fact = facts
                    .iter_mut()
                    .find(|fact| &fact.source_id == source_id)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(source_id.clone()))?;
                let slot = fact
                    .content
                    .get_mut(*content_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(source_id.clone()))?;
                if !content_is_placeholder(slot, &entry.body) {
                    return Err(CheckpointBodyError::SlotMismatch(source_id.clone()));
                }
                *slot = content_from_body(&entry.body, bytes)?;
                self.state.runtime_facts = facts.into();
                Ok(())
            }
            CheckpointBodySlot::PrivateContext => {
                let Some(payload) = self.state.private_context.as_ref() else {
                    return Err(CheckpointBodyError::SlotMismatch(
                        "privateContext".to_owned(),
                    ));
                };
                if !payload.content().is_empty() || !opaque_matches_kind(payload, &entry.body) {
                    return Err(CheckpointBodyError::SlotMismatch(
                        "privateContext".to_owned(),
                    ));
                }
                let restored = payload_from_body(&entry.body, bytes)?;
                self.state.private_context = Some(restored);
                Ok(())
            }
        }
    }

    /// Refills one body of the pending delivery with `call_id`.
    ///
    /// A tool output keeps its producer payload, model projection and interaction request in fields
    /// only its own constructors write, so a refilled body rebuilds the output from the parts it
    /// still carries instead of mutating a field in place. Every other frozen field is preserved.
    fn apply_delivery_body(
        &mut self,
        call_id: &str,
        kind: &CheckpointBodyKind,
        body: DeliveryBody,
    ) -> Result<(), CheckpointBodyError> {
        let mut deliveries = self.state.deliveries.iter().cloned().collect::<Vec<_>>();
        let delivery = deliveries
            .iter_mut()
            .find(|delivery| delivery.call_id == call_id)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(call_id.to_owned()))?;
        let output = delivery.output.clone();
        match body {
            DeliveryBody::Payload(payload) => {
                if !output.payload().content().is_empty()
                    || !opaque_matches_kind(output.payload(), kind)
                {
                    return Err(CheckpointBodyError::SlotMismatch(call_id.to_owned()));
                }
                delivery.output = rebuild_tool_output(payload, output.context().to_vec(), &output)?;
            }
            DeliveryBody::OutputContext {
                content_index,
                content,
            } => {
                let mut context = output.context().to_vec();
                let slot = context
                    .get_mut(content_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(call_id.to_owned()))?;
                if !content_is_placeholder(slot, kind) {
                    return Err(CheckpointBodyError::SlotMismatch(call_id.to_owned()));
                }
                *slot = content;
                delivery.output = rebuild_tool_output(output.payload().clone(), context, &output)?;
            }
            DeliveryBody::DeliveredContext {
                content_index,
                content,
            } => {
                let slot = delivery
                    .delivered_context
                    .get_mut(content_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(call_id.to_owned()))?;
                if !content_is_placeholder(slot, kind) {
                    return Err(CheckpointBodyError::SlotMismatch(call_id.to_owned()));
                }
                *slot = content;
            }
            DeliveryBody::Interaction(payload) => {
                let interaction = output
                    .interaction()
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(call_id.to_owned()))?;
                if !interaction.content().is_empty() || !opaque_matches_kind(interaction, kind) {
                    return Err(CheckpointBodyError::SlotMismatch(call_id.to_owned()));
                }
                let mut rebuilt = rebuild_tool_output(
                    output.payload().clone(),
                    output.context().to_vec(),
                    &output,
                )?;
                rebuilt = rebuilt.with_interaction(payload);
                delivery.output = rebuilt;
            }
        }
        self.state.deliveries = deliveries.into();
        Ok(())
    }

    /// Refills one argument body of a tool call in the current context.
    fn restore_record_call(
        &mut self,
        record_id: &str,
        call_index: usize,
        entry: &CheckpointExternalBody,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        let mut records = self
            .state
            .context
            .records
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let record = records
            .iter_mut()
            .find(|record| record.id == record_id)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        let call = record
            .tool_calls
            .get_mut(call_index)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        let restored = restore_payload(&entry.body, bytes, &call.arguments, &entry.slot)?;
        call.arguments = restored;
        self.state.context.records = records.into();
        Ok(())
    }

    /// Refills one body of the unconsumed input with `ordinal`.
    fn restore_input(
        &mut self,
        ordinal: u64,
        entry: &CheckpointExternalBody,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        let mut inputs = self.state.inputs.iter().cloned().collect::<Vec<_>>();
        let input = inputs
            .iter_mut()
            .find(|record| record.ordinal == ordinal)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        match &entry.slot {
            CheckpointBodySlot::InputPayload { .. } => {
                let restored =
                    restore_payload(&entry.body, bytes, &input.input.payload, &entry.slot)?;
                input.input.payload = restored;
            }
            CheckpointBodySlot::InputContent { content_index, .. } => {
                let slot =
                    input.input.context.get_mut(*content_index).ok_or_else(|| {
                        CheckpointBodyError::SlotMismatch(slot_label(&entry.slot))
                    })?;
                let restored = restore_content(&entry.body, bytes, slot, &entry.slot)?;
                *slot = restored;
            }
            // This helper is dispatched only for its own slot family.
            _ => return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot))),
        }
        self.state.inputs = inputs.into();
        Ok(())
    }

    /// Refills one body of the unconsumed inbox message with `sequence`.
    fn restore_inbox(
        &mut self,
        sequence: u64,
        entry: &CheckpointExternalBody,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        let mut inbox = self.state.inbox.iter().cloned().collect::<Vec<_>>();
        let record = inbox
            .iter_mut()
            .find(|record| record.sequence == sequence)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        match &entry.slot {
            CheckpointBodySlot::InboxPayload { .. } => {
                let restored =
                    restore_payload(&entry.body, bytes, &record.message.payload, &entry.slot)?;
                record.message.payload = restored;
            }
            CheckpointBodySlot::InboxContent { content_index, .. } => {
                let slot = record
                    .message
                    .context
                    .get_mut(*content_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_content(&entry.body, bytes, slot, &entry.slot)?;
                *slot = restored;
            }
            _ => return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot))),
        }
        self.state.inbox = inbox.into();
        Ok(())
    }

    /// Refills one body of the interaction with `id`.
    fn restore_interaction(
        &mut self,
        id: &str,
        entry: &CheckpointExternalBody,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        let record = self
            .state
            .interactions
            .get_mut(id)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        match &entry.slot {
            CheckpointBodySlot::InteractionRequest { .. } => {
                let restored =
                    restore_payload(&entry.body, bytes, &record.request.payload, &entry.slot)?;
                record.request.payload = restored;
            }
            CheckpointBodySlot::InteractionMutationPayload { mutation_index, .. } => {
                let mutation = record
                    .extension_mutations
                    .get_mut(*mutation_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let ExtensionMutation::Put { payload, .. } = mutation else {
                    return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)));
                };
                let restored = restore_payload(&entry.body, bytes, payload, &entry.slot)?;
                *payload = restored;
            }
            CheckpointBodySlot::InteractionResponsePayload { .. } => {
                let InteractionState::Resolved(response) = &mut record.state else {
                    return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)));
                };
                let restored = restore_payload(&entry.body, bytes, &response.payload, &entry.slot)?;
                response.payload = restored;
            }
            CheckpointBodySlot::InteractionResponseContent { content_index, .. } => {
                let InteractionState::Resolved(response) = &mut record.state else {
                    return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)));
                };
                let slot = response
                    .context
                    .get_mut(*content_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_content(&entry.body, bytes, slot, &entry.slot)?;
                *slot = restored;
            }
            _ => return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot))),
        }
        Ok(())
    }

    /// Refills one body of the live permission record with `id`.
    fn restore_permission(
        &mut self,
        id: &str,
        entry: &CheckpointExternalBody,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        let record = self
            .state
            .permissions
            .get_mut(id)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        match &entry.slot {
            CheckpointBodySlot::PermissionPayload { .. } => {
                let restored = restore_payload(&entry.body, bytes, &record.payload, &entry.slot)?;
                record.payload = restored;
            }
            CheckpointBodySlot::PermissionResponse { .. } => {
                let response = record
                    .response
                    .as_mut()
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_payload(&entry.body, bytes, response, &entry.slot)?;
                *response = restored;
            }
            _ => return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot))),
        }
        Ok(())
    }

    /// Refills the declaration of the discovered tool at `tool_index`.
    fn restore_discovered_tool(
        &mut self,
        tool_index: usize,
        entry: &CheckpointExternalBody,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        if !matches!(entry.slot, CheckpointBodySlot::ToolDeclaration { .. }) {
            return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)));
        }
        let mut tools = self
            .state
            .discovered_tools
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let tool = tools
            .get_mut(tool_index)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        let restored = restore_payload(&entry.body, bytes, &tool.declaration, &entry.slot)?;
        tool.declaration = restored;
        self.state.discovered_tools = tools.into();
        Ok(())
    }

    /// Validates identity and consistency, then returns the persisted state and its restart
    /// settlement.
    ///
    /// Activating a Thread is an explicit host operation, so resuming always produces a live owner:
    /// a checkpoint captured while the previous owner was closing is settled back into execution
    /// instead of leaving the Thread permanently released. Running work still settles into
    /// interrupted facts before the new owner publishes.
    pub(crate) fn into_states(
        self,
        expected_thread_id: &str,
    ) -> Result<(ThreadSnapshot, ThreadSnapshot), ThreadError> {
        if !Self::supports_schema(self.schema_version)
            || !self.external_bodies.is_empty()
            || self.thread_id.is_empty()
            || self.thread_id != expected_thread_id
            || self.state_revision != self.state.commit_sequence
            || self.history_fence > self.state_revision
            || self.state.usage_summary.applied_sequence > self.state_revision
        {
            return Err(ThreadError::InvalidIdentity);
        }
        let published = self.state;
        let state = recovery::settle(published.clone())?;
        Ok((published, state))
    }
}

/// Replaces the oversized bodies of one record list with references.
fn externalize_records(
    records: &[ContextRecord],
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> Vec<ContextRecord> {
    records
        .iter()
        .map(|record| externalize_record(record, threshold, extracted))
        .collect()
}

/// Replaces one record's oversized content and tool-call arguments with placeholders.
fn externalize_record(
    record: &ContextRecord,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> ContextRecord {
    let mut externalized = record.clone();
    let mut content = Vec::with_capacity(record.content.len());
    for (content_index, item) in record.content.iter().enumerate() {
        let slot = CheckpointBodySlot::ContextContent {
            record_id: record.id.clone(),
            content_index,
        };
        content.push(externalize_content(item, slot, threshold, extracted));
    }
    let mut calls = Vec::with_capacity(record.tool_calls.len());
    for (call_index, call) in record.tool_calls.iter().enumerate() {
        let slot = CheckpointBodySlot::ContextToolCall {
            record_id: record.id.clone(),
            call_index,
        };
        let mut rebuilt = call.clone();
        rebuilt.arguments = externalize_payload(&call.arguments, slot, threshold, extracted);
        calls.push(rebuilt);
    }
    externalized.content = content;
    externalized.tool_calls = calls;
    externalized
}

/// Replaces one current runtime fact's oversized content with references.
fn externalize_fact(
    fact: &RuntimeFact,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> RuntimeFact {
    let mut externalized = fact.clone();
    let content = externalized
        .content
        .iter()
        .enumerate()
        .map(|(content_index, content)| {
            externalize_content(
                content,
                CheckpointBodySlot::RuntimeFactContent {
                    source_id: fact.source_id.clone(),
                    content_index,
                },
                threshold,
                extracted,
            )
        })
        .collect::<Vec<_>>();
    externalized.content = content;
    externalized
}

/// Replaces one unconsumed input's oversized bodies with references.
fn externalize_input(
    record: &InputRecord,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> InputRecord {
    let mut externalized = record.clone();
    let ordinal = record.ordinal;
    externalized.input.payload = externalize_payload(
        &record.input.payload,
        CheckpointBodySlot::InputPayload { ordinal },
        threshold,
        extracted,
    );
    let mut context = Vec::with_capacity(record.input.context.len());
    for (content_index, item) in record.input.context.iter().enumerate() {
        context.push(externalize_content(
            item,
            CheckpointBodySlot::InputContent {
                ordinal,
                content_index,
            },
            threshold,
            extracted,
        ));
    }
    externalized.input.context = context;
    externalized
}

/// Replaces one unconsumed inbox message's oversized bodies with references.
fn externalize_inbox(
    record: &InboxRecord,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> InboxRecord {
    let mut externalized = record.clone();
    let sequence = record.sequence;
    externalized.message.payload = externalize_payload(
        &record.message.payload,
        CheckpointBodySlot::InboxPayload { sequence },
        threshold,
        extracted,
    );
    let mut context = Vec::with_capacity(record.message.context.len());
    for (content_index, item) in record.message.context.iter().enumerate() {
        context.push(externalize_content(
            item,
            CheckpointBodySlot::InboxContent {
                sequence,
                content_index,
            },
            threshold,
            extracted,
        ));
    }
    externalized.message.context = context;
    externalized
}

/// Replaces one interaction's oversized request, response and mutation bodies with references.
fn externalize_interaction(
    id: &str,
    record: &InteractionRecord,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> InteractionRecord {
    let mut externalized = record.clone();
    externalized.request.payload = externalize_payload(
        &record.request.payload,
        CheckpointBodySlot::InteractionRequest { id: id.to_owned() },
        threshold,
        extracted,
    );
    for (mutation_index, mutation) in externalized.extension_mutations.iter_mut().enumerate() {
        if let ExtensionMutation::Put { payload, .. } = mutation {
            let external = externalize_payload(
                payload,
                CheckpointBodySlot::InteractionMutationPayload {
                    id: id.to_owned(),
                    mutation_index,
                },
                threshold,
                extracted,
            );
            *payload = external;
        }
    }
    if let InteractionState::Resolved(response) = &record.state {
        let payload = externalize_payload(
            &response.payload,
            CheckpointBodySlot::InteractionResponsePayload { id: id.to_owned() },
            threshold,
            extracted,
        );
        let mut context = Vec::with_capacity(response.context.len());
        for (content_index, item) in response.context.iter().enumerate() {
            context.push(externalize_content(
                item,
                CheckpointBodySlot::InteractionResponseContent {
                    id: id.to_owned(),
                    content_index,
                },
                threshold,
                extracted,
            ));
        }
        externalized.state = InteractionState::Resolved(InteractionResponse { payload, context });
    }
    externalized
}

/// Replaces one live permission record's oversized prompt and decision bodies with references.
fn externalize_permission(
    id: &str,
    record: &PermissionRecord,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> PermissionRecord {
    let mut externalized = record.clone();
    externalized.payload = externalize_payload(
        &record.payload,
        CheckpointBodySlot::PermissionPayload { id: id.to_owned() },
        threshold,
        extracted,
    );
    if let Some(response) = &record.response {
        externalized.response = Some(externalize_payload(
            response,
            CheckpointBodySlot::PermissionResponse { id: id.to_owned() },
            threshold,
            extracted,
        ));
    }
    externalized
}

/// Replaces one tool declaration's oversized body with a reference.
fn externalize_tool(
    tool: &ModelToolDeclaration,
    slot: CheckpointBodySlot,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> ModelToolDeclaration {
    ModelToolDeclaration {
        tool_id: tool.tool_id.clone(),
        declaration: externalize_payload(&tool.declaration, slot, threshold, extracted),
    }
}

/// Rebuilds one pending delivery with its oversized bodies replaced by references.
fn externalize_delivery(
    delivery: &ToolDelivery,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> ToolDelivery {
    let output = delivery.output.clone();
    let payload = externalize_payload(
        output.payload(),
        CheckpointBodySlot::DeliveryPayload {
            call_id: delivery.call_id.clone(),
        },
        threshold,
        extracted,
    );
    let context = output
        .context()
        .iter()
        .enumerate()
        .map(|(content_index, content)| {
            externalize_content(
                content,
                CheckpointBodySlot::DeliveryOutputContext {
                    call_id: delivery.call_id.clone(),
                    content_index,
                },
                threshold,
                extracted,
            )
        })
        .collect();
    // A frozen output only carries an interaction request while it awaits its host answer, so the
    // request is externalized exactly when the rebuild below will restore it.
    let interaction = match output.control() {
        ToolControl::AwaitInteraction => output.interaction().map(|payload| {
            externalize_payload(
                payload,
                CheckpointBodySlot::DeliveryInteraction {
                    call_id: delivery.call_id.clone(),
                },
                threshold,
                extracted,
            )
        }),
        ToolControl::Continue | ToolControl::EndTurn => None,
    };
    let delivered_context = delivery
        .delivered_context
        .iter()
        .enumerate()
        .map(|(content_index, content)| {
            externalize_content(
                content,
                CheckpointBodySlot::DeliveryDeliveredContext {
                    call_id: delivery.call_id.clone(),
                    content_index,
                },
                threshold,
                extracted,
            )
        })
        .collect();

    let mut rebuilt = ToolOutput::new(payload, context)
        .with_revealed_tools(output.revealed_tools().to_vec())
        .with_extension_mutations(output.extension_mutations().to_vec());
    match (output.control(), interaction) {
        (ToolControl::EndTurn, _) => rebuilt = rebuilt.ending_turn(),
        (ToolControl::AwaitInteraction, Some(interaction)) => {
            rebuilt = rebuilt.with_interaction(interaction);
        }
        // `Continue` keeps the default control: extraction above reads an interaction request only
        // for `AwaitInteraction`, so `Continue` never pairs with `Some`, and an `AwaitInteraction`
        // output without a request keeps that same default.
        (ToolControl::Continue, _) | (ToolControl::AwaitInteraction, None) => {}
    }

    ToolDelivery {
        target: delivery.target.clone(),
        call_id: delivery.call_id.clone(),
        tool_id: delivery.tool_id.clone(),
        output: rebuilt,
        delivered_context,
        outcome: delivery.outcome.clone(),
    }
}

/// Replaces one context content body when it is larger than the threshold.
fn externalize_content(
    content: &ContextContent,
    slot: CheckpointBodySlot,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> ContextContent {
    match content {
        ContextContent::Text { text } if text.len() > threshold => {
            let bytes = text.as_bytes().to_vec();
            record_body(CheckpointBodyKind::Text, slot, bytes, extracted, |_| {
                Some(ContextContent::Text {
                    text: Arc::from(""),
                })
            })
            .unwrap_or_else(|| content.clone())
        }
        ContextContent::Opaque { payload } if payload.byte_len() > threshold => {
            let bytes = payload.content().as_bytes().to_vec();
            let kind = CheckpointBodyKind::Opaque {
                format: payload.format().to_owned(),
                version: payload.version(),
            };
            let placeholder = OpaquePayload::new(payload.format(), payload.version(), "")
                .map(|payload| ContextContent::Opaque { payload })
                .ok();
            record_body(kind, slot, bytes, extracted, |_| placeholder)
                .unwrap_or_else(|| content.clone())
        }
        // A resource reference is already a stable identity its own owner resolves; the checkpoint
        // neither duplicates nor re-externalizes it.
        ContextContent::Text { .. }
        | ContextContent::Resource { .. }
        | ContextContent::Opaque { .. } => content.clone(),
    }
}

/// Replaces one opaque payload body when it is larger than the threshold.
fn externalize_payload(
    payload: &OpaquePayload,
    slot: CheckpointBodySlot,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> OpaquePayload {
    if payload.byte_len() <= threshold {
        return payload.clone();
    }
    let bytes = payload.content().as_bytes().to_vec();
    let kind = CheckpointBodyKind::Opaque {
        format: payload.format().to_owned(),
        version: payload.version(),
    };
    record_body(kind, slot, bytes, extracted, |_| {
        OpaquePayload::new(payload.format(), payload.version(), "").ok()
    })
    .unwrap_or_else(|| payload.clone())
}

/// Records one extracted body and returns the value that replaces it in the checkpoint.
///
/// Nothing is recorded when the replacement cannot be built: the body stays inline instead of
/// leaving a manifest entry with no matching placeholder.
fn record_body<T>(
    kind: CheckpointBodyKind,
    slot: CheckpointBodySlot,
    bytes: Vec<u8>,
    extracted: &mut Vec<ExtractedCheckpointBody>,
    placeholder: impl FnOnce(&[u8]) -> Option<T>,
) -> Option<T> {
    // A slot is unique inside one manifest: a second body claiming the same slot stays inline, so a
    // reference can never be restored into the wrong place.
    if extracted.iter().any(|body| body.entry.slot == slot) {
        return None;
    }
    let replacement = placeholder(&bytes)?;
    let reference = CheckpointBodyReference::of(&bytes);
    extracted.push(ExtractedCheckpointBody {
        entry: CheckpointExternalBody {
            slot,
            body: kind,
            reference,
        },
        bytes,
    });
    Some(replacement)
}

/// Whether `content` is exactly the placeholder the manifest entry implies.
fn content_is_placeholder(content: &ContextContent, kind: &CheckpointBodyKind) -> bool {
    match (content, kind) {
        (ContextContent::Text { text }, CheckpointBodyKind::Text) => text.is_empty(),
        (ContextContent::Opaque { payload }, _) => {
            payload.content().is_empty() && opaque_matches_kind(payload, kind)
        }
        (ContextContent::Text { .. }, _) | (ContextContent::Resource { .. }, _) => false,
    }
}

/// Whether an opaque placeholder still carries the format and version of its extracted body.
fn opaque_matches_kind(payload: &OpaquePayload, kind: &CheckpointBodyKind) -> bool {
    matches!(
        kind,
        CheckpointBodyKind::Opaque { format, version }
            if payload.format() == format.as_str() && payload.version() == *version
    )
}

/// Whether an opaque body is exactly the placeholder the manifest entry implies.
fn opaque_is_placeholder(payload: &OpaquePayload, kind: &CheckpointBodyKind) -> bool {
    payload.content().is_empty() && opaque_matches_kind(payload, kind)
}

/// Human-readable identity of the slot one manifest entry names.
fn slot_label(slot: &CheckpointBodySlot) -> String {
    format!("{slot:?}")
}

/// Restores one context content body into the slot the manifest names.
fn restore_content(
    kind: &CheckpointBodyKind,
    bytes: &[u8],
    current: &ContextContent,
    slot: &CheckpointBodySlot,
) -> Result<ContextContent, CheckpointBodyError> {
    if !content_is_placeholder(current, kind) {
        return Err(CheckpointBodyError::SlotMismatch(slot_label(slot)));
    }
    content_from_body(kind, bytes)
}

/// Restores one opaque payload body into the slot the manifest names.
fn restore_payload(
    kind: &CheckpointBodyKind,
    bytes: &[u8],
    current: &OpaquePayload,
    slot: &CheckpointBodySlot,
) -> Result<OpaquePayload, CheckpointBodyError> {
    if !opaque_is_placeholder(current, kind) {
        return Err(CheckpointBodyError::SlotMismatch(slot_label(slot)));
    }
    payload_from_body(kind, bytes)
}

/// Restores one context content body exactly as it was externalized.
fn content_from_body(
    kind: &CheckpointBodyKind,
    bytes: &[u8],
) -> Result<ContextContent, CheckpointBodyError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CheckpointBodyError::InvalidBody)?;
    Ok(match kind {
        CheckpointBodyKind::Text => ContextContent::Text {
            text: Arc::from(text),
        },
        CheckpointBodyKind::Opaque { .. } => ContextContent::Opaque {
            payload: payload_from_body(kind, bytes)?,
        },
    })
}

/// Restores one opaque payload body exactly as it was externalized.
fn payload_from_body(
    kind: &CheckpointBodyKind,
    bytes: &[u8],
) -> Result<OpaquePayload, CheckpointBodyError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CheckpointBodyError::InvalidBody)?;
    match kind {
        CheckpointBodyKind::Opaque { format, version } => {
            OpaquePayload::new(format.as_str(), *version, text)
                .map_err(|_| CheckpointBodyError::InvalidBody)
        }
        CheckpointBodyKind::Text => Err(CheckpointBodyError::InvalidBody),
    }
}

/// One body a loader refills into a pending tool delivery.
///
/// The delivery is rebuilt from the parts it still carries after the refill, so the loader names
/// which part it restored instead of mutating a field the tool output does not expose.
enum DeliveryBody {
    Payload(OpaquePayload),
    OutputContext {
        content_index: usize,
        content: ContextContent,
    },
    DeliveredContext {
        content_index: usize,
        content: ContextContent,
    },
    Interaction(OpaquePayload),
}

/// Rebuilds a tool output from replaced parts, preserving every other frozen field.
fn rebuild_tool_output(
    payload: OpaquePayload,
    context: Vec<ContextContent>,
    source: &ToolOutput,
) -> Result<ToolOutput, CheckpointBodyError> {
    let mut output = ToolOutput::new(payload, context)
        .with_revealed_tools(source.revealed_tools().to_vec())
        .with_extension_mutations(source.extension_mutations().to_vec());
    match source.control() {
        ToolControl::Continue => {}
        ToolControl::EndTurn => output = output.ending_turn(),
        ToolControl::AwaitInteraction => {
            let interaction = source
                .interaction()
                .cloned()
                .ok_or(CheckpointBodyError::InvalidBody)?;
            output = output.with_interaction(interaction);
        }
    }
    Ok(output)
}
