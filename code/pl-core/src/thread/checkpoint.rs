//! Direct restart state for one Thread owner.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::extensions::ExtensionMutation;
use super::inbox::InboxRecord;
use super::input::InputRecord;
use super::interactions::{InteractionRecord, InteractionResponse, InteractionState};
use super::permissions::PermissionRecord;
use super::{
    AttemptOutcome, RequestAttempt, RuntimeFact, ThreadError, ThreadSnapshot, ToolDelivery,
    recovery,
};
use crate::context::{ContextContent, ContextRecord, OpaquePayload};
use crate::error_record::chain_source;
use crate::model::{ModelError, ModelStepOutput, ModelToolDeclaration};
use crate::tool::{ToolControl, ToolOutput};

/// Bodies larger than this leave the checkpoint file and are named by a blob reference instead.
///
/// The threshold is checkpoint-owner policy, not a context limit: it keeps the once-per-second
/// `state.toml` replacement bounded while every normal record stays inline and directly readable.
/// It never bounds what the live owner holds — the in-memory current state always keeps the exact
/// bytes the next model request consumes — only how many of them are copied into the file.
pub const CHECKPOINT_BODY_THRESHOLD_BYTES: usize = 64 * 1024;

/// Body format of one externalized diagnostic source chain.
///
/// A chain is the portable text form a persisted `ModelError` keeps; it is encoded as a JSON string
/// array so it round-trips exactly, including embedded newlines and NUL characters.
const ERROR_SOURCE_FORMAT: &str = "pl.core.error-source";

/// Version of the diagnostic-chain body encoding above.
const ERROR_SOURCE_VERSION: u32 = 1;

/// Text a checkpoint writes in place of an externalized diagnostic chain.
const ERROR_SOURCE_PLACEHOLDER: &str = "[pure-lang checkpoint error-source reference]";

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
    /// A schema-1 checkpoint written before externalization existed has no such field and keeps
    /// every body inline; a loader treats the absent field as an empty manifest and reads the file
    /// exactly as before, so no already published checkpoint has to be rewritten first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_bodies: Vec<CheckpointExternalBody>,
    pub state: ThreadSnapshot,
}

impl ThreadCheckpoint {
    /// Current checkpoint schema. Schema 2 may name external bodies instead of inlining them.
    pub const SCHEMA_VERSION: u32 = 2;

    /// Inline-only schema written before bodies were externalized.
    ///
    /// It stays readable: every body is already in the file, so the loader only has to accept the
    /// version. A future schema stays unreadable on purpose, because a reader that does not know a
    /// newer layout cannot tell a real body from a reference.
    pub const LEGACY_SCHEMA_VERSION: u32 = 1;

    /// Whether this build interprets `schema_version` without loss.
    pub fn supports_schema(schema_version: u32) -> bool {
        matches!(
            schema_version,
            Self::SCHEMA_VERSION | Self::LEGACY_SCHEMA_VERSION
        )
    }

    /// Captures the restart DTO: current facts only, with history referenced by its fence.
    ///
    /// This is the value that is published as `state.toml`, so it is the pruned form of
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
    /// Callers must publish [`Self::pruned`] as `state.toml`.
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
    /// runtime facts, application records, tool declarations, and resident attempts together with
    /// their frozen model input and produced step. A failed or cancelled attempt also externalizes
    /// its provider-owned details and its recorded diagnostic chain text. No field keeps an
    /// unbounded body just because it is nested.
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
            None,
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

