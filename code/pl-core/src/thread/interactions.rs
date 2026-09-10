//! Owner-mediated interaction facts; content formats and rendering belong to the host.
use super::*;

/// An external question presented without interpreting its business payload.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    pub id: String,
    pub turn_id: String,
    pub payload: OpaquePayload,
}

/// Exact response facts supplied by the host. Context remains low-authority runtime content.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResponse {
    pub payload: OpaquePayload,
    pub context: Vec<ContextContent>,
}

/// Interaction transitions are explicit commands, never inferred from payload keys.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum InteractionState {
    Pending,
    Resolved(InteractionResponse),
    Cancelled,
}

/// Canonical interaction state with a monotonic owner revision.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRecord {
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub continuation_id: Option<String>,
    pub request: InteractionRequest,
    pub revision: u64,
    pub state: InteractionState,
    pub extension_mutations: Vec<super::extensions::ExtensionMutation>,
}

/// Host-decoded interaction response and state mutations forming one atomic checkpoint.
#[derive(Debug, Clone)]
pub struct InteractionResolution {
    pub continuation: Option<super::input::ThreadInput>,
    pub id: String,
    pub expected_revision: u64,
    pub response: InteractionResponse,
    pub mutations: Vec<super::extensions::ExtensionMutation>,
}

/// Explicit cancellation of one pending interaction at its observed revision.
#[derive(Debug, Clone)]
pub struct InteractionCancellation {
    pub id: String,
    pub expected_revision: u64,
}

impl Owner {
    pub(super) fn request_interaction(
        &mut self,
        request: InteractionRequest,
    ) -> Result<InteractionRecord, ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open {
            return Err(ThreadError::Closed);
        }
        if request.id.is_empty() || request.turn_id.is_empty() {
            return Err(ThreadError::InvalidIdentity);
        }
        if let Some(record) = self.state.interactions.get(&request.id) {
            return if record.request == request {
                Ok(record.clone())
            } else {
                Err(ThreadError::InvalidIdentity)
            };
        }
        let now = crate::time::unix_seconds();
        let record = InteractionRecord {
            created_at: now,
            updated_at: now,
            continuation_id: None,
            request,
            revision: 1,
            state: InteractionState::Pending,
            extension_mutations: Vec::new(),
        };
        self.state
            .interactions
            .insert(record.request.id.clone(), record.clone());
        let mut changes = self.state.interaction_changes.to_vec();
        changes.push(record.clone());
        self.state.interaction_changes = changes.into();
        self.publish();
        Ok(record)
    }

    pub(super) fn cancel_interaction(
        &mut self,
        cancellation: InteractionCancellation,
    ) -> Result<InteractionRecord, ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open {
            return Err(ThreadError::Closed);
        }
        self.settle_interaction_cancellation(cancellation)
    }

    pub(super) fn settle_interaction_cancellation(
        &mut self,
        cancellation: InteractionCancellation,
    ) -> Result<InteractionRecord, ThreadError> {
        if !self.pending.is_empty() {
            return Err(ThreadError::PendingTools);
        }
        let previous = self
            .state
            .interactions
            .get(&cancellation.id)
            .ok_or(ThreadError::InvalidIdentity)?;
        if previous.state == InteractionState::Cancelled
            && cancellation.expected_revision.checked_add(1) == Some(previous.revision)
        {
            return Ok(previous.clone());
        }
        if previous.revision != cancellation.expected_revision
            || previous.state != InteractionState::Pending
        {
            return Err(ThreadError::InvalidIdentity);
        }
        let record = InteractionRecord {
            created_at: previous.created_at,
            updated_at: crate::time::unix_seconds(),
            continuation_id: None,
            request: previous.request.clone(),
            revision: previous
                .revision
                .checked_add(1)
                .ok_or(ThreadError::RevisionExhausted)?,
            state: InteractionState::Cancelled,
            extension_mutations: Vec::new(),
        };
        let mut candidate = self.state.clone();
        stage_terminal_context(
            &mut candidate,
            &record,
            vec![ContextContent::Text {
                text: Arc::from("The interaction was cancelled without an answer."),
            }],
        )?;
        self.state = candidate;
        self.publish();
        Ok(record)
    }

    pub(super) fn resolve_interaction(
        &mut self,
        resolution: InteractionResolution,
    ) -> Result<InteractionRecord, ThreadError> {
        if !self.pending.is_empty() {
            return Err(ThreadError::PendingTools);
        }
        let mut candidate = self.state.clone();
        let record = stage_resolution(&mut candidate, resolution)?;
        self.state = candidate;
        self.publish();
        Ok(record)
    }
}

