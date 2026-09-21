//! Durable input admission, separate from model-request admission and execution.
use super::*;

/// Immutable host-authored input. Only `context` is placed in the user context record.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadInput {
    pub id: String,
    pub payload: OpaquePayload,
    pub context: Vec<ContextContent>,
}

impl ThreadInput {
    /// Content digest used by the bounded identity ledger for duplicate submission checks.
    ///
    /// The digest covers the immutable host-authored body, never the framework routing receipt.
    pub fn digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.id.as_bytes());
        hasher.update([0]);
        hasher.update(self.payload.format().as_bytes());
        hasher.update([0]);
        hasher.update(self.payload.content().as_bytes());
        for content in &self.context {
            hasher.update([0]);
            hasher.update(serde_json::to_vec(content).unwrap_or_default());
        }
        format!("sha256:{:x}", hasher.finalize())
    }
}

/// Host-selected input routing policy, evaluated atomically by the Thread owner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputPolicy {
    #[default]
    StartOrSteer,
    StartOrQueue,
    StartOnly,
    SteerOnly,
}

/// Original routing decision. An unconsumed steer is retained for later recovery like any input.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind",
    content = "value"
)]
pub enum InputDelivery {
    #[default]
    NextTurn,
    CurrentTurn {
        turn_id: String,
    },
}

/// One input and its execution intent. Duplicate IDs retain their original routing receipt.
#[derive(Debug)]
pub struct InputSubmission {
    pub input: ThreadInput,
    pub policy: InputPolicy,
    pub drive: Option<InputDriverOptions>,
}

/// Consumption is a framework fact, never inferred from the input's opaque payload.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    tag = "kind",
    rename_all_fields = "camelCase"
)]
pub enum InputState {
    Pending,
    Consumed { turn_id: String, attempt_id: String },
    Discarded,
}

/// Input identity, FIFO ordinal and latest immutable state revision.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputRecord {
    /// Commit that first admitted this input; retained unchanged after consumption.
    pub accepted_sequence: u64,
    #[serde(default)]
    pub delivery: InputDelivery,
    pub input: ThreadInput,
    pub ordinal: u64,
    pub revision: u64,
    pub state: InputState,
}

/// Minimal resident identity of an admitted input that already reached a terminal state.
///
/// The body and framework routing facts stay in the durable effect history; this record is the
/// only thing the live owner keeps so a repeated submission can still be answered idempotently.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputIdentity {
    /// Content digest of the originally accepted body.
    pub digest: String,
    pub delivery: InputDelivery,
    pub id: String,
    pub ordinal: u64,
    pub revision: u64,
    pub state: InputState,
}

impl InputIdentity {
    /// Projects one terminal input record into its minimal durable identity.
    ///
    /// The Studio runtime persists this record in the per-Thread history identity index so a
    /// repeated submission can be answered after the resident terminal input has left core state.
    pub fn from_record(record: &InputRecord) -> Self {
        Self {
            digest: record.input.digest(),
            delivery: record.delivery.clone(),
            id: record.input.id.clone(),
            ordinal: record.ordinal,
            revision: record.revision,
            state: record.state.clone(),
        }
    }

    /// Rebuilds the framework receipt for a repeated, content-identical submission.
    pub fn receipt(&self, input: ThreadInput, accepted_sequence: u64) -> InputRecord {
        InputRecord {
            accepted_sequence,
            delivery: self.delivery.clone(),
            input,
            ordinal: self.ordinal,
            revision: self.revision,
            state: self.state.clone(),
        }
    }
}

/// Builds the minimal durable identity of a terminal input record.
///
/// Host layers use this to persist the identity index entry alongside the effect that settled the
/// input; core keeps only the pending queue and never re-reads the durable history.
pub fn input_identity(record: &InputRecord) -> InputIdentity {
    InputIdentity::from_record(record)
}

/// Rebuilds one framework receipt from a stored identity.
///
/// The caller passes the watermark that answers the repeat (the current commit for a live owner,
/// or the durable write sequence for a cold Thread), so a repeated submission returns a receipt
/// without re-admission.
pub fn input_receipt(
    identity: &InputIdentity,
    input: ThreadInput,
    accepted_sequence: u64,
) -> InputRecord {
    identity.receipt(input, accepted_sequence)
}

impl InputRecord {
    /// Returns the framework context identity used when this input was actually admitted to a
    /// model call. A queued or metadata-only input has no model-visible record. Compaction may
    /// remove the record from current context while the original request and journal retain it.
    pub fn context_record_id(&self) -> Option<String> {
        let InputState::Consumed {
            turn_id,
            attempt_id,
        } = &self.state
        else {
            return None;
        };
        if self.input.context.is_empty() {
            return None;
        }
        Some(
            if matches!(&self.delivery, InputDelivery::CurrentTurn { turn_id: target } if target == turn_id)
            {
                steering_record_id(turn_id, &self.input.id)
            } else {
                format!("{attempt_id}:input")
            },
        )
    }
}

/// Queue log operation. Consumption references the original content instead of copying it again.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    tag = "kind",
    content = "value",
    rename_all_fields = "camelCase"
)]
pub enum InputChange {
    Accepted(InputRecord),
    Transition {
        id: String,
        revision: u64,
        state: InputState,
    },
}

/// Execution parameters for the oldest pending input; these do not alter its saved content.
#[derive(Debug)]
pub struct QueuedTurn {
    pub turn_id: String,
    pub attempt_prefix: String,
    pub max_model_steps: crate::thread::ModelStepLimit,
    pub cancellation: CancellationToken,
}

