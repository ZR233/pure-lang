//! Immutable Thread commit deltas. Model/tool execution never occurs while reading this journal.
use super::*;

/// Context mutation within an atomic Thread commit.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ContextChange {
    Append {
        revision: u64,
        records: Arc<[ContextRecord]>,
    },
    Replace(ContextSnapshot),
}

/// Request facts reference the input context revision already committed in the same journal.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptUpdate {
    #[serde(default)]
    pub request_metadata: Option<OpaquePayload>,
    #[serde(default)]
    pub tool_projection: Option<OpaquePayload>,
    pub turn_id: String,
    pub attempt_id: String,
    pub retry_of: Option<String>,
    pub input_revision: u64,
    pub tools: Arc<[ModelToolDeclaration]>,
    pub outcome: AttemptOutcome,
    pub input_estimate: Option<crate::model::TokenEstimate>,
}

/// Explicit private-material mutation. Missing mutation means unchanged.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum PrivateContextChange {
    Set(OpaquePayload),
    Clear,
}

/// One atomic delta containing context, output, continuation and lifecycle changes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCommit {
    /// Time this fact batch became canonical in memory, independent from asynchronous database flush.
    pub committed_at: i64,
    #[serde(default)]
    pub permissions: Arc<[permissions::PermissionRecord]>,
    #[serde(default)]
    pub wake_messages_through: Option<u64>,
    #[serde(default)]
    pub inputs: Arc<[input::InputChange]>,
    #[serde(default)]
    pub tasks: Arc<[task::TaskRecord]>,
    pub thread_id: String,
    pub sequence: u64,
    pub context: Option<ContextChange>,
    /// None means unchanged; Clear is preserved as an explicit serialized operation.
    pub private_context: Option<PrivateContextChange>,
    pub attempt: Option<AttemptUpdate>,
    pub turn: Option<TurnRecord>,
    pub discovered_tools: Option<Arc<[ModelToolDeclaration]>>,
    pub deliveries: Arc<[ToolDelivery]>,
    pub extensions: Arc<[extensions::ExtensionChange]>,
    pub inbox: Arc<[inbox::InboxRecord]>,
    pub consumed_messages: Option<u64>,
    pub interactions: Arc<[interactions::InteractionRecord]>,
    pub replacements: Arc<[ContextReplacement]>,
    pub runtime_facts: Option<Arc<[RuntimeFact]>>,
    pub lifecycle: Option<ThreadLifecycle>,
}

/// Invalid encoded journal data. Unknown outer formats are rejected without altering raw bytes.
#[derive(Debug, thiserror::Error)]
pub enum JournalCodecError {
    #[error("unsupported Thread commit format {format} version {version}")]
    Unsupported { format: String, version: u32 },
    #[error("invalid Thread commit encoding: {0}")]
    Encoding(#[from] serde_json::Error),
}

impl ThreadCommit {
    /// Freezes the actual commit as an opaque cold-store payload.
    ///
    /// # Errors
    /// Returns an encoding failure without changing the committed state.
    pub fn encode(&self) -> Result<OpaquePayload, JournalCodecError> {
        let content = serde_json::to_string(self)?;
        Ok(OpaquePayload::new("pl.core.thread-commit", 1, content)
            .expect("static nonempty format and nonzero version"))
    }

    /// Loads a framework commit without interpreting nested tool or model payload formats.
    ///
    /// # Errors
    /// Rejects an unsupported outer schema or invalid envelope encoding.
    pub fn decode(payload: &OpaquePayload) -> Result<Self, JournalCodecError> {
        if payload.format() != "pl.core.thread-commit" || payload.version() != 1 {
            return Err(JournalCodecError::Unsupported {
                format: payload.format().into(),
                version: payload.version(),
            });
        }
        Ok(serde_json::from_str(payload.content())?)
    }