        let attempts = externalized
            .state
            .attempts
            .iter()
            .enumerate()
            .map(|(attempt_index, attempt)| {
                externalize_attempt(attempt_index, attempt, threshold, &mut extracted)
            })
            .collect::<Vec<_>>();
        externalized.state.attempts = attempts.into();

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
            CheckpointBodySlot::AttemptMetadata { attempt_index }
            | CheckpointBodySlot::AttemptToolProjection { attempt_index }
            | CheckpointBodySlot::AttemptToolDeclaration { attempt_index, .. }
            | CheckpointBodySlot::AttemptInputContent { attempt_index, .. }
            | CheckpointBodySlot::AttemptInputToolCall { attempt_index, .. }
            | CheckpointBodySlot::AttemptOutputContent { attempt_index, .. }
            | CheckpointBodySlot::AttemptOutputToolCall { attempt_index, .. }
            | CheckpointBodySlot::AttemptOutputPrivateContext { attempt_index } => {
                self.restore_attempt(*attempt_index, entry, bytes)
            }
            CheckpointBodySlot::AttemptErrorDetails { attempt_index } => {
                self.restore_attempt_error(*attempt_index, AttemptErrorBody::Details, entry, bytes)
            }
            CheckpointBodySlot::AttemptErrorSource { attempt_index } => {
                self.restore_attempt_error(*attempt_index, AttemptErrorBody::Source, entry, bytes)
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

    /// Refills one body of the resident attempt at `attempt_index`.
    fn restore_attempt(
        &mut self,
        attempt_index: usize,
        entry: &CheckpointExternalBody,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        let mut attempts = self.state.attempts.iter().cloned().collect::<Vec<_>>();
        let attempt = attempts
            .get_mut(attempt_index)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        match &entry.slot {
            CheckpointBodySlot::AttemptMetadata { .. } => {
                let metadata = attempt
                    .request_metadata
                    .as_mut()
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_payload(&entry.body, bytes, metadata, &entry.slot)?;
                *metadata = restored;
            }
            CheckpointBodySlot::AttemptToolProjection { .. } => {
                let projection = attempt
                    .tool_projection
                    .as_mut()
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_payload(&entry.body, bytes, projection, &entry.slot)?;
                *projection = restored;
            }
            CheckpointBodySlot::AttemptToolDeclaration { tool_index, .. } => {
                let mut tools = attempt.tools.to_vec();
                let tool = tools
                    .get_mut(*tool_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_payload(&entry.body, bytes, &tool.declaration, &entry.slot)?;
                tool.declaration = restored;
                attempt.tools = tools.into();
            }
            CheckpointBodySlot::AttemptInputContent {
                record_id,
                content_index,
                ..
            } => {
                let mut records = attempt.input.records.to_vec();
                let record = records
                    .iter_mut()
                    .find(|record| &record.id == record_id)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let slot = record
                    .content
                    .get_mut(*content_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_content(&entry.body, bytes, slot, &entry.slot)?;
                *slot = restored;
                attempt.input.records = records.into();
            }
            CheckpointBodySlot::AttemptInputToolCall {
                record_id,
                call_index,
                ..
            } => {
                let mut records = attempt.input.records.to_vec();
                let record = records
                    .iter_mut()
                    .find(|record| &record.id == record_id)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let call = record
                    .tool_calls
                    .get_mut(*call_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_payload(&entry.body, bytes, &call.arguments, &entry.slot)?;
                call.arguments = restored;
                attempt.input.records = records.into();
            }
            CheckpointBodySlot::AttemptOutputContent { content_index, .. } => {
                let output = step_output_mut(&mut attempt.outcome)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let slot = output
                    .content
                    .get_mut(*content_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_content(&entry.body, bytes, slot, &entry.slot)?;
                *slot = restored;
            }
            CheckpointBodySlot::AttemptOutputToolCall { call_index, .. } => {
                let output = step_output_mut(&mut attempt.outcome)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let call = output
                    .tool_calls
                    .get_mut(*call_index)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_payload(&entry.body, bytes, &call.arguments, &entry.slot)?;
                call.arguments = restored;
            }
            CheckpointBodySlot::AttemptOutputPrivateContext { .. } => {
                let output = step_output_mut(&mut attempt.outcome)
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let private = output
                    .private_context
                    .as_mut()
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_payload(&entry.body, bytes, private, &entry.slot)?;
                *private = restored;
            }
            _ => return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot))),
        }
        self.state.attempts = attempts.into();
        Ok(())
    }

    /// Refills one body of the failure outcome of the resident attempt at `attempt_index`.
    ///
    /// A failure is rebuilt rather than mutated in place: `ModelError` is not `Clone` because it owns
    /// the provider error object, so the copy is assembled from the details payload and the recorded
    /// chain text, which is exactly what the persisted form keeps.
    fn restore_attempt_error(
        &mut self,
        attempt_index: usize,
        body: AttemptErrorBody,
        entry: &CheckpointExternalBody,
        bytes: &[u8],
    ) -> Result<(), CheckpointBodyError> {
        let mut attempts = self.state.attempts.iter().cloned().collect::<Vec<_>>();
        let attempt = attempts
            .get_mut(attempt_index)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        let error = attempt_error_mut(&mut attempt.outcome)
            .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
        let current = error.clone();
        let rebuilt = match body {
            AttemptErrorBody::Details => {
                let details = current
                    .details
                    .as_deref()
                    .ok_or_else(|| CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)))?;
                let restored = restore_payload(&entry.body, bytes, details, &entry.slot)?;
                current.with_parts(
                    Some(Box::new(restored)),
                    chain_source(current.source_chain()),
                )
            }
            AttemptErrorBody::Source => {
                if current.source_chain() != vec![ERROR_SOURCE_PLACEHOLDER.to_owned()] {
                    return Err(CheckpointBodyError::SlotMismatch(slot_label(&entry.slot)));
                }
                let chain = decode_error_chain(&entry.body, bytes)?;
                current.with_parts(current.details.clone(), chain_source(chain))
            }
        };
        *error = Arc::new(rebuilt);
        self.state.attempts = attempts.into();
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
///
/// The same record list appears as the current context and, frozen, as a resident attempt's model
/// input; `attempt` selects which slot family the manifest uses so a body is never named twice.
fn externalize_records(
    records: &[ContextRecord],
    attempt: Option<usize>,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> Vec<ContextRecord> {
    records
        .iter()
        .map(|record| externalize_record(record, attempt, threshold, extracted))
        .collect()
}

/// Replaces one record's oversized content and tool-call arguments with placeholders.
fn externalize_record(
    record: &ContextRecord,
    attempt: Option<usize>,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> ContextRecord {
    let mut externalized = record.clone();
    let mut content = Vec::with_capacity(record.content.len());
    for (content_index, item) in record.content.iter().enumerate() {
        let slot = match attempt {
            Some(attempt_index) => CheckpointBodySlot::AttemptInputContent {
                attempt_index,
                record_id: record.id.clone(),
                content_index,
            },
            None => CheckpointBodySlot::ContextContent {
                record_id: record.id.clone(),
                content_index,
            },
        };
        content.push(externalize_content(item, slot, threshold, extracted));
    }
    let mut calls = Vec::with_capacity(record.tool_calls.len());
    for (call_index, call) in record.tool_calls.iter().enumerate() {
        let slot = match attempt {
            Some(attempt_index) => CheckpointBodySlot::AttemptInputToolCall {
                attempt_index,
                record_id: record.id.clone(),
                call_index,
            },
            None => CheckpointBodySlot::ContextToolCall {
                record_id: record.id.clone(),
                call_index,
            },
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

/// Replaces one resident attempt's oversized bodies with references.
fn externalize_attempt(
    attempt_index: usize,
    attempt: &RequestAttempt,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> RequestAttempt {
    let mut externalized = attempt.clone();
    if let Some(metadata) = &attempt.request_metadata {
        externalized.request_metadata = Some(externalize_payload(
            metadata,
            CheckpointBodySlot::AttemptMetadata { attempt_index },
            threshold,
            extracted,
        ));
    }
    if let Some(projection) = &attempt.tool_projection {
        externalized.tool_projection = Some(externalize_payload(
            projection,
            CheckpointBodySlot::AttemptToolProjection { attempt_index },
            threshold,
            extracted,
        ));
    }
    externalized.input.records = externalize_records(
        &attempt.input.records,
        Some(attempt_index),
        threshold,
        extracted,
    )
    .into();
    let mut tools = Vec::with_capacity(attempt.tools.len());
    for (tool_index, tool) in attempt.tools.iter().enumerate() {
        tools.push(externalize_tool(
            tool,
            CheckpointBodySlot::AttemptToolDeclaration {
                attempt_index,
                tool_index,
            },
            threshold,
            extracted,
        ));
    }
    externalized.tools = tools.into();
    externalized.outcome =
        externalize_outcome(attempt_index, &attempt.outcome, threshold, extracted);
    externalized
}

/// Replaces the oversized bodies of one attempt outcome with references.
fn externalize_outcome(
    attempt_index: usize,
    outcome: &AttemptOutcome,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> AttemptOutcome {
    match outcome {
        AttemptOutcome::Committed(output) => AttemptOutcome::Committed(externalize_step_output(
            attempt_index,
            output,
            threshold,
            extracted,
        )),
        AttemptOutcome::Cancelled { result } => AttemptOutcome::Cancelled {
            result: match result {
                Ok(output) => Ok(externalize_step_output(
                    attempt_index,
                    output,
                    threshold,
                    extracted,
                )),
                Err(error) => Err(Arc::new(externalize_error(
                    attempt_index,
                    error,
                    threshold,
                    extracted,
                ))),
            },
        },
        AttemptOutcome::Rejected { output, reason } => AttemptOutcome::Rejected {
            output: externalize_step_output(attempt_index, output, threshold, extracted),
            reason: reason.clone(),
        },
        // A failure keeps its provider-owned details and its diagnostic chain text; both can be as
        // large as any other body, so the same manifest covers them.
        AttemptOutcome::Failed(error) => AttemptOutcome::Failed(Arc::new(externalize_error(
            attempt_index,
            error,
            threshold,
            extracted,
        ))),
        AttemptOutcome::Running | AttemptOutcome::Interrupted => outcome.clone(),
    }
}

/// Replaces the oversized bodies of one produced step with references.
fn externalize_step_output(
    attempt_index: usize,
    output: &ModelStepOutput,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> ModelStepOutput {
    let mut externalized = output.clone();
    let mut content = Vec::with_capacity(output.content.len());
    for (content_index, item) in output.content.iter().enumerate() {
        content.push(externalize_content(
            item,
            CheckpointBodySlot::AttemptOutputContent {
                attempt_index,
                content_index,
            },
            threshold,
            extracted,
        ));
    }
    let mut calls = Vec::with_capacity(output.tool_calls.len());
    for (call_index, call) in output.tool_calls.iter().enumerate() {
        let mut rebuilt = call.clone();
        rebuilt.arguments = externalize_payload(
            &call.arguments,
            CheckpointBodySlot::AttemptOutputToolCall {
                attempt_index,
                call_index,
            },
            threshold,
            extracted,
        );
        calls.push(rebuilt);
    }
    externalized.content = content;
    externalized.tool_calls = calls;
    if let Some(private) = &output.private_context {
        externalized.private_context = Some(externalize_payload(
            private,
            CheckpointBodySlot::AttemptOutputPrivateContext { attempt_index },
            threshold,
            extracted,
        ));
    }
    externalized
}

/// Replaces the oversized bodies of one resident failure with references.
///
/// A `ModelError` owns a `Box<dyn Error>` and is deliberately not `Clone`, so the copy is rebuilt
/// from the two things the persisted form keeps: the provider-owned details payload and the
/// diagnostic source-chain text. That text is exactly what serialization records, so the rebuilt
/// failure persists and reports like the original while the live owner keeps the untouched value.
fn externalize_error(
    attempt_index: usize,
    error: &ModelError,
    threshold: usize,
    extracted: &mut Vec<ExtractedCheckpointBody>,
) -> ModelError {
    let details = error.details.as_deref().map(|payload| {
        Box::new(externalize_payload(
            payload,
            CheckpointBodySlot::AttemptErrorDetails { attempt_index },
            threshold,
            extracted,
        ))
    });
    let chain = error.source_chain();
    let source = if chain.is_empty() {
        None
    } else {
        let external = encode_error_chain(&chain)
            .filter(|encoded| encoded.len() > threshold)
            .and_then(|encoded| {
                record_body(
                    CheckpointBodyKind::Opaque {
                        format: ERROR_SOURCE_FORMAT.to_owned(),
                        version: ERROR_SOURCE_VERSION,
                    },
                    CheckpointBodySlot::AttemptErrorSource { attempt_index },
                    encoded,
                    extracted,
                    |_| chain_source(vec![ERROR_SOURCE_PLACEHOLDER.to_owned()]),
                )
            });
        external.or_else(|| chain_source(chain))
    };
    error.with_parts(details, source)
}

/// Encodes one diagnostic chain as the exact bytes a checkpoint body holds.
fn encode_error_chain(chain: &[String]) -> Option<Vec<u8>> {
    serde_json::to_vec(chain).ok()
}

/// Decodes one externalized diagnostic chain, rejecting any other body shape.
fn decode_error_chain(
    kind: &CheckpointBodyKind,
    bytes: &[u8],
) -> Result<Vec<String>, CheckpointBodyError> {
    let CheckpointBodyKind::Opaque { format, version } = kind else {
        return Err(CheckpointBodyError::InvalidBody);
    };
    if format != ERROR_SOURCE_FORMAT || *version != ERROR_SOURCE_VERSION {
        return Err(CheckpointBodyError::InvalidBody);
    }
    serde_json::from_slice(bytes).map_err(|_| CheckpointBodyError::InvalidBody)
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

/// The produced step of one attempt outcome, while that outcome still carries one.
fn step_output_mut(outcome: &mut AttemptOutcome) -> Option<&mut ModelStepOutput> {
    match outcome {
        AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => Some(output),
        AttemptOutcome::Cancelled { result } => result.as_mut().ok(),
        AttemptOutcome::Running | AttemptOutcome::Interrupted | AttemptOutcome::Failed(_) => None,
    }
}

/// The failure of one attempt outcome, while that outcome still carries one.
fn attempt_error_mut(outcome: &mut AttemptOutcome) -> Option<&mut Arc<ModelError>> {
    match outcome {
        AttemptOutcome::Failed(error) => Some(error),
        AttemptOutcome::Cancelled { result: Err(error) } => Some(error),
        _ => None,
    }
}

/// Which body of one resident failure a manifest entry refills.
#[derive(Debug, Clone, Copy)]
enum AttemptErrorBody {
    Details,
    Source,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ContextSnapshot, ContextSource};
    use crate::thread::{ToolDeliveryTarget, ToolOutcome};

    const THRESHOLD: usize = 64;

    fn body(label: &str, bytes: usize) -> String {
        format!("{label}:{}", "x".repeat(bytes))
    }

    fn assistant_record(body: &str) -> ContextRecord {
        ContextRecord {
            id: "record-1".to_owned(),
            turn_id: None,
            source: ContextSource::Assistant,
            content: vec![
                ContextContent::Text { text: body.into() },
                ContextContent::Opaque {
                    payload: OpaquePayload::new("pl.model.assistant", 2, body).unwrap(),
                },
            ],
            tool_calls: Vec::new(),
        }
    }

    fn restore(
        checkpoint: &ThreadCheckpoint,
        bodies: &[ExtractedCheckpointBody],
    ) -> ThreadCheckpoint {
        let mut restored = checkpoint.clone();
        while let Some(entry) = restored.pending_body().cloned() {
            let extracted = bodies
                .iter()
                .find(|body| body.entry.reference == entry.reference)
                .expect("every manifest entry has its extracted bytes");
            restored
                .materialize_body(&entry.reference, &extracted.bytes)
                .expect("a body restores from its own bytes");
        }
        restored
    }

    fn attempt_with_outcome(outcome: AttemptOutcome) -> RequestAttempt {
        RequestAttempt {
            request_metadata: None,
            tool_projection: None,
            turn_id: "turn-1".to_owned(),
            attempt_id: "attempt-1".to_owned(),
            retry_of: None,
            input: ContextSnapshot::default(),
            tools: Vec::new().into(),
            outcome,
            input_estimate: None,
        }
    }

    /// 大正文本体离开 state，只留下版本化引用；按引用物化后逐字段等于原状态。
    #[test]
    fn oversized_context_bodies_round_trip_exactly() {
        let body = body("assistant", THRESHOLD * 4);
        let state = ThreadSnapshot {
            commit_sequence: 1,
            context: ContextSnapshot {
                revision: 1,
                records: vec![assistant_record(&body)].into(),
            },
            ..Default::default()
        };
        let checkpoint = ThreadCheckpoint::capture("thread".to_owned(), 1, state);

        let (externalized, bodies) = checkpoint.externalize_bodies(THRESHOLD);
        assert_eq!(externalized.external_bodies.len(), 2);
        assert_eq!(bodies.len(), 2);
        assert_eq!(
            externalized.external_bodies[0].slot,
            CheckpointBodySlot::ContextContent {
                record_id: "record-1".to_owned(),
                content_index: 0,
            }
        );
        assert_eq!(
            externalized.external_bodies[0].body,
            CheckpointBodyKind::Text
        );
        assert!(matches!(
            externalized.external_bodies[1].body,
            CheckpointBodyKind::Opaque { .. }
        ));
        let serialized = serde_json::to_string(&externalized).unwrap();
        assert!(serialized.contains("externalBodies"));
        assert!(!serialized.contains(&body));

        let restored = restore(&externalized, &bodies);
        assert!(restored.is_materialized());
        assert_eq!(
            restored.state.context.records,
            checkpoint.state.context.records
        );
        assert_eq!(
            serde_json::to_string(&restored).unwrap(),
            serde_json::to_string(&checkpoint).unwrap()
        );
    }

    /// 当前应用记录、运行时事实与私有上下文同样按引用外置并精确还原。
    #[test]
    fn oversized_application_state_round_trips_exactly() {
        let payload = body("extension", THRESHOLD * 4);
        let fact = body("fact", THRESHOLD * 4);
        let private = body("private", THRESHOLD * 4);
        let state = ThreadSnapshot {
            commit_sequence: 1,
            extensions: std::collections::BTreeMap::from([(
                "app.record".to_owned(),
                crate::thread::extensions::ExtensionRecord {
                    revision: 3,
                    payload: OpaquePayload::new("app.record", 1, payload.as_str()).unwrap(),
                },
            )]),
            runtime_facts: vec![RuntimeFact {
                source_id: "source-1".to_owned(),
                content: vec![ContextContent::Opaque {
                    payload: OpaquePayload::new("runtime.fact", 1, fact.as_str()).unwrap(),
                }],
            }]
            .into(),
            private_context: Some(
                OpaquePayload::new("private.context", 1, private.as_str()).unwrap(),
            ),
            ..Default::default()
        };
        let checkpoint = ThreadCheckpoint::capture("thread".to_owned(), 1, state);

        let (externalized, bodies) = checkpoint.externalize_bodies(THRESHOLD);
        assert_eq!(externalized.external_bodies.len(), 3);
        let serialized = serde_json::to_string(&externalized).unwrap();
        for hidden in [&payload, &fact, &private] {
            assert!(!serialized.contains(hidden.as_str()));
        }

        let restored = restore(&externalized, &bodies);
        assert!(restored.is_materialized());
        assert_eq!(
            serde_json::to_string(&restored).unwrap(),
            serde_json::to_string(&checkpoint).unwrap()
        );
    }

    /// 未消费输入、待答复交互与活动权限的正文同样按引用外置并精确还原。
    #[test]
    fn pending_inputs_interactions_and_permissions_round_trip_exactly() {
        use crate::thread::extensions::ExtensionMutation;
        use crate::thread::inbox::{InboxRecord, ThreadMessage};
        use crate::thread::input::{InputDelivery, InputRecord, InputState, ThreadInput};
        use crate::thread::interactions::{
            InteractionRecord, InteractionRequest, InteractionResponse, InteractionState,
        };
        use crate::thread::permissions::{PermissionRecord, PermissionState};

        let body = body("pending", THRESHOLD * 4);
        let large = |format: &str, version: u32| {
            OpaquePayload::new(format, version, body.as_str()).unwrap()
        };
        let input = InputRecord {
            accepted_sequence: 4,
            delivery: InputDelivery::NextTurn,
            input: ThreadInput {
                id: "input-1".to_owned(),
                payload: large("pl.input", 1),
                context: vec![ContextContent::Text {
                    text: body.as_str().into(),
                }],
            },
            ordinal: 7,
            revision: 4,
            state: InputState::Pending,
        };
        let inbox = InboxRecord {
            sequence: 3,
            message: ThreadMessage {
                id: "message-1".to_owned(),
                source_id: "child".to_owned(),
                payload: large("pl.message", 1),
                context: vec![ContextContent::Opaque {
                    payload: large("pl.message.context", 1),
                }],
            },
        };
        let interaction = InteractionRecord {
            created_at: 1,
            updated_at: 2,
            continuation_id: None,
            request: InteractionRequest {
                id: "interaction-1".to_owned(),
                turn_id: "turn-1".to_owned(),
                payload: large("pl.interaction", 1),
            },
            revision: 2,
            state: InteractionState::Resolved(InteractionResponse {
                payload: large("pl.interaction.response", 1),
                context: vec![ContextContent::Text {
                    text: body.as_str().into(),
                }],
            }),
            extension_mutations: vec![ExtensionMutation::Put {
                id: "app.record".to_owned(),
                expected_revision: None,
                payload: large("app.record", 1),
            }],
        };
        let permission = PermissionRecord {
            created_at: 1,
            updated_at: 2,
            response: Some(large("pl.permission.response", 1)),
            id: "permission:call-1".to_owned(),
            task_id: "task-1".to_owned(),
            call_id: "call-1".to_owned(),
            turn_id: "turn-1".to_owned(),
            tool_id: "tool-1".to_owned(),
            revision: 1,
            payload: large("pl.permission", 1),
            state: PermissionState::Allowed,
        };
        let state = ThreadSnapshot {
            commit_sequence: 1,
            inputs: vec![input].into(),
            inbox: vec![inbox].into(),
            interactions: std::collections::BTreeMap::from([(
                "interaction-1".to_owned(),
                interaction,
            )]),
            permissions: std::collections::BTreeMap::from([(
                "permission:call-1".to_owned(),
                permission,
            )]),
            ..Default::default()
        };
        let checkpoint = ThreadCheckpoint::capture_transfer("thread".to_owned(), 1, state);

        let (externalized, bodies) = checkpoint.externalize_bodies(THRESHOLD);
        assert_eq!(externalized.external_bodies.len(), 10);
        let serialized = serde_json::to_string(&externalized).unwrap();
        assert!(!serialized.contains(&body));

        let restored = restore(&externalized, &bodies);
        assert!(restored.is_materialized());
        assert_eq!(
            serde_json::to_string(&restored).unwrap(),
            serde_json::to_string(&checkpoint).unwrap()
        );
    }

    /// 常驻 attempt 的冻结输入、工具声明与产生结果同样按引用外置并精确还原。
    #[test]
    fn resident_attempt_bodies_round_trip_exactly() {
        use crate::model::{ModelToolCall, ModelToolDeclaration, ModelUsage};
        use crate::thread::{AttemptOutcome, RequestAttempt};

        let body = body("attempt", THRESHOLD * 4);
        let output = ModelStepOutput {
            attempt_id: "attempt-1".to_owned(),
            base_context_revision: 1,
            content: vec![ContextContent::Text {
                text: body.as_str().into(),
            }],
            tool_calls: vec![ModelToolCall {
                call_id: "call-1".to_owned(),
                tool_id: "tool-1".to_owned(),
                arguments: OpaquePayload::new("pl.tool.call", 1, body.as_str()).unwrap(),
            }],
            private_context: Some(
                OpaquePayload::new("pl.model.private", 1, body.as_str()).unwrap(),
            ),
            usage: ModelUsage::default(),
        };
        let attempt = RequestAttempt {
            request_metadata: Some(
                OpaquePayload::new("pl.model.request", 1, body.as_str()).unwrap(),
            ),
            tool_projection: Some(OpaquePayload::new("pl.model.tools", 1, body.as_str()).unwrap()),
            turn_id: "turn-1".to_owned(),
            attempt_id: "attempt-1".to_owned(),
            retry_of: None,
            input: ContextSnapshot {
                revision: 1,
                records: vec![assistant_record(&body)].into(),
            },
            tools: vec![ModelToolDeclaration {
                tool_id: "tool-1".to_owned(),
                declaration: OpaquePayload::new("pl.tool.declaration", 1, body.as_str()).unwrap(),
            }]
            .into(),
            outcome: AttemptOutcome::Committed(output),
            input_estimate: None,
        };
        let state = ThreadSnapshot {
            commit_sequence: 1,
            discovered_tools: vec![ModelToolDeclaration {
                tool_id: "tool-2".to_owned(),
                declaration: OpaquePayload::new("pl.tool.declaration", 2, body.as_str()).unwrap(),
            }]
            .into(),
            attempts: vec![attempt].into(),
            ..Default::default()
        };
        let checkpoint = ThreadCheckpoint::capture_transfer("thread".to_owned(), 1, state);

        let (externalized, bodies) = checkpoint.externalize_bodies(THRESHOLD);
        assert_eq!(externalized.external_bodies.len(), 9);
        let serialized = serde_json::to_string(&externalized).unwrap();
        assert!(!serialized.contains(&body));

        let restored = restore(&externalized, &bodies);
        assert!(restored.is_materialized());
        assert_eq!(
            serde_json::to_string(&restored).unwrap(),
            serde_json::to_string(&checkpoint).unwrap()
        );
    }

    /// 失败与取消结果携带的诊断详情、错误链同样外置，并按原变体、格式与链文本精确还原。
    #[test]
    fn failed_and_cancelled_attempt_errors_round_trip_exactly() {
        use crate::model::{ModelFailureKind, ModelUsage};
        use crate::thread::AttemptOutcome;

        let failed_details = body("failed-details", THRESHOLD * 4);
        let failed_source = body("failed-source", THRESHOLD * 4);
        let cancelled_details = body("cancelled-details", THRESHOLD * 4);
        let cancelled_source = body("cancelled-source", THRESHOLD * 4);
        let failed_chain = vec![failed_source.clone(), "failed inner".to_owned()];
        let cancelled_chain = vec![cancelled_source.clone(), "cancelled inner".to_owned()];

        let failed = ModelError {
            details: Some(Box::new(
                OpaquePayload::new("pl.model.failure", 3, failed_details.as_str()).unwrap(),
            )),
            kind: ModelFailureKind::Unavailable,
            usage: ModelUsage {
                input_tokens: Some(11),
                output_tokens: Some(7),
                ..Default::default()
            },
            source: chain_source(failed_chain.clone()),
        };
        let cancelled = ModelError {
            details: Some(Box::new(
                OpaquePayload::new("pl.model.cancel", 5, cancelled_details.as_str()).unwrap(),
            )),
            kind: ModelFailureKind::Cancelled,
            usage: ModelUsage {
                input_tokens: Some(3),
                reasoning_tokens: Some(2),
                ..Default::default()
            },
            source: chain_source(cancelled_chain.clone()),
        };

        let state = ThreadSnapshot {
            commit_sequence: 1,
            attempts: vec![
                attempt_with_outcome(AttemptOutcome::Failed(Arc::new(failed))),
                attempt_with_outcome(AttemptOutcome::Cancelled {
                    result: Err(Arc::new(cancelled)),
                }),
            ]
            .into(),
            ..Default::default()
        };
        let checkpoint = ThreadCheckpoint::capture_transfer("thread".to_owned(), 1, state);

        let (externalized, bodies) = checkpoint.externalize_bodies(THRESHOLD);
        // The details and the diagnostic chain of both the failed and the cancelled attempt leave
        // the file: four independently referenced bodies.
        assert_eq!(externalized.external_bodies.len(), 4);
        let serialized = serde_json::to_string(&externalized).unwrap();
        for hidden in [
            &failed_details,
            &failed_source,
            &cancelled_details,
            &cancelled_source,
        ] {
            assert!(!serialized.contains(hidden.as_str()));
        }

        let restored = restore(&externalized, &bodies);
        assert!(restored.is_materialized());
        assert_eq!(
            serde_json::to_string(&restored).unwrap(),
            serde_json::to_string(&checkpoint).unwrap()
        );

        let failed = match &restored.state.attempts[0].outcome {
            AttemptOutcome::Failed(error) => error.clone(),
            other => panic!("failed attempt keeps its outcome variant: {other:?}"),
        };
        assert_eq!(failed.kind, ModelFailureKind::Unavailable);
        assert_eq!(failed.usage.input_tokens, Some(11));
        assert_eq!(failed.usage.output_tokens, Some(7));
        assert_eq!(
            failed.details.as_deref().map(OpaquePayload::format),
            Some("pl.model.failure")
        );
        assert_eq!(
            failed.details.as_deref().map(OpaquePayload::version),
            Some(3)
        );
        assert_eq!(
            failed.details.as_deref().map(OpaquePayload::content),
            Some(failed_details.as_str())
        );
        assert_eq!(failed.source_chain(), failed_chain);

        let cancelled = match &restored.state.attempts[1].outcome {
            AttemptOutcome::Cancelled { result } => match result {
                Err(error) => error.clone(),
                Ok(_) => panic!("a cancelled failure stays an error, not a produced step"),
            },
            other => panic!("cancelled attempt keeps its outcome variant: {other:?}"),
        };
        assert_eq!(cancelled.kind, ModelFailureKind::Cancelled);
        assert_eq!(cancelled.usage.input_tokens, Some(3));
        assert_eq!(cancelled.usage.reasoning_tokens, Some(2));
        assert_eq!(
            cancelled.details.as_deref().map(OpaquePayload::format),
            Some("pl.model.cancel")
        );
        assert_eq!(
            cancelled.details.as_deref().map(OpaquePayload::version),
            Some(5)
        );
        assert_eq!(
            cancelled.details.as_deref().map(OpaquePayload::content),
            Some(cancelled_details.as_str())
        );
        assert_eq!(cancelled.source_chain(), cancelled_chain);
    }

    /// 待交付工具结果的正文与交互请求同样外置，并按原格式/角色精确还原。
    #[test]
    fn pending_delivery_bodies_round_trip_exactly() {
        let payload = body("payload", THRESHOLD * 4);
        let projection = body("projection", THRESHOLD * 4);
        let interaction = body("interaction", THRESHOLD * 4);
        let delivery = ToolDelivery {
            target: ToolDeliveryTarget::Inbox {
                message_id: "message-1".to_owned(),
            },
            call_id: "call-1".to_owned(),
            tool_id: "tool-1".to_owned(),
            output: ToolOutput::new(
                OpaquePayload::new("pl.tool.result", 1, payload.as_str()).unwrap(),
                vec![ContextContent::Text {
                    text: projection.as_str().into(),
                }],
            )
            .with_revealed_tools(vec!["tool-2".to_owned()])
            .with_interaction(
                OpaquePayload::new("pl.tool.interaction", 1, interaction.as_str()).unwrap(),
            ),
            delivered_context: vec![ContextContent::Text {
                text: projection.as_str().into(),
            }],
            outcome: ToolOutcome::Succeeded,
        };
        let state = ThreadSnapshot {
            commit_sequence: 1,
            deliveries: vec![delivery].into(),
            ..Default::default()
        };
        // The transfer form keeps the pending delivery: this test externalizes the exact body the
        // writer hands to publication, not a pruned restart DTO that already settled it.
        let checkpoint = ThreadCheckpoint::capture_transfer("thread".to_owned(), 1, state);

        let (externalized, bodies) = checkpoint.externalize_bodies(THRESHOLD);
        assert_eq!(externalized.external_bodies.len(), 4);
        let serialized = serde_json::to_string(&externalized).unwrap();
        for hidden in [&payload, &projection, &interaction] {
            assert!(!serialized.contains(hidden.as_str()));
        }

        let restored = restore(&externalized, &bodies);
        assert!(restored.is_materialized());
        assert_eq!(
            serde_json::to_string(&restored).unwrap(),
            serde_json::to_string(&checkpoint).unwrap()
        );
        let output = &restored.state.deliveries[0].output;
        assert_eq!(output.payload().content(), payload.as_str());
        assert_eq!(output.control(), ToolControl::AwaitInteraction);
        assert_eq!(
            output.interaction().map(OpaquePayload::content),
            Some(interaction.as_str())
        );
        assert_eq!(output.revealed_tools().to_vec(), vec!["tool-2".to_owned()]);
    }

    /// 小正文保持内联，正常 checkpoint 仍是可直接阅读的 TOML。
    #[test]
    fn small_bodies_stay_inline() {
        let body = body("small", 8);
        let state = ThreadSnapshot {
            commit_sequence: 1,
            context: ContextSnapshot {
                revision: 1,
                records: vec![assistant_record(&body)].into(),
            },
            ..Default::default()
        };
        let checkpoint = ThreadCheckpoint::capture("thread".to_owned(), 1, state);
        let (externalized, bodies) = checkpoint.externalize_bodies(THRESHOLD);
        assert!(externalized.external_bodies.is_empty());
        assert!(bodies.is_empty());
        assert_eq!(
            externalized.state.context.records,
            checkpoint.state.context.records
        );
        assert!(
            serde_json::to_string(&externalized)
                .unwrap()
                .contains(&body)
        );
    }

    /// 摘要不符、未挂起的引用与被替换过的槽位都显式失败，绝不退化成空正文。
    #[test]
    fn replaced_or_unknown_body_fails_closed() {
        let body = body("assistant", THRESHOLD * 4);
        let state = ThreadSnapshot {
            commit_sequence: 1,
            context: ContextSnapshot {
                revision: 1,
                records: vec![assistant_record(&body)].into(),
            },
            ..Default::default()
        };
        let checkpoint = ThreadCheckpoint::capture("thread".to_owned(), 1, state);
        let (externalized, bodies) = checkpoint.externalize_bodies(THRESHOLD);
        let entry = externalized.pending_body().unwrap().clone();

        let mut replaced = externalized.clone();
        assert_eq!(
            replaced.materialize_body(&entry.reference, b"replaced"),
            Err(CheckpointBodyError::ContentMismatch)
        );

        let mut unknown = externalized.clone();
        let missing = CheckpointBodyReference::of(b"never externalized");
        assert_eq!(
            unknown.materialize_body(&missing, b"never externalized"),
            Err(CheckpointBodyError::UnknownReference(
                missing.digest.clone()
            ))
        );

        // 槽位里已经不是空占位正文时，文件与清单互相矛盾，必须失败闭锁。
        let mut tampered = externalized.clone();
        let mut records = tampered.state.context.records.to_vec();
        records[0].content[0] = ContextContent::Text {
            text: "tampered".into(),
        };
        tampered.state.context.records = records.into();
        assert_eq!(
            tampered.materialize_body(&entry.reference, &bodies[0].bytes),
            Err(CheckpointBodyError::SlotMismatch("record-1".to_owned()))
        );
    }

    /// 迁移前的内联 checkpoint 与未来 schema 的判定。
    #[test]
    fn schema_versions_are_explicit() {
        assert!(ThreadCheckpoint::supports_schema(
            ThreadCheckpoint::SCHEMA_VERSION
        ));
        assert!(ThreadCheckpoint::supports_schema(
            ThreadCheckpoint::LEGACY_SCHEMA_VERSION
        ));
        assert!(!ThreadCheckpoint::supports_schema(
            ThreadCheckpoint::SCHEMA_VERSION + 1
        ));
    }

    /// 未物化的 checkpoint 不能被交给 owner：激活必须看到完整正文。
    #[test]
    fn unresolved_bodies_cannot_activate() {
        let body = body("assistant", THRESHOLD * 4);
        let state = ThreadSnapshot {
            commit_sequence: 1,
            context: ContextSnapshot {
                revision: 1,
                records: vec![assistant_record(&body)].into(),
            },
            ..Default::default()
        };
        let checkpoint = ThreadCheckpoint::capture("thread".to_owned(), 1, state);
        let (externalized, _) = checkpoint.externalize_bodies(THRESHOLD);
        assert!(externalized.into_states("thread").is_err());
    }
}