/// Host-selected bounds for serial queue execution, frozen separately for each Turn.
#[derive(Debug, Clone, Copy)]
pub struct InputDriverOptions {
    pub max_model_steps: crate::thread::ModelStepLimit,
}

/// Live execution observation. Restored history always begins paused.
#[derive(Debug, Clone, Default)]
pub enum InputExecution {
    #[default]
    Paused,
    Ready,
    Interrupting {
        turn_id: String,
    },
    Failed {
        error: Arc<ThreadError>,
    },
    Running {
        input_id: String,
    },
}

/// Execution authorization is independent from physical cancellation and pending input facts.
/// Notifications may wake a dormant driver, but only explicit continuation can leave a pause.
#[derive(Debug)]
pub(super) enum InputDriver {
    Dormant,
    Enabled(InputDriverOptions),
    Paused,
    Failed(Arc<ThreadError>),
}

impl InputDriver {
    pub(super) fn options(&self) -> Option<InputDriverOptions> {
        match self {
            Self::Enabled(options) => Some(*options),
            _ => None,
        }
    }
    pub(super) fn error(&self) -> Option<&Arc<ThreadError>> {
        match self {
            Self::Failed(error) => Some(error),
            _ => None,
        }
    }
    pub(super) fn enable(&mut self, options: InputDriverOptions) {
        *self = Self::Enabled(options);
    }
    pub(super) fn wake(&mut self, options: InputDriverOptions) {
        match self {
            Self::Dormant | Self::Enabled(_) => self.enable(options),
            Self::Paused | Self::Failed(_) => {}
        }
    }
    pub(super) fn pause(&mut self) {
        if !matches!(self, Self::Failed(_)) {
            *self = Self::Paused;
        }
    }
    pub(super) fn fail(&mut self, error: Arc<ThreadError>) {
        if !matches!(self, Self::Failed(_)) {
            *self = Self::Failed(error);
        }
    }
}