fn stage_resolution(
    state: &mut ThreadSnapshot,
    resolution: InteractionResolution,
) -> Result<InteractionRecord, ThreadError> {
    let InteractionResolution {
        continuation,
        id,
        expected_revision,
        response,
        mutations,
    } = resolution;
    if state.lifecycle != ThreadLifecycle::Open {
        return Err(ThreadError::Closed);
    }
    let continuation_id = continuation.as_ref().map(|input| input.id.clone());
    if continuation_id.as_ref().is_some_and(|input_id| {
        state.interactions.iter().any(|(other_id, record)| {
            other_id != &id && record.continuation_id.as_ref() == Some(input_id)
        })
    }) {
        return Err(ThreadError::InvalidIdentity);
    }
    let previous = state
        .interactions
        .get(&id)
        .ok_or(ThreadError::InvalidIdentity)?;
    if let InteractionState::Resolved(existing) = &previous.state
        && existing == &response
        && previous.extension_mutations == mutations
        && previous.continuation_id == continuation_id
        && expected_revision.checked_add(1) == Some(previous.revision)
    {
        let record = previous.clone();
        if let Some(input) = continuation {
            super::input::stage_input(state, input)?;
        }
        return Ok(record);
    }
    if previous.revision != expected_revision || previous.state != InteractionState::Pending {
        return Err(ThreadError::InvalidIdentity);
    }
    let revision = previous
        .revision
        .checked_add(1)
        .ok_or(ThreadError::RevisionExhausted)?;
    let record = InteractionRecord {
        created_at: previous.created_at,
        updated_at: crate::time::unix_seconds(),
        continuation_id,
        request: previous.request.clone(),
        revision,
        state: InteractionState::Resolved(response.clone()),
        extension_mutations: mutations.clone(),
    };
    super::extensions::stage_extensions(state, mutations)?;
    if let Some(input) = continuation
        && super::input::stage_input(state, input)?.state != super::input::InputState::Pending
    {
        return Err(ThreadError::InvalidIdentity);
    }
    stage_terminal_context(state, &record, response.context)?;
    Ok(record)
}

fn stage_terminal_context(
    state: &mut ThreadSnapshot,
    record: &InteractionRecord,
    content: Vec<ContextContent>,
) -> Result<(), ThreadError> {
    let id = &record.request.id;
    let revision = record.revision;
    let context_revision = state
        .context
        .revision
        .checked_add(1)
        .ok_or(ThreadError::RevisionExhausted)?;
    let mut records = state.context.records.to_vec();
    records.push(ContextRecord {
        id: format!("interaction:{id}:{revision}"),
        turn_id: Some(record.request.turn_id.clone()),
        source: ContextSource::Runtime {
            source_id: format!("interaction:{id}"),
        },
        content,
        tool_calls: Vec::new(),
    });
    let context = ContextSnapshot {
        revision: context_revision,
        records: records.into(),
    };
    context.validate_complete()?;
    state.context = context;
    state.interactions.insert(id.clone(), record.clone());
    let mut changes = state.interaction_changes.to_vec();
    changes.push(record.clone());
    state.interaction_changes = changes.into();
    Ok(())
}