    pub(super) fn between(
        thread_id: &str,
        previous: &ThreadSnapshot,
        current: &ThreadSnapshot,
        sequence: u64,
    ) -> Option<Self> {
        let context = if previous.context == current.context {
            None
        } else if current
            .context
            .records
            .starts_with(&previous.context.records)
        {
            Some(ContextChange::Append {
                revision: current.context.revision,
                records: current.context.records[previous.context.records.len()..]
                    .to_vec()
                    .into(),
            })
        } else {
            Some(ContextChange::Replace(current.context.clone()))
        };
        let private_context = (previous.private_context != current.private_context).then(|| {
            match &current.private_context {
                Some(payload) => PrivateContextChange::Set(payload.clone()),
                None => PrivateContextChange::Clear,
            }
        });
        let attempt = if Arc::ptr_eq(&previous.attempts, &current.attempts) {
            None
        } else {
            current.attempts.last().map(|attempt| AttemptUpdate {
                request_metadata: attempt.request_metadata.clone(),
                tool_projection: attempt.tool_projection.clone(),
                turn_id: attempt.turn_id.clone(),
                attempt_id: attempt.attempt_id.clone(),
                retry_of: attempt.retry_of.clone(),
                input_revision: attempt.input.revision,
                tools: attempt.tools.clone(),
                outcome: attempt.outcome.clone(),
                input_estimate: attempt.input_estimate,
            })
        };
        let inputs: Arc<[input::InputChange]> = current.input_changes
            [previous.input_changes.len()..]
            .to_vec()
            .into();
        let tasks: Arc<[task::TaskRecord]> = current.task_changes[previous.task_changes.len()..]
            .to_vec()
            .into();
        let deliveries: Arc<[ToolDelivery]> = current.deliveries[previous.deliveries.len()..]
            .to_vec()
            .into();
        let replacements: Arc<[ContextReplacement]> = current.context_replacements
            [previous.context_replacements.len()..]
            .to_vec()
            .into();
        let runtime_facts = (previous.runtime_facts != current.runtime_facts)
            .then(|| current.runtime_facts.clone());
        let extensions: Arc<[extensions::ExtensionChange]> = current.extension_changes
            [previous.extension_changes.len()..]
            .to_vec()
            .into();
        let inbox: Arc<[inbox::InboxRecord]> =
            current.inbox[previous.inbox.len()..].to_vec().into();
        let wake_messages_through = (previous.wake_messages_through
            != current.wake_messages_through)
            .then_some(current.wake_messages_through);
        let consumed_messages = (previous.consumed_messages != current.consumed_messages)
            .then_some(current.consumed_messages);
        let turn = (!Arc::ptr_eq(&previous.turns, &current.turns))
            .then(|| current.turns.last().cloned())
            .flatten();
        let permissions: Arc<[permissions::PermissionRecord]> = current.permission_changes
            [previous.permission_changes.len()..]
            .to_vec()
            .into();
        let interactions: Arc<[interactions::InteractionRecord]> = current.interaction_changes
            [previous.interaction_changes.len()..]
            .to_vec()
            .into();
        let discovered_tools = (previous.discovered_tools != current.discovered_tools)
            .then(|| current.discovered_tools.clone());
        let lifecycle = (previous.lifecycle != current.lifecycle).then_some(current.lifecycle);
        if context.is_none()
            && private_context.is_none()
            && attempt.is_none()
            && turn.is_none()
            && discovered_tools.is_none()
            && tasks.is_empty()
            && inputs.is_empty()
            && deliveries.is_empty()
            && replacements.is_empty()
            && runtime_facts.is_none()
            && lifecycle.is_none()
            && extensions.is_empty()
            && inbox.is_empty()
            && consumed_messages.is_none()
            && wake_messages_through.is_none()
            && permissions.is_empty()
            && interactions.is_empty()
        {
            return None;
        }
        Some(Self {
            committed_at: crate::time::unix_seconds(),
            wake_messages_through,
            inputs,
            tasks,
            thread_id: thread_id.to_owned(),
            sequence,
            context,
            private_context,
            attempt,
            turn,
            discovered_tools,
            deliveries,
            extensions,
            inbox,
            consumed_messages,
            permissions,
            interactions,
            replacements,
            runtime_facts,
            lifecycle,
        })
    }
}

/// Reconstructs published facts without creating executors, preparing models or running reducers.
///
/// # Errors
/// Rejects missing commit sequences and attempts referencing an unavailable context version.
pub fn replay(commits: &[Arc<ThreadCommit>]) -> Result<ThreadSnapshot, ThreadError> {
    let mut state = ThreadSnapshot::default();
    let mut contexts = std::collections::BTreeMap::from([(0, ContextSnapshot::default())]);
    let owner = commits.first().map(|commit| commit.thread_id.as_str());
    for (index, commit) in commits.iter().enumerate() {
        if commit.thread_id.is_empty() || Some(commit.thread_id.as_str()) != owner {
            return Err(ThreadError::InvalidIdentity);
        }
        if commit.sequence != index as u64 + 1 {
            return Err(ThreadError::InvalidContext);
        }
        if let Some(change) = &commit.context {
            let next = match change {
                ContextChange::Append { revision, records } => {
                    let mut next = state.context.records.to_vec();
                    next.extend(records.iter().cloned());
                    ContextSnapshot {
                        revision: *revision,
                        records: next.into(),
                    }
                }
                ContextChange::Replace(next) => next.clone(),
            };
            next.pending_calls()?;
            if next.revision
                != state
                    .context
                    .revision
                    .checked_add(1)
                    .ok_or(ThreadError::RevisionExhausted)?
            {
                return Err(ThreadError::InvalidContext);
            }
            contexts.insert(next.revision, next.clone());
            state.context = next;
        }
        if let Some(value) = &commit.private_context {
            state.private_context = match value {
                PrivateContextChange::Set(payload) => Some(payload.clone()),
                PrivateContextChange::Clear => None,
            };
        }
        if let Some(update) = &commit.attempt {
            let input = contexts
                .get(&update.input_revision)
                .ok_or(ThreadError::InvalidContext)?
                .clone();
            input.validate_complete()?;
            if update.attempt_id.is_empty() || update.turn_id.is_empty() {
                return Err(ThreadError::InvalidIdentity);
            }
            if let AttemptOutcome::Committed(output) = &update.outcome {
                if output.attempt_id != update.attempt_id
                    || output.base_context_revision != update.input_revision
                {
                    return Err(ThreadError::InvalidOutput);
                }
                if state.private_context != output.private_context {
                    return Err(ThreadError::InvalidOutput);
                }
                let record = state
                    .context
                    .records
                    .last()
                    .ok_or(ThreadError::InvalidOutput)?;
                if record.source != ContextSource::Assistant
                    || record.content != output.content
                    || record.tool_calls != output.tool_calls
                {
                    return Err(ThreadError::InvalidOutput);
                }
            }
            let attempt = RequestAttempt {
                request_metadata: update.request_metadata.clone(),
                tool_projection: update.tool_projection.clone(),
                turn_id: update.turn_id.clone(),
                attempt_id: update.attempt_id.clone(),
                retry_of: update.retry_of.clone(),
                input,
                tools: update.tools.clone(),
                outcome: update.outcome.clone(),
                input_estimate: update.input_estimate,
            };
            let mut attempts = state.attempts.to_vec();
            if let Some(previous) = attempts
                .iter_mut()
                .find(|previous| previous.attempt_id == update.attempt_id)
            {
                if !matches!(previous.outcome, AttemptOutcome::Running)
                    || matches!(attempt.outcome, AttemptOutcome::Running)
                    || previous.turn_id != attempt.turn_id
                    || previous.retry_of != attempt.retry_of
                    || previous.input != attempt.input
                    || previous.tools != attempt.tools
                    || previous.tool_projection != attempt.tool_projection
                    || previous.request_metadata != attempt.request_metadata
                    || previous.input_estimate != attempt.input_estimate
                {
                    return Err(ThreadError::InvalidOutput);
                }
                *previous = attempt;
            } else {
                if !matches!(attempt.outcome, AttemptOutcome::Running) {
                    return Err(ThreadError::InvalidOutput);
                }
                if let Some(source_id) = &attempt.retry_of {
                    let source = attempts
                        .last()
                        .filter(|source| &source.attempt_id == source_id)
                        .ok_or(ThreadError::InvalidIdentity)?;
                    if !matches!(
                        source.outcome,
                        AttemptOutcome::Failed(_) | AttemptOutcome::Cancelled { .. }
                    ) || source.turn_id != attempt.turn_id
                        || source.input != attempt.input
                        || source.tools != attempt.tools
                    {
                        return Err(ThreadError::InvalidContext);
                    }
                }
                attempts.push(attempt);
            }
            state.attempts = attempts.into();
        }
        if let Some(declarations) = &commit.discovered_tools {
            let mut ids = std::collections::BTreeSet::new();
            if declarations.iter().any(|declaration| {
                declaration.tool_id.is_empty() || !ids.insert(&declaration.tool_id)
            }) {
                return Err(ThreadError::InvalidIdentity);
            }
            state.discovered_tools = declarations.clone();
        }
        if let Some(turn) = &commit.turn {
            if turn.turn_id.is_empty() {
                return Err(ThreadError::InvalidIdentity);
            }
            let mut turns = state.turns.to_vec();
            if let Some(previous) = turns
                .iter_mut()
                .find(|previous| previous.turn_id == turn.turn_id)
            {
                if previous.input_id != turn.input_id
                    || previous.state != TurnState::Running
                    || turn.state == TurnState::Running
                    || turn.model_steps < previous.model_steps
                {
                    return Err(ThreadError::InvalidOutput);
                }
                *previous = turn.clone();
            } else {
                if turn.state != TurnState::Running || turn.model_steps != 0 {
                    return Err(ThreadError::InvalidOutput);
                }
                if turn.input_id.as_ref().is_some_and(|id| {
                    !state.inputs.iter().any(|input| {
                        &input.input.id == id && input.state == input::InputState::Pending
                    })
                }) {
                    return Err(ThreadError::InvalidOutput);
                }
                turns.push(turn.clone());
            }
            state.turns = turns.into();
        }
        let mut interaction_history = state.interaction_changes.to_vec();
        for record in commit.interactions.iter() {
            if record.continuation_id.is_some()
                && !matches!(record.state, interactions::InteractionState::Resolved(_))
            {
                return Err(ThreadError::InvalidOutput);
            }
            if record.request.id.is_empty() || record.request.turn_id.is_empty() {
                return Err(ThreadError::InvalidIdentity);
            }
            if let Some(previous) = state.interactions.get(&record.request.id) {
                if previous.request != record.request
                    || previous.created_at != record.created_at
                    || previous.state != interactions::InteractionState::Pending
                    || record.state == interactions::InteractionState::Pending
                    || previous.revision.checked_add(1) != Some(record.revision)
                {
                    return Err(ThreadError::InvalidOutput);
                }
            } else if record.revision != 1
                || record.state != interactions::InteractionState::Pending
            {
                return Err(ThreadError::InvalidOutput);
            }
            match &record.state {
                interactions::InteractionState::Pending => {
                    if !record.extension_mutations.is_empty() {
                        return Err(ThreadError::InvalidOutput);
                    }
                }
                interactions::InteractionState::Resolved(_)
                | interactions::InteractionState::Cancelled => {
                    let Some(ContextChange::Append { records, .. }) = &commit.context else {
                        return Err(ThreadError::InvalidOutput);
                    };
                    let id = &record.request.id;
                    let revision = record.revision;
                    let context = records
                        .iter()
                        .find(|context| context.id == format!("interaction:{id}:{revision}"))
                        .ok_or(ThreadError::InvalidOutput)?;
                    if context.turn_id.as_ref() != Some(&record.request.turn_id)
                        || context.source
                            != (ContextSource::Runtime {
                                source_id: format!("interaction:{id}"),
                            })
                        || !context.tool_calls.is_empty()
                    {
                        return Err(ThreadError::InvalidOutput);
                    }
                    match &record.state {
                        interactions::InteractionState::Resolved(response) => {
                            if context.content != response.context {
                                return Err(ThreadError::InvalidOutput);
                            }
                        }
                        interactions::InteractionState::Cancelled => {
                            if context.content.is_empty() || !record.extension_mutations.is_empty()
                            {
                                return Err(ThreadError::InvalidOutput);
                            }
                        }
                        interactions::InteractionState::Pending => {
                            return Err(ThreadError::InvalidOutput);
                        }
                    }
                }
            }
            state
                .interactions
                .insert(record.request.id.clone(), record.clone());
            interaction_history.push(record.clone());
        }
        state.interaction_changes = interaction_history.into();
        let mut inbox = state.inbox.to_vec();
        for record in commit.inbox.iter() {
            if record.sequence != inbox.len() as u64 + 1
                || record.message.id.is_empty()
                || record.message.source_id.is_empty()
                || inbox
                    .iter()
                    .any(|previous| previous.message.id == record.message.id)
            {
                return Err(ThreadError::InvalidIdentity);
            }
            inbox.push(record.clone());
        }
        state.inbox = inbox.into();
        if let Some(watermark) = commit.consumed_messages {
            if watermark < state.consumed_messages
                || watermark > state.inbox.len() as u64
                || !commit
                    .attempt
                    .as_ref()
                    .is_some_and(|attempt| matches!(attempt.outcome, AttemptOutcome::Running))
            {
                return Err(ThreadError::InvalidContext);
            }
            state.consumed_messages = watermark;
        }
        let mut extension_history = state.extension_changes.to_vec();
        for change in commit.extensions.iter() {
            let expected = state
                .extension_sequence
                .checked_add(1)
                .ok_or(ThreadError::RevisionExhausted)?;
            match change {
                extensions::ExtensionChange::Put { id, record } => {
                    if id.is_empty() || record.revision != expected {
                        return Err(ThreadError::InvalidContext);
                    }
                    state.extensions.insert(id.clone(), record.clone());
                }
                extensions::ExtensionChange::Delete { id, revision } => {
                    if *revision != expected || state.extensions.remove(id).is_none() {
                        return Err(ThreadError::InvalidContext);
                    }
                }
            }
            state.extension_sequence = expected;
            extension_history.push(change.clone());
        }
        state.extension_changes = extension_history.into();
        for delivery in commit.deliveries.iter() {
            if state
                .deliveries
                .iter()
                .any(|previous| previous.call_id == delivery.call_id)
            {
                return Err(ThreadError::InvalidOutput);
            }
            match &delivery.target {
                ToolDeliveryTarget::CallResult => {
                    let recorded = state.context.records.iter().find(|record| matches!(&record.source, ContextSource::ToolResult { call_id, tool_id } if call_id == &delivery.call_id && tool_id == &delivery.tool_id)).ok_or(ThreadError::InvalidOutput)?;
                    if recorded.content != delivery.delivered_context {
                        return Err(ThreadError::InvalidOutput);
                    }
                }
                ToolDeliveryTarget::Inbox { message_id } => {
                    let message = commit
                        .inbox
                        .iter()
                        .find(|record| &record.message.id == message_id)
                        .ok_or(ThreadError::InvalidOutput)?;
                    if message.message.source_id != format!("task:{}", delivery.call_id)
                        || message.message.context != delivery.delivered_context
                        || &message.message.payload != delivery.output.payload()
                    {
                        return Err(ThreadError::InvalidOutput);
                    }
                }
            }
        }
        let mut delivery_ids = std::collections::BTreeSet::new();
        if commit
            .deliveries
            .iter()
            .any(|delivery| !delivery_ids.insert(&delivery.call_id))
        {
            return Err(ThreadError::InvalidOutput);
        }
        let mut deliveries = state.deliveries.to_vec();
        deliveries.extend(commit.deliveries.iter().cloned());
        state.deliveries = deliveries.into();
        let mut replacements = state.context_replacements.to_vec();
        replacements.extend(commit.replacements.iter().cloned());
        state.context_replacements = replacements.into();
        if let Some(facts) = &commit.runtime_facts {
            state.runtime_facts = facts.clone();
        }
        if let Some(lifecycle) = commit.lifecycle {
            state.lifecycle = lifecycle;
        }
        if let Some(watermark) = commit.wake_messages_through {
            if watermark < state.wake_messages_through
                || watermark > state.inbox.last().map_or(0, |record| record.sequence)
            {
                return Err(ThreadError::InvalidOutput);
            }
            state.wake_messages_through = watermark;
        }
        input::replay(&mut state, commit)?;
        for record in commit.interactions.iter() {
            if let Some(input_id) = &record.continuation_id
                && (!state.inputs.iter().any(|input| &input.input.id == input_id)
                    || state.interactions.iter().any(|(id, other)| {
                        id != &record.request.id && other.continuation_id.as_ref() == Some(input_id)
                    }))
            {
                return Err(ThreadError::InvalidOutput);
            }
        }

        task::replay(&mut state, commit)?;
        for record in commit.permissions.iter() {
            permissions::replay(&mut state, record)?;
        }

        state.commit_sequence = commit.sequence;
    }
    Ok(state)
}

/// Produces restart settlement facts without constructing a model, tool or execution owner.
/// Already closed histories remain closed; reactivation is a separate explicit host operation.
///
/// # Errors
/// Rejects corrupt history or exhausted commit identities.
pub fn recovery_commit(commits: &[Arc<ThreadCommit>]) -> Result<Option<ThreadCommit>, ThreadError> {
    let Some(last) = commits.last() else {
        return Ok(None);
    };
    let before = replay(commits)?;
    if before.lifecycle == ThreadLifecycle::Closed {
        return Ok(None);
    }
    let after = super::recovery::settle(before.clone())?;
    let sequence = before
        .commit_sequence
        .checked_add(1)
        .ok_or(ThreadError::RevisionExhausted)?;
    Ok(ThreadCommit::between(
        &last.thread_id,
        &before,
        &after,
        sequence,
    ))
}