impl Owner {
    pub(super) fn resume_inputs(&mut self, options: InputDriverOptions) -> Result<(), ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        self.input_driver.enable(options);
        self.publish_snapshot();
        Ok(())
    }

    pub(super) fn pause_inputs(&mut self) {
        self.input_driver.pause();
        self.publish_snapshot();
    }

    pub(super) async fn drive_one_input(&mut self) -> bool {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return false;
        }
        if !self.continuation_ready() {
            return false;
        }
        let Some(options) = self.input_driver.options() else {
            return false;
        };
        let pending_input = self
            .state
            .inputs
            .iter()
            .find(|record| record.state == InputState::Pending);
        let source = if let Some(record) = pending_input {
            format!("input:{}", record.input.id)
        } else if self.state.wake_messages_through > self.state.consumed_messages {
            format!("message:{}", self.state.wake_messages_through)
        } else {
            return false;
        };
        let has_input = pending_input.is_some();
        let mut suffix = self.state.commit_sequence;
        let turn_id = loop {
            let candidate = format!("{source}:{suffix}");
            if !self
                .state
                .turns
                .iter()
                .any(|turn| turn.turn_id == candidate)
                && !self
                    .state
                    .attempts
                    .iter()
                    .any(|attempt| attempt.turn_id == candidate)
            {
                break candidate;
            }
            let Some(next) = suffix.checked_add(1) else {
                self.input_driver
                    .fail(Arc::new(ThreadError::RevisionExhausted));
                self.pause_inputs();
                return false;
            };
            suffix = next;
        };
        let request = QueuedTurn {
            attempt_prefix: turn_id.clone(),
            turn_id,
            max_model_steps: options.max_model_steps,
            cancellation: CancellationToken::new(),
        };
        let result = if has_input {
            self.run_next_input(request).await
        } else {
            let active = self.interrupt.activate(&request.cancellation);
            self.run_turn(TurnInput {
                turn_id: request.turn_id,
                attempt_prefix: request.attempt_prefix,
                max_model_steps: request.max_model_steps,
                content: Vec::new(),
                cancellation: active.token.clone(),
            })
            .await
            .map(Some)
        };
        match result {
            Ok(Some(TurnCompletion {
                outcome: TurnOutcome::Completed,
                ..
            })) => {}
            Ok(Some(TurnCompletion {
                outcome: TurnOutcome::WaitingInteraction | TurnOutcome::StepLimit,
                ..
            }))
            | Ok(None) => self.pause_inputs(),
            Err(ThreadError::Cancelled)
                if self.interrupted_turn.is_some()
                    && self.input_driver.options().is_some()
                    && !self.interrupt.is_closing() => {}
            Err(ThreadError::Cancelled) => self.pause_inputs(),
            Err(error) => {
                self.input_driver.fail(Arc::new(error));
                self.pause_inputs();
            }
        }
        true
    }

    pub(super) fn accept_input(&mut self, input: ThreadInput) -> Result<InputRecord, ThreadError> {
        self.accept_input_with_policy(input, InputPolicy::StartOrQueue)
    }

    pub(super) fn accept_input_with_policy(
        &mut self,
        input: ThreadInput,
        policy: InputPolicy,
    ) -> Result<InputRecord, ThreadError> {
        if input.id.is_empty() {
            return Err(ThreadError::InvalidIdentity);
        }
        if let Some(receipt) = self.duplicate_input_receipt(&input)? {
            return Ok(receipt);
        }
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        self.refresh_storage_pressure();
        self.publish_snapshot();
        if self.state.persistence.pressure_paused {
            return Err(ThreadError::StoragePressure);
        }
        if let Some(error) = &self.cold_error {
            return Err(ThreadError::Storage(error.clone()));
        }
        let active_turn = self
            .state
            .turns
            .last()
            .filter(|turn| turn.state == TurnState::Running)
            .map(|turn| turn.turn_id.clone());
        let delivery = match policy {
            InputPolicy::StartOnly
                if active_turn.is_some()
                    || self
                        .state
                        .inputs
                        .iter()
                        .any(|input| input.state == InputState::Pending) =>
            {
                return Err(ThreadError::InputRequiresIdle);
            }
            InputPolicy::SteerOnly if active_turn.is_none() => {
                return Err(ThreadError::InputRequiresActiveTurn);
            }
            InputPolicy::StartOrSteer | InputPolicy::SteerOnly => active_turn
                .map_or(InputDelivery::NextTurn, |turn_id| {
                    InputDelivery::CurrentTurn { turn_id }
                }),
            InputPolicy::StartOnly | InputPolicy::StartOrQueue => InputDelivery::NextTurn,
        };
        let record = stage_input_with_delivery(&mut self.state, input, delivery)?;
        self.publish();
        Ok(record)
    }

    /// Answers a repeated submission from bounded current state.
    ///
    /// A still-pending input keeps its full record; an already-consumed or discarded input is
    /// answered from its identity ledger entry by comparing the immutable content digest.
    fn duplicate_input_receipt(
        &self,
        input: &ThreadInput,
    ) -> Result<Option<InputRecord>, ThreadError> {
        duplicate_input_receipt(&self.state, input)
    }

    pub(super) fn discard_input(&mut self, id: &str) -> Result<InputRecord, ThreadError> {
        if self.state.lifecycle == ThreadLifecycle::Closed {
            return Err(ThreadError::Closed);
        }
        let previous = self
            .state
            .inputs
            .iter()
            .find(|record| record.input.id == id)
            .ok_or(ThreadError::InvalidIdentity)?;
        match previous.state {
            InputState::Discarded => return Ok(previous.clone()),
            InputState::Consumed { .. } => return Err(ThreadError::InputConsumed),
            InputState::Pending => {}
        }
        if self.active_inputs.iter().any(|active| active == id) {
            return Err(ThreadError::InputInUse);
        }
        let record = self.change_input(id, InputState::Discarded)?;
        self.publish();
        Ok(record)
    }

    pub(super) async fn run_next_input(
        &mut self,
        request: QueuedTurn,
    ) -> Result<Option<TurnCompletion>, ThreadError> {
        let Some(record) = self
            .state
            .inputs
            .iter()
            .find(|record| record.state == InputState::Pending)
            .cloned()
        else {
            return Ok(None);
        };
        let through = self.input_batch_through.take().unwrap_or(record.ordinal);
        let batch: Vec<_> = self
            .state
            .inputs
            .iter()
            .filter(|input| input.state == InputState::Pending && input.ordinal <= through)
            .cloned()
            .collect();
        self.active_inputs = batch.iter().map(|input| input.input.id.clone()).collect();
        let content = batch
            .into_iter()
            .flat_map(|input| input.input.context)
            .collect();
        let active = self.interrupt.activate(&request.cancellation);
        let result = self
            .run_turn(TurnInput {
                turn_id: request.turn_id,
                attempt_prefix: request.attempt_prefix,
                content,
                max_model_steps: request.max_model_steps,
                cancellation: active.token.clone(),
            })
            .await;
        self.active_inputs.clear();
        self.publish_snapshot();
        result.map(Some)
    }

    /// Stages consumption just before request publication; no independently published watermark.
    pub(super) fn steering_context(&self, turn_id: &str) -> (Vec<ContextRecord>, Vec<String>) {
        let mut records = Vec::new();
        let mut ids = Vec::new();
        for input in self.state.inputs.iter().filter(|input| input.state == InputState::Pending
            && matches!(&input.delivery, InputDelivery::CurrentTurn { turn_id: target } if target == turn_id)
            && !self.active_inputs.contains(&input.input.id)) {
            ids.push(input.input.id.clone());
            if !input.input.context.is_empty() {
                records.push(ContextRecord { id: steering_record_id(turn_id, &input.input.id), turn_id: Some(turn_id.into()),
                    source: ContextSource::User, content: input.input.context.clone(), tool_calls: Vec::new() });
            }
        }
        (records, ids)
    }

    pub(super) fn validate_steering(&self, ids: &[String]) -> Result<(), ThreadError> {
        if ids.iter().any(|id| {
            !self
                .state
                .inputs
                .iter()
                .any(|record| &record.input.id == id && record.state == InputState::Pending)
        }) {
            return Err(ThreadError::InputConsumed);
        }
        Ok(())
    }

    pub(super) fn consume_steering(
        &mut self,
        ids: &[String],
        turn_id: &str,
        attempt_id: &str,
    ) -> Result<(), ThreadError> {
        for id in ids {
            self.change_input(
                id,
                InputState::Consumed {
                    turn_id: turn_id.into(),
                    attempt_id: attempt_id.into(),
                },
            )?;
        }
        Ok(())
    }

    pub(super) fn consume_active_input(
        &mut self,
        turn_id: &str,
        attempt_id: &str,
    ) -> Result<(), ThreadError> {
        for id in self.active_inputs.clone() {
            let Some(previous) = self
                .state
                .inputs
                .iter()
                .find(|record| record.input.id == id)
            else {
                // The input already reached a terminal state and left the bounded resident queue
                // when an earlier step of this Turn consumed it. Its durable receipt stays the
                // authority: a consumption attributed to this Turn completes normally, while any
                // other terminal state stays an input conflict.
                if active_input_already_consumed(&self.state, &id, turn_id) {
                    continue;
                }
                return Err(ThreadError::InputConsumed);
            };
            match &previous.state {
                InputState::Pending => {
                    self.change_input(
                        &id,
                        InputState::Consumed {
                            turn_id: turn_id.into(),
                            attempt_id: attempt_id.into(),
                        },
                    )?;
                }
                InputState::Consumed {
                    turn_id: consumed_turn,
                    ..
                } if consumed_turn == turn_id => {}
                InputState::Consumed { .. } | InputState::Discarded => {
                    return Err(ThreadError::InputConsumed);
                }
            }
        }
        Ok(())
    }

    fn change_input(&mut self, id: &str, state: InputState) -> Result<InputRecord, ThreadError> {
        let mut inputs = self.state.inputs.to_vec();
        let record = inputs
            .iter_mut()
            .find(|record| record.input.id == id)
            .ok_or(ThreadError::InvalidIdentity)?;
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        record.state = state;
        let record = record.clone();
        self.state.inputs = inputs.into();
        self.append_input_change(InputChange::Transition {
            id: record.input.id.clone(),
            revision: record.revision,
            state: record.state.clone(),
        });
        Ok(record)
    }

    fn append_input_change(&mut self, record: InputChange) {
        let mut changes = self.state.input_changes.to_vec();
        changes.push(record);
        self.state.input_changes = changes.into();
    }
}

