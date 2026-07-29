use super::super::{AgentActivityState, AgentDurableState, AgentIdentity, AgentLifecycleState};
use super::*;

enum SpawnCompensation {
    RolledBack,
    Faulted { reason: String },
}

pub(super) async fn register_agent<H>(
    host: &H,
    runtime: &AgentRuntimeHandle,
    actors: &mut BTreeMap<AgentId, AgentActorHandle>,
    registration: AgentRegistration,
    options: AgentRuntimeOptions,
) -> AgentRuntimeResult<AgentSnapshot>
where
    H: AgentRuntimeHost,
{
    let id = registration.identity.id.clone();
    if actors.contains_key(&id) {
        return Err(AgentRuntimeError::AlreadyExists(id));
    }
    let state = registration.into_durable_state();
    let event = AgentRuntimeEvent {
        agent_id: id.clone(),
        sequence: state.snapshot.event_sequence,
        created_at: unix_timestamp(),
        kind: AgentRuntimeEventKind::Registered {
            snapshot: state.snapshot.clone(),
        },
    };
    let outcome = host
        .repository()
        .commit(AgentCommit {
            agent_id: id.clone(),
            expected_revision: None,
            next_state: state.clone(),
            events: vec![event.clone()],
            trace_events: Vec::new(),
            session_projection: None,
            mutation: super::super::AgentStateMutation::SnapshotAndQueue,
        })
        .await
        .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
    match outcome {
        AgentCommitOutcome::Applied => {}
        AgentCommitOutcome::RevisionConflict { actual_revision } => {
            return Err(AgentRuntimeError::RevisionConflict {
                expected: None,
                actual: actual_revision,
            });
        }
    }
    runtime.agent_events.publish_runtime_event(&event);
    host.observer()
        .publish(AgentCommittedEvent::runtime(event))
        .await;
    actors.insert(
        id,
        spawn_agent_actor(
            host.clone(),
            state.clone(),
            runtime.clone(),
            options.cancel_grace,
            true,
            options.command_capacity,
        ),
    );
    Ok(state.snapshot)
}

pub(super) async fn spawn_child_agent<H>(
    host: &H,
    runtime: &AgentRuntimeHandle,
    actors: &mut BTreeMap<AgentId, AgentActorHandle>,
    request: AgentSpawnRequest,
    options: AgentRuntimeOptions,
) -> AgentRuntimeResult<AgentSpawnResult>
where
    H: AgentRuntimeHost,
{
    let parent = snapshot_for(actors, &request.parent_id).await?;
    if parent.lifecycle != AgentLifecycleState::Active {
        return Err(AgentRuntimeError::NotActive(
            request.parent_id,
            parent.lifecycle,
        ));
    }
    let child_id = AgentId::generate();
    let identity = AgentIdentity {
        id: child_id.clone(),
        parent_id: Some(parent.identity.id.clone()),
        role: request.role,
        depth: parent.identity.depth.saturating_add(1),
    };
    let mut state = AgentRegistration {
        identity,
        wake_policy: request.wake_policy,
        sessions: vec![request.session.clone()],
    }
    .into_durable_state();
    let child_session_id = request.session.id.clone();
    let metadata = request.metadata.clone();
    let initial_turn_id = request.initial_message.map(|message| {
        let turn_id = TurnId::generate();
        state.pending_inputs.push_back(PendingAgentInput {
            mail_id: format!("mail:{turn_id}"),
            turn_id: turn_id.clone(),
            wake_id: None,
            wake_signal_ids: Vec::new(),
            session_id: child_session_id.clone(),
            message,
            metadata: request.metadata,
            trigger: super::super::MailboxTurnTrigger::StartIfIdle,
            delivery_state: Default::default(),
            dispatch_generation: state.snapshot.dispatch_generation,
            queued_at: unix_timestamp(),
        });
        state.refresh_mailbox_snapshot();
        state.snapshot.activity = AgentActivityState::Queued;
        turn_id
    });
    let lease = host
        .lifecycle()
        .prepare_spawn(SpawnLifecycleRequest {
            parent,
            child: state.snapshot.clone(),
            child_session_id,
            metadata,
        })
        .await
        .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
    let event = AgentRuntimeEvent {
        agent_id: child_id.clone(),
        sequence: state.snapshot.event_sequence,
        created_at: unix_timestamp(),
        kind: AgentRuntimeEventKind::Registered {
            snapshot: state.snapshot.clone(),
        },
    };
    let persisted = host
        .repository()
        .commit(AgentCommit {
            agent_id: child_id.clone(),
            expected_revision: None,
            next_state: state.clone(),
            events: vec![event.clone()],
            trace_events: Vec::new(),
            session_projection: None,
            mutation: super::super::AgentStateMutation::SnapshotAndQueue,
        })
        .await;
    let outcome = match persisted {
        Ok(outcome) => outcome,
        Err(error) => {
            return match host.lifecycle().rollback_spawn(lease).await {
                Ok(()) => Err(AgentRuntimeError::Repository(error.to_string())),
                Err(rollback_error) => Err(AgentRuntimeError::Lifecycle(format!(
                    "agent registration failed: {error}; spawn rollback failed: {rollback_error}"
                ))),
            };
        }
    };
    if let AgentCommitOutcome::RevisionConflict { actual_revision } = outcome {
        let conflict = AgentRuntimeError::RevisionConflict {
            expected: None,
            actual: actual_revision,
        };
        return match host.lifecycle().rollback_spawn(lease).await {
            Ok(()) => Err(conflict),
            Err(rollback_error) => Err(AgentRuntimeError::Lifecycle(format!(
                "{conflict}; spawn rollback failed: {rollback_error}"
            ))),
        };
    }
    if let Err(error) = host.lifecycle().activate_spawn(&lease).await {
        let rollback = host.lifecycle().rollback_spawn(lease).await;
        let (reason, compensation) = match rollback {
            Ok(()) => (error.to_string(), SpawnCompensation::RolledBack),
            Err(rollback_error) => {
                let reason = format!("{error}; spawn rollback failed: {rollback_error}");
                (reason.clone(), SpawnCompensation::Faulted { reason })
            }
        };
        let compensated = persist_spawn_compensation(host, runtime, state, compensation).await?;
        actors.insert(
            child_id,
            spawn_agent_actor(
                host.clone(),
                compensated,
                runtime.clone(),
                options.cancel_grace,
                true,
                options.command_capacity,
            ),
        );
        return Err(AgentRuntimeError::Lifecycle(reason));
    }
    runtime.agent_events.publish_runtime_event(&event);
    host.observer()
        .publish(AgentCommittedEvent::runtime(event))
        .await;
    actors.insert(
        child_id,
        spawn_agent_actor(
            host.clone(),
            state.clone(),
            runtime.clone(),
            options.cancel_grace,
            true,
            options.command_capacity,
        ),
    );
    Ok(AgentSpawnResult {
        snapshot: state.snapshot,
        initial_turn_id,
    })
}

