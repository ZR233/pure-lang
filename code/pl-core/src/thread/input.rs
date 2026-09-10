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

impl InputRecord {
    /// Returns the framework context identity used when this input was actually admitted to a model call.
    /// A queued or metadata-only input has no model-visible record. Compaction may remove the record
    /// from current context while the original request and journal retain it.
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
    pub max_model_steps: std::num::NonZeroU32,
    pub cancellation: CancellationToken,
}

/// Host-selected bounds for serial queue execution, frozen separately for each Turn.
#[derive(Debug, Clone, Copy)]
pub struct InputDriverOptions {
    pub max_model_steps: std::num::NonZeroU32,
}

/// Live execution observation. Restored history always begins paused.
#[derive(Debug, Clone, Default)]
pub enum InputExecution {
    #[default]
    Paused,
    Ready,
    Failed {
        error: Arc<ThreadError>,
    },
    Running {
        input_id: String,
    },
}

impl Owner {
    pub(super) fn resume_inputs(&mut self, options: InputDriverOptions) -> Result<(), ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        self.input_driver = Some(options);
        self.input_driver_error = None;
        self.publish_snapshot();
        Ok(())
    }

    pub(super) fn pause_inputs(&mut self) {
        self.input_driver = None;
        self.publish_snapshot();
    }

    pub(super) async fn drive_one_input(&mut self) -> bool {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return false;
        }
        let Some(options) = self.input_driver else {
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
                self.input_driver_error = Some(Arc::new(ThreadError::RevisionExhausted));
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
                outcome: TurnOutcome::Completed | TurnOutcome::ToolCompleted,
                ..
            })) => {}
            Ok(Some(TurnCompletion {
                outcome: TurnOutcome::WaitingInteraction | TurnOutcome::StepLimit,
                ..
            }))
            | Ok(None) => self.pause_inputs(),
            Err(error) => {
                self.input_driver_error = Some(Arc::new(error));
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
        if let Some(previous) = self
            .state
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
        if self.active_input.as_deref() == Some(id) {
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
        self.active_input = Some(record.input.id);
        let active = self.interrupt.activate(&request.cancellation);
        let result = self
            .run_turn(TurnInput {
                turn_id: request.turn_id,
                attempt_prefix: request.attempt_prefix,
                content: record.input.context,
                max_model_steps: request.max_model_steps,
                cancellation: active.token.clone(),
            })
            .await;
        self.active_input = None;
        self.publish_snapshot();
        result.map(Some)
    }

    /// Stages consumption just before request publication; no independently published watermark.
    pub(super) fn steering_context(&self, turn_id: &str) -> (Vec<ContextRecord>, Vec<String>) {
        let mut records = Vec::new();
        let mut ids = Vec::new();
        for input in self.state.inputs.iter().filter(|input| input.state == InputState::Pending
            && matches!(&input.delivery, InputDelivery::CurrentTurn { turn_id: target } if target == turn_id)
            && self.active_input.as_deref() != Some(input.input.id.as_str())) {
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
        let Some(id) = self.active_input.as_ref().cloned() else {
            return Ok(());
        };
        let previous = self
            .state
            .inputs
            .iter()
            .find(|record| record.input.id == id)
            .ok_or(ThreadError::InvalidIdentity)?;
        match &previous.state {
            InputState::Pending => {
                self.change_input(
                    &id,
                    InputState::Consumed {
                        turn_id: turn_id.to_owned(),
                        attempt_id: attempt_id.to_owned(),
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
    commit: &journal::ThreadCommit,
) -> Result<(), ThreadError> {
    let mut inputs = state.inputs.to_vec();
    let mut changes = state.input_changes.to_vec();
    let mut identities = std::collections::BTreeSet::new();
    let mut consumed_input = None;
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
            if !steering && consumed_input.replace(record.ordinal).is_some() {
                return Err(ThreadError::InvalidOutput);
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
            if !record.input.context.is_empty() {
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
    state.inputs = inputs.into();
    state.input_changes = changes.into();
    Ok(())
}

fn steering_record_id(turn_id: &str, input_id: &str) -> String {
    format!("steer:{}:{turn_id}{input_id}", turn_id.len())
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
    let ordinal = u64::try_from(state.inputs.len())
        .ok()
        .and_then(|count| count.checked_add(1))
        .ok_or(ThreadError::RevisionExhausted)?;
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
            max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
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
                max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
            })
            .await
            .unwrap();
        let mut subscription = thread.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let snapshot = subscription.next().await.expect("owner stays live");
                if matches!(snapshot.inputs[1].state, InputState::Consumed { .. }) {
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
        let snapshot = thread.snapshot();
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
        let history = thread.journal().await.unwrap();
        assert_eq!(journal::replay(&history).unwrap().inputs, snapshot.inputs);
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
        let mut corrupt = history.clone();
        let index = consumption.sequence as usize - 1;
        let mut commit = (*corrupt[index]).clone();
        let mut changes = commit.inputs.to_vec();
        if let InputChange::Transition {
            state: InputState::Consumed { attempt_id, .. },
            ..
        } = &mut changes[0]
        {
            *attempt_id = "not-admitted".into();
        } else {
            panic!("expected a consumed input transition");
        }
        commit.inputs = changes.into();
        corrupt[index] = Arc::new(commit);
        assert!(journal::replay(&corrupt).is_err());
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
            thread.snapshot().turns[0].input_id.as_deref(),
            Some("first")
        );
        assert!(thread.snapshot().inputs[0].context_record_id().is_none());
        thread.close().await.unwrap();
        let restored = ThreadHandle::restore(
            "queue".into(),
            DynModelSession::new(Probe {
                executed: executed.clone(),
                preparing: Arc::new(Notify::new()),
                release: None,
            }),
            thread.journal().await.unwrap(),
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
        let snapshot = restored.snapshot();
        assert_eq!(snapshot.turns[0].input_id.as_deref(), Some("first"));
        assert_eq!(snapshot.turns[1].input_id.as_deref(), Some("first"));
        let context_id = snapshot.inputs[0].context_record_id().unwrap();
        let context = snapshot
            .context
            .records
            .iter()
            .find(|record| record.id == context_id)
            .unwrap();
        assert_eq!(context.content, submitted("first").context);
        assert_eq!(restored.snapshot().inputs[1].state, InputState::Pending);
        restored.discard_input("second".into()).await.unwrap();
        assert_eq!(restored.snapshot().inputs[1].state, InputState::Discarded);
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
            max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
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
        thread.journal().await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(thread.snapshot().inputs[1].state, InputState::Pending);
        assert_eq!(thread.snapshot().inputs[2].state, InputState::Pending);
        thread.resume_inputs(options).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let snapshot = subscription.next().await.unwrap();
                if snapshot.attempts.len() == 2
                    && matches!(snapshot.input_execution, InputExecution::Failed { .. })
                {
                    break;
                }
            }
        })
        .await
        .expect("explicit resume drives one new request then pauses again");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(thread.snapshot().inputs[2].state, InputState::Pending);
        thread.close().await.unwrap();
    }
}