/// Validates queue changes against the same commit's request, without invoking a decoder or executor.
pub(super) fn replay(
    state: &mut ThreadSnapshot,
    commit: &ThreadEffectBatch,
) -> Result<(), ThreadError> {
    let mut inputs = state.inputs.to_vec();
    let mut changes = state.input_changes.to_vec();
    let mut identities = std::collections::BTreeSet::new();
    let mut consumed_context = Vec::new();
    let mut consumed_request = None;
    for change in commit.inputs.iter() {
        let record = match change {
            InputChange::Accepted(record) => {
                if record.accepted_sequence != commit.sequence
                    || record.ordinal != inputs.len() as u64 + 1
                    || record.revision != 1
                    || record.state != InputState::Pending
                    || inputs
                        .iter()
                        .any(|previous| previous.input.id == record.input.id)
                {
                    return Err(ThreadError::InvalidOutput);
                }
                if let InputDelivery::CurrentTurn { turn_id } = &record.delivery
                    && !state
                        .turns
                        .iter()
                        .any(|turn| &turn.turn_id == turn_id && turn.state == TurnState::Running)
                {
                    return Err(ThreadError::InvalidOutput);
                }
                inputs.push(record.clone());
                record.clone()
            }
            InputChange::Transition {
                id,
                revision,
                state: next,
            } => {
                let previous = inputs
                    .iter_mut()
                    .find(|previous| &previous.input.id == id)
                    .ok_or(ThreadError::InvalidIdentity)?;
                if previous.revision.checked_add(1) != Some(*revision)
                    || previous.state != InputState::Pending
                    || *next == InputState::Pending
                {
                    return Err(ThreadError::InvalidOutput);
                }
                previous.revision = *revision;
                previous.state = next.clone();
                previous.clone()
            }
        };
        if record.input.id.is_empty() || !identities.insert(record.input.id.clone()) {
            return Err(ThreadError::InvalidIdentity);
        }
        if let InputState::Consumed {
            turn_id,
            attempt_id,
        } = &record.state
        {
            let steering = matches!(&record.delivery, InputDelivery::CurrentTurn { turn_id: target } if target == turn_id);
            if !steering {
                consumed_context.extend(record.input.context.clone());
                consumed_request = Some((turn_id.clone(), attempt_id.clone()));
            }
            let attempt = commit
                .attempt
                .as_ref()
                .filter(|attempt| {
                    &attempt.turn_id == turn_id
                        && &attempt.attempt_id == attempt_id
                        && matches!(attempt.outcome, AttemptOutcome::Running)
                })
                .ok_or(ThreadError::InvalidOutput)?;
            let admitted = state
                .attempts
                .iter()
                .find(|item| item.attempt_id == attempt.attempt_id)
                .ok_or(ThreadError::InvalidOutput)?;
            if steering && !record.input.context.is_empty() {
                let user = admitted
                    .input
                    .records
                    .iter()
                    .find(|item| record.context_record_id().as_deref() == Some(item.id.as_str()))
                    .ok_or(ThreadError::InvalidOutput)?;
                if user.source != ContextSource::User
                    || user.turn_id.as_ref() != Some(turn_id)
                    || user.content != record.input.context
                {
                    return Err(ThreadError::InvalidOutput);
                }
            }
            if inputs.iter().any(|previous| {
                previous.ordinal < record.ordinal && previous.state == InputState::Pending
                    && (!steering || matches!(&previous.delivery, InputDelivery::CurrentTurn { turn_id: target } if target == turn_id))
            }) {
                return Err(ThreadError::InvalidOutput);
            }
        }
        changes.push(change.clone());
    }
    if let Some((turn_id, attempt_id)) = consumed_request {
        let admitted = state
            .attempts
            .iter()
            .find(|attempt| attempt.attempt_id == attempt_id)
            .ok_or(ThreadError::InvalidOutput)?;
        let user = admitted
            .input
            .records
            .iter()
            .find(|record| record.id == format!("{attempt_id}:input"));
        match user {
            Some(record)
                if record.source == ContextSource::User
                    && record.turn_id.as_ref() == Some(&turn_id)
                    && record.content == consumed_context => {}
            None if consumed_context.is_empty() => {}
            _ => return Err(ThreadError::InvalidOutput),
        }
    }
    state.inputs = inputs.into();
    let newest_ordinal = state
        .inputs
        .iter()
        .map(|record| record.ordinal)
        .max()
        .unwrap_or(0);
    let watermark = state.input_ordinal;
    state.input_ordinal = watermark.max(newest_ordinal);
    state.input_changes = changes.into();
    Ok(())
}