async fn persist_spawn_compensation<H>(
    host: &H,
    runtime: &AgentRuntimeHandle,
    mut state: AgentDurableState,
    compensation: SpawnCompensation,
) -> AgentRuntimeResult<AgentDurableState>
where
    H: AgentRuntimeHost,
{
    let expected_revision = state.snapshot.revision;
    state.snapshot.revision = expected_revision.saturating_add(1);
    state.snapshot.event_sequence = state.snapshot.event_sequence.saturating_add(1);
    state.snapshot.activity = AgentActivityState::Idle;
    state.snapshot.active_turn_id = None;
    state.snapshot.active_session_id = None;
    match &compensation {
        SpawnCompensation::RolledBack => {
            state.pending_inputs.clear();
            state.active_input = None;
            state.refresh_mailbox_snapshot();
            state.snapshot.lifecycle = AgentLifecycleState::Closed;
        }
        SpawnCompensation::Faulted { .. } => {
            state.snapshot.lifecycle = AgentLifecycleState::Faulted;
        }
    }
    state.snapshot.updated_at = unix_timestamp();
    let event = AgentRuntimeEvent {
        agent_id: state.snapshot.identity.id.clone(),
        sequence: state.snapshot.event_sequence,
        created_at: state.snapshot.updated_at,
        kind: match compensation {
            SpawnCompensation::RolledBack => AgentRuntimeEventKind::StateChanged {
                snapshot: state.snapshot.clone(),
            },
            SpawnCompensation::Faulted { reason } => AgentRuntimeEventKind::Faulted {
                reason,
                snapshot: state.snapshot.clone(),
            },
        },
    };
    let outcome = host
        .repository()
        .commit(AgentCommit {
            agent_id: state.snapshot.identity.id.clone(),
            expected_revision: Some(expected_revision),
            next_state: state.clone(),
            events: vec![event.clone()],
            trace_events: Vec::new(),
            session_projection: None,
            mutation: super::super::AgentStateMutation::SnapshotAndQueue,
        })
        .await
        .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
    match outcome {
        AgentCommitOutcome::Applied => {
            runtime.agent_events.store_snapshot(state.snapshot.clone());
            host.observer()
                .publish(AgentCommittedEvent::runtime(event))
                .await;
            Ok(state)
        }
        AgentCommitOutcome::RevisionConflict { actual_revision } => {
            Err(AgentRuntimeError::RevisionConflict {
                expected: Some(expected_revision),
                actual: actual_revision,
            })
        }
    }
}