fn steering_record_id(turn_id: &str, input_id: &str) -> String {
    format!("steer:{}:{turn_id}{input_id}", turn_id.len())
}

/// Answers a repeated input submission from bounded current state.
///
/// A still-pending input keeps its full record; a terminal input is answered from the bounded
/// identity window by comparing the immutable content digest. An identity older than that window
/// is answered by the host's durable identity index, so core never accumulates history.
/// A repeated id with a different body stays an identity conflict.
pub(super) fn duplicate_input_receipt(
    state: &ThreadSnapshot,
    input: &ThreadInput,
) -> Result<Option<InputRecord>, ThreadError> {
    if let Some(previous) = state
        .inputs
        .iter()
        .find(|record| record.input.id == input.id)
    {
        if previous.input != *input {
            return Err(ThreadError::InvalidIdentity);
        }
        return Ok(Some(previous.clone()));
    }
    let Some(identity) = state
        .terminal_inputs
        .iter()
        .find(|identity| identity.id == input.id)
    else {
        return Ok(None);
    };
    if identity.digest != input.digest() {
        return Err(ThreadError::InvalidIdentity);
    }
    Ok(Some(identity.receipt(input.clone(), state.commit_sequence)))
}

/// Reports whether bounded durable state already records `id` as consumed by `turn_id`.
///
/// A consumed input leaves the resident queue as soon as its commit publishes, so a later step of
/// the same Turn sees only its durable receipt. An identity that already left the bounded window is
/// accepted as this Turn's own admission receipt: the owner lists only inputs it admitted into the
/// active batch, discarding an in-use input is refused, and Turns are serial, so a listed input that
/// is no longer resident can only have been consumed by this Turn.
fn active_input_already_consumed(state: &ThreadSnapshot, id: &str, turn_id: &str) -> bool {
    state
        .terminal_inputs
        .iter()
        .find(|identity| identity.id == id)
        .is_none_or(|identity| {
            matches!(&identity.state, InputState::Consumed { turn_id: consumed, .. } if consumed == turn_id)
        })
}

/// Stages input as part of an already-admitted control transaction, without starting execution.
pub(super) fn stage_input(
    state: &mut ThreadSnapshot,
    input: ThreadInput,
) -> Result<InputRecord, ThreadError> {
    stage_input_with_delivery(state, input, InputDelivery::NextTurn)
}

fn stage_input_with_delivery(
    state: &mut ThreadSnapshot,
    input: ThreadInput,
    delivery: InputDelivery,
) -> Result<InputRecord, ThreadError> {
    if input.id.is_empty() {
        return Err(ThreadError::InvalidIdentity);
    }
    if let Some(previous) = state
        .inputs
        .iter()
        .find(|record| record.input.id == input.id)
    {
        return if previous.input == input {
            Ok(previous.clone())
        } else {
            Err(ThreadError::InvalidIdentity)
        };
    }
    if state.lifecycle != ThreadLifecycle::Open {
        return Err(ThreadError::Closed);
    }
    let ordinal = state
        .input_ordinal
        .checked_add(1)
        .ok_or(ThreadError::RevisionExhausted)?;
    state.input_ordinal = ordinal;
    let record = InputRecord {
        accepted_sequence: state
            .commit_sequence
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?,
        delivery,
        input,
        ordinal,
        revision: 1,
        state: InputState::Pending,
    };
    let mut inputs = state.inputs.to_vec();
    inputs.push(record.clone());
    state.inputs = inputs.into();
    let mut changes = state.input_changes.to_vec();
    changes.push(InputChange::Accepted(record.clone()));
    state.input_changes = changes.into();
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelSession, PreparedModelCall};
    use crate::thread::tests::history_snapshot;
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    struct Probe {
        executed: Arc<AtomicUsize>,
        preparing: Arc<Notify>,
        release: Option<Arc<Notify>>,
    }

    impl ModelSession for Probe {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            self.preparing.notify_one();
            if let Some(release) = self.release.take() {
                release.notified().await;
            }
            let executed = self.executed.clone();
            Ok(PreparedModelCall::new(async move {
                executed.fetch_add(1, Ordering::SeqCst);
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: Arc::from("reply"),
                    }],
                    tool_calls: Vec::new(),
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }

        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    fn submitted(id: &str) -> ThreadInput {
        ThreadInput {
            id: id.into(),
            payload: OpaquePayload::new(
                "future.input",
                902,
                " {\"endTurn\":true, \"large\":90071992547409999} \r\n原文",
            )
            .unwrap(),
            context: vec![ContextContent::Text {
                text: Arc::from(id),
            }],
        }
    }

    fn execution(turn_id: &str, cancellation: CancellationToken) -> QueuedTurn {
        QueuedTurn {
            turn_id: turn_id.into(),
            attempt_prefix: format!("{turn_id}-attempt"),
            max_model_steps: crate::thread::ModelStepLimit::Limited(
                std::num::NonZeroU32::new(3).unwrap(),
            ),
            cancellation,
        }
    }

    #[tokio::test]
    async fn queue_admission_is_idempotent_and_consumption_pairs_with_the_actual_request() {
        let executed = Arc::new(AtomicUsize::new(0));
        let thread = ThreadHandle::start(
            "queue".into(),
            DynModelSession::new(Probe {
                executed: executed.clone(),
                preparing: Arc::new(Notify::new()),
                release: None,
            }),
        )
        .unwrap();
        let receipt = thread.submit_input(submitted("first")).await.unwrap();
        assert_eq!(
            thread.submit_input(submitted("first")).await.unwrap(),
            receipt
        );
        thread.submit_input(submitted("second")).await.unwrap();
        let mut conflict = submitted("first");
        conflict.payload = OpaquePayload::text("different bytes");
        assert!(matches!(
            thread.submit_input(conflict).await,
            Err(ThreadError::InvalidIdentity)
        ));
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        assert!(thread.snapshot().context.records.is_empty());
        assert!(
            thread
                .run_next_input(execution("turn-1", CancellationToken::new()))
                .await
                .unwrap()
                .is_some()
        );
        thread
            .resume_inputs(InputDriverOptions {
                max_model_steps: crate::thread::ModelStepLimit::Limited(
                    std::num::NonZeroU32::new(3).unwrap(),
                ),
            })
            .await
            .unwrap();
        let mut subscription = thread.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                subscription.next().await.expect("owner stays live");
                // A consumed input leaves the bounded live queue; observe the committed projection.
                let committed = history_snapshot(&thread).await;
                if matches!(committed.inputs[1].state, InputState::Consumed { .. }) {
                    break;
                }
            }
        })
        .await
        .expect("actor drains the next input without a product task");
        assert!(
            thread
                .run_next_input(execution("unused", CancellationToken::new()))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(executed.load(Ordering::SeqCst), 2);
        let snapshot = history_snapshot(&thread).await;
        assert_eq!(snapshot.inputs[0].input, submitted("first"));
        assert_eq!(
            snapshot.inputs[0].state,
            InputState::Consumed {
                turn_id: "turn-1".into(),
                attempt_id: "turn-1-attempt:0".into()
            }
        );
        assert_eq!(
            snapshot.inputs[1].state,
            InputState::Consumed {
                turn_id: snapshot.attempts[1].turn_id.clone(),
                attempt_id: snapshot.attempts[1].attempt_id.clone()
            }
        );
        let history = thread.effects().await.unwrap();
        let consumption = history
            .iter()
            .find(|commit| {
                commit.inputs.iter().any(|change| {
                    matches!(
                        change,
                        InputChange::Transition {
                            state: InputState::Consumed { .. },
                            ..
                        }
                    )
                })
            })
            .unwrap();
        assert!(consumption.context.is_some());
        assert!(consumption.attempt.is_some());
        assert!(
            !consumption
                .encode()
                .unwrap()
                .content()
                .contains("future.input")
        );
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_preparation_preserves_queued_inputs_and_restore_never_executes_them() {
        let executed = Arc::new(AtomicUsize::new(0));
        let preparing = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let thread = ThreadHandle::start(
            "queue".into(),
            DynModelSession::new(Probe {
                executed: executed.clone(),
                preparing: preparing.clone(),
                release: Some(release.clone()),
            }),
        )
        .unwrap();
        thread.submit_input(submitted("first")).await.unwrap();
        let cancellation = CancellationToken::new();
        let running = {
            let thread = thread.clone();
            let request = execution("cancelled-turn", cancellation.clone());
            tokio::spawn(async move { thread.run_next_input(request).await })
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), preparing.notified())
            .await
            .expect("model preparation starts");
        thread.submit_input(submitted("second")).await.unwrap();
        assert!(matches!(
            thread.discard_input("first".into()).await,
            Err(ThreadError::InputInUse)
        ));
        assert_eq!(thread.snapshot().inputs.len(), 2);
        cancellation.cancel();
        release.notify_one();
        assert!(matches!(
            running.await.unwrap(),
            Err(ThreadError::Cancelled)
        ));
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        assert!(
            thread
                .snapshot()
                .inputs
                .iter()
                .all(|record| record.state == InputState::Pending)
        );
        assert_eq!(
            history_snapshot(&thread).await.turns[0].input_id.as_deref(),
            Some("first")
        );
        assert!(
            history_snapshot(&thread).await.inputs[0]
                .context_record_id()
                .is_none()
        );
        let checkpoint = thread
            .checkpoint(thread.snapshot().commit_sequence)
            .unwrap();
        thread.close().await.unwrap();
        let restored = ThreadHandle::resume(
            "queue".into(),
            DynModelSession::new(Probe {
                executed: executed.clone(),
                preparing: Arc::new(Notify::new()),
                release: None,
            }),
            Some(checkpoint),
        )
        .unwrap();
        assert_eq!(restored.snapshot().inputs, thread.snapshot().inputs);
        assert!(restored.snapshot().attempts.is_empty());
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        restored
            .run_next_input(execution("resumed-turn", CancellationToken::new()))
            .await
            .unwrap();
        assert_eq!(executed.load(Ordering::SeqCst), 1);
        // A resumed owner keeps no pre-restore Turn history, so its committed projection holds only
        // the refreshed execution of the still-pending input.
        let snapshot = history_snapshot(&restored).await;
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(snapshot.turns[0].input_id.as_deref(), Some("first"));
        // The refreshed execution admitted the pending input into model context.
        let context = snapshot
            .context
            .records
            .iter()
            .find(|record| record.content == submitted("first").context)
            .expect("the refreshed execution admits the pending input into context");
        assert_eq!(context.content, submitted("first").context);
        // A terminal input leaves the bounded live queue, so identify the still-pending input by
        // identity instead of by index.
        assert!(
            restored.snapshot().inputs.iter().any(|record| {
                record.input.id == "second" && record.state == InputState::Pending
            })
        );
        restored.discard_input("second".into()).await.unwrap();
        assert!(restored.effects().await.unwrap().iter().any(|effect| {
            effect.inputs.iter().any(|change| {
                matches!(
                    change,
                    InputChange::Transition {
                        id,
                        state: InputState::Discarded,
                        ..
                    } if id.as_str() == "second"
                )
            })
        }));
        assert!(
            restored
                .run_next_input(execution("empty", CancellationToken::new()))
                .await
                .unwrap()
                .is_none()
        );
        restored.close().await.unwrap();
    }
    struct FailingProbe(Arc<AtomicUsize>);
    impl ModelSession for FailingProbe {
        fn prepare(
            &mut self,
            _: ModelRequest,
        ) -> impl std::future::Future<Output = Result<PreparedModelCall, ModelError>> + Send
        {
            let calls = self.0.clone();
            async move {
                Ok(PreparedModelCall::new(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(ModelError {
                        details: None,
                        kind: crate::model::ModelFailureKind::Unavailable,
                        usage: Default::default(),
                        source: Some(Box::new(std::io::Error::other("provider failure"))),
                    })
                }))
            }
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn failed_driven_turn_pauses_following_inputs_until_an_explicit_resume() {
        let calls = Arc::new(AtomicUsize::new(0));
        let thread = ThreadHandle::start(
            "queue".into(),
            DynModelSession::new(FailingProbe(calls.clone())),
        )
        .unwrap();
        thread.submit_input(submitted("first")).await.unwrap();
        thread.submit_input(submitted("second")).await.unwrap();
        let options = InputDriverOptions {
            max_model_steps: crate::thread::ModelStepLimit::Limited(
                std::num::NonZeroU32::new(3).unwrap(),
            ),
        };
        let mut subscription = thread.subscribe();
        thread.resume_inputs(options).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if matches!(
                    subscription.next().await.unwrap().input_execution,
                    InputExecution::Failed { .. }
                ) {
                    break;
                }
            }
        })
        .await
        .expect("failed turn publishes a paused driver");
        let duplicate = thread
            .submit_input_and_run(submitted("first"), options)
            .await
            .unwrap();
        assert!(matches!(duplicate.state, InputState::Consumed { .. }));
        thread.submit_input(submitted("third")).await.unwrap();
        thread.effects().await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Consumed inputs leave the bounded live queue, so the admitted order comes from the
        // committed projection.
        let committed = history_snapshot(&thread).await;
        assert_eq!(committed.inputs[1].state, InputState::Pending);
        assert_eq!(committed.inputs[2].state, InputState::Pending);
        thread.resume_inputs(options).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let snapshot = subscription.next().await.unwrap();
                if matches!(snapshot.input_execution, InputExecution::Failed { .. })
                    && history_snapshot(&thread).await.attempts.len() == 2
                {
                    break;
                }
            }
        })
        .await
        .expect("explicit resume drives one new request then pauses again");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            history_snapshot(&thread).await.inputs[2].state,
            InputState::Pending
        );
        thread.close().await.unwrap();
    }
    struct RedirectProbe {
        started: Arc<Notify>,
        cleanup: Arc<Notify>,
        token: Arc<std::sync::Mutex<Option<CancellationToken>>>,
        requests: Arc<std::sync::Mutex<Vec<ContextSnapshot>>>,
    }

    impl ModelSession for RedirectProbe {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let first = self.requests.lock().unwrap().is_empty();
            self.requests.lock().unwrap().push(request.context.clone());
            if first {
                *self.token.lock().unwrap() = Some(request.cancellation.clone());
                self.started.notify_one();
                request.cancellation.cancelled().await;
                self.cleanup.notified().await;
            }
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![],
                    tool_calls: vec![],
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn continue_interrupts_preparation_and_batches_pending_inputs_without_replaying_them() {
        let started = Arc::new(Notify::new());
        let cleanup = Arc::new(Notify::new());
        let token = Arc::new(std::sync::Mutex::new(None));
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread = ThreadHandle::start(
            "redirect".into(),
            DynModelSession::new(RedirectProbe {
                started: started.clone(),
                cleanup: cleanup.clone(),
                token: token.clone(),
                requests: requests.clone(),
            }),
        )
        .unwrap();
        let options = InputDriverOptions {
            max_model_steps: ModelStepLimit::Unlimited,
        };
        thread
            .submit_input_and_continue(submitted("first"), options)
            .await
            .unwrap();
        started.notified().await;
        let mut conflict = submitted("first");
        conflict.context = submitted("changed").context;
        assert!(matches!(
            thread.submit_input_and_continue(conflict, options).await,
            Err(ThreadError::InvalidIdentity)
        ));
        assert!(!token.lock().unwrap().as_ref().unwrap().is_cancelled());
        let receipt = thread
            .submit_input_and_continue(submitted("second"), options)
            .await
            .unwrap();
        assert!(
            token.lock().unwrap().as_ref().unwrap().is_cancelled(),
            "accepted redirect cancels current generation before returning"
        );
        thread
            .submit_input_and_continue(submitted("third"), options)
            .await
            .unwrap();
        assert_eq!(
            thread
                .submit_input_and_continue(submitted("second"), options)
                .await
                .unwrap(),
            receipt
        );
        assert_eq!(
            requests.lock().unwrap().len(),
            1,
            "cleanup must finish before the next request"
        );
        cleanup.notify_one();
        let mut updates = thread.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                updates.next().await.unwrap();
                // A finished Turn is a history fact: settle on the committed projection.
                let committed = history_snapshot(&thread).await;
                if committed
                    .turns
                    .last()
                    .is_some_and(|turn| turn.state == TurnState::Finished(TurnOutcome::Completed))
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let state = history_snapshot(&thread).await;
        assert_eq!(state.turns.len(), 2);
        assert_eq!(state.turns[0].state, TurnState::Interrupted);
        assert!(state.inputs.iter().all(|input| matches!(&input.state, InputState::Consumed { turn_id, .. } if turn_id == &state.turns[1].turn_id)));
        assert_eq!(requests.lock().unwrap().len(), 2);
        assert_eq!(
            requests.lock().unwrap()[1]
                .records
                .iter()
                .flat_map(|record| record.content.clone())
                .collect::<Vec<_>>(),
            [
                submitted("first").context,
                submitted("second").context,
                submitted("third").context
            ]
            .concat()
        );
        assert!(thread.snapshot().inputs.is_empty());
        thread
            .submit_input_and_continue(submitted("second"), options)
            .await
            .unwrap();
        assert_eq!(history_snapshot(&thread).await.turns.len(), 2);
        thread.close().await.unwrap();
    }

    struct ExecuteCancellationProbe {
        executing: Arc<Notify>,
        requests: Arc<std::sync::Mutex<Vec<ContextSnapshot>>>,
    }

    impl ModelSession for ExecuteCancellationProbe {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let first = self.requests.lock().unwrap().is_empty();
            self.requests.lock().unwrap().push(request.context.clone());
            if !first {
                return Ok(PreparedModelCall::new(async move {
                    Ok(ModelStepOutput {
                        attempt_id: request.attempt_id,
                        base_context_revision: request.context.revision,
                        content: vec![ContextContent::Text {
                            text: Arc::from("reply"),
                        }],
                        tool_calls: Vec::new(),
                        private_context: None,
                        usage: Default::default(),
                    })
                }));
            }
            let cancellation = request.cancellation.clone();
            let executing = self.executing.clone();
            Ok(PreparedModelCall::new(async move {
                executing.notify_one();
                // Wait until the runtime targets this Turn's generation, then report the
                // interruption through the new contract: the call observed its own
                // cancellation, so it must not be classified as a provider fault.
                cancellation.cancelled().await;
                Err(ModelError {
                    details: None,
                    kind: crate::model::ModelFailureKind::Cancelled,
                    usage: Default::default(),
                    source: Some(Box::new(std::io::Error::other(
                        "model invocation cancelled",
                    ))),
                })
            }))
        }

        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn continue_interrupts_execution_and_preserves_the_interrupted_reason() {
        let executing = Arc::new(Notify::new());
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread = ThreadHandle::start(
            "redirect-execute".into(),
            DynModelSession::new(ExecuteCancellationProbe {
                executing: executing.clone(),
                requests: requests.clone(),
            }),
        )
        .unwrap();
        let options = InputDriverOptions {
            max_model_steps: ModelStepLimit::Unlimited,
        };
        thread
            .submit_input_and_continue(submitted("first"), options)
            .await
            .unwrap();
        executing.notified().await;
        thread
            .submit_input_and_continue(submitted("second"), options)
            .await
            .unwrap();
        let mut updates = thread.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                updates.next().await.unwrap();
                // A finished Turn is a history fact: settle on the committed projection.
                let committed = history_snapshot(&thread).await;
                if committed
                    .turns
                    .last()
                    .is_some_and(|turn| turn.state == TurnState::Finished(TurnOutcome::Completed))
                {
                    break;
                }
            }
        })
        .await
        .expect("a cancelled execution must not fault the input driver");
        let state = history_snapshot(&thread).await;
        assert_eq!(
            state.turns.len(),
            2,
            "the interrupted Turn must not stop the driver"
        );
        assert_eq!(
            state.turns[0].state,
            TurnState::Interrupted,
            "a targeted execution interrupt preserves its terminal reason"
        );
        assert!(
            !matches!(state.turns[0].state, TurnState::Failed { .. }),
            "the interruption must not surface as a model failure description"
        );
        assert_eq!(
            state.turns[1].state,
            TurnState::Finished(TurnOutcome::Completed)
        );
        assert!(
            state
                .inputs
                .iter()
                .all(|input| !matches!(&input.state, InputState::Pending)),
            "the inserted input must be consumed by the next Turn"
        );
        let consumed_turn = |id: &str| {
            state
                .inputs
                .iter()
                .find(|record| record.input.id == id)
                .and_then(|record| match &record.state {
                    InputState::Consumed { turn_id, .. } => Some(turn_id.as_str()),
                    _ => None,
                })
        };
        assert_eq!(
            consumed_turn("first"),
            Some(state.turns[0].turn_id.as_str()),
            "the already-consumed input stays bound to the interrupted Turn"
        );
        assert_eq!(
            consumed_turn("second"),
            Some(state.turns[1].turn_id.as_str()),
            "the inserted input advances to the next Turn"
        );
        assert_eq!(requests.lock().unwrap().len(), 2);
        assert_eq!(
            requests.lock().unwrap()[1]
                .records
                .iter()
                .flat_map(|record| record.content.clone())
                .collect::<Vec<_>>(),
            [submitted("first").context, submitted("second").context].concat()
        );
        thread.close().await.unwrap();
    }
}
