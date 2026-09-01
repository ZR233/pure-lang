use super::super::host::{PersistenceClass, initial_transcript_mutation};
use super::super::{
    AgentCommand, AgentFaultClassification, AgentIdentity, AgentSnapshotTransition,
    ThreadActorState,
};
use super::*;

enum SpawnCompensation {
    RolledBack,
    Faulted { reason: String },
}

pub(super) async fn register_agent<H>(
    host: &H,
    runtime: &AgentRuntimeHandle,
    actors: &AgentRegistry,
    registration: AgentRegistration,
    options: AgentRuntimeOptions,
) -> AgentRuntimeResult<AgentSnapshot>
where
    H: AgentRuntimeHost,
{
    let id = registration.identity.id.clone();
    if actors.read().await.contains_key(&id) {
        return Err(AgentRuntimeError::AlreadyExists(id));
    }
    let state = registration.into_durable_state();
    let event = AgentRuntimeEvent {
        agent_id: id.clone(),
        sequence: state.snapshot.event_sequence,
        created_at: unix_timestamp(),
        kind: AgentRuntimeEventKind::Registered {
            snapshot: Box::new(state.snapshot.clone()),
        },
    };
    host.repository()
        .commit(ThreadCommit {
            agent_id: id.clone(),
            persistence: PersistenceClass::Standard,
            expected_revision: None,
            next_state: state.clone(),
            facts: DurableCommitFacts::from_state(
                &state,
                vec![event.clone()],
                Vec::new(),
                None,
                initial_transcript_mutation(state.session.session.items()),
            ),
            mutation: super::super::ThreadMutation::SnapshotAndQueue,
        })
        .await
        .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
    let mut thread_snapshot = pl_protocol::ThreadSnapshot::empty(id.as_str());
    thread_snapshot.revision = state.session.thread_revision;
    runtime
        .thread_events
        .replace_snapshot(thread_snapshot)
        .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
    runtime.directory.publish_runtime_event(&event);
    host.observer()
        .publish(AgentCommittedEvent::runtime(event))
        .await;
    let actor = spawn_agent_loop(
        host.clone(),
        state.clone(),
        runtime.clone(),
        options.cancel_grace,
        true,
        options.command_capacity,
    );
    actors.write().await.insert(id, actor);
    Ok(state.snapshot)
}

pub(super) async fn spawn_child_agent<H>(
    host: &H,
    runtime: &AgentRuntimeHandle,
    actors: &AgentRegistry,
    request: AgentSpawnRequest,
    options: AgentRuntimeOptions,
) -> AgentRuntimeResult<AgentSpawnResult>
where
    H: AgentRuntimeHost,
{
    let parent = snapshot_for(actors, &request.parent_id).await?;
    if !parent.state.is_accepting_work() {
        return Err(AgentRuntimeError::NotActive(
            request.parent_id,
            parent.state,
        ));
    }
    let child_id = request.thread_id.clone();
    if actors.read().await.contains_key(&child_id) {
        return Err(AgentRuntimeError::AlreadyExists(child_id));
    }
    let identity = AgentIdentity {
        id: child_id.clone(),
        parent_id: Some(parent.identity.id.clone()),
        role: request.role,
        depth: parent.identity.depth.saturating_add(1),
    };
    let mut state = AgentRegistration {
        identity,
        session: request.session.clone(),
        runtime_revision: 1,
        event_sequence: 1,
    }
    .into_durable_state();
    let child_thread_id = child_id.clone();
    let metadata = request.metadata.clone();
    let initial_turn_id = if let Some(message) = request.initial_message {
        let turn_id = request.initial_turn_id.unwrap_or_else(TurnId::generate);
        state.pending_inputs.push_back(DurableMailboxEnvelope {
            mail_id: format!("mail:{turn_id}"),
            turn_id: turn_id.clone(),
            thread_id: child_thread_id.clone(),
            payload: super::super::MailboxInputPayload {
                message,
                attachments: Vec::new(),
                presentation: super::super::MailboxPresentation::Hidden,
                metadata: request.metadata.into(),
            },
            queue_coalescing_key: None,
            budget_action: super::super::MailboxBudgetAction::Preserve,
            delivery_state: Default::default(),
            queued_at: unix_timestamp(),
        });
        state.refresh_mailbox_snapshot();
        state
            .snapshot
            .transition(AgentCommand::Queue {
                turn_id: turn_id.clone(),
            })
            .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        Some(turn_id)
    } else {
        None
    };
    let lease = host
        .lifecycle()
        .prepare_spawn(SpawnLifecycleRequest {
            parent,
            child: state.snapshot.clone(),
            child_thread_id,
            agent_profile: state.session.session.agent_profile().cloned(),
            metadata,
        })
        .await
        .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
    let workspace_assignment = match host.lifecycle().workspace_assignment(&lease) {
        Ok(assignment) => assignment,
        Err(error) => {
            let reason = SpawnRollbackReason {
                phase: SpawnRollbackPhase::InitialContext,
                message: error.to_string(),
            };
            return match host.lifecycle().rollback_spawn(lease, reason).await {
                Ok(()) => Err(AgentRuntimeError::Lifecycle(error.to_string())),
                Err(rollback_error) => Err(AgentRuntimeError::Lifecycle(format!(
                    "spawn workspace assignment failed: {error}; spawn rollback failed: {rollback_error}"
                ))),
            };
        }
    };
    state
        .session
        .session
        .replace_workspace_assignment(workspace_assignment.clone());
    let initial_context = match host.lifecycle().initial_context(&lease) {
        Ok(context) => context,
        Err(error) => {
            let reason = SpawnRollbackReason {
                phase: SpawnRollbackPhase::InitialContext,
                message: error.to_string(),
            };
            return match host.lifecycle().rollback_spawn(lease, reason).await {
                Ok(()) => Err(AgentRuntimeError::Lifecycle(error.to_string())),
                Err(rollback_error) => Err(AgentRuntimeError::Lifecycle(format!(
                    "spawn context preparation failed: {error}; spawn rollback failed: {rollback_error}"
                ))),
            };
        }
    };
    for section in initial_context {
        state.session.session.upsert_pinned_context(section);
    }
    let event = AgentRuntimeEvent {
        agent_id: child_id.clone(),
        sequence: state.snapshot.event_sequence,
        created_at: unix_timestamp(),
        kind: AgentRuntimeEventKind::Registered {
            snapshot: Box::new(state.snapshot.clone()),
        },
    };
    let persisted = host
        .repository()
        .commit(ThreadCommit {
            agent_id: child_id.clone(),
            persistence: PersistenceClass::Standard,
            expected_revision: None,
            next_state: state.clone(),
            facts: DurableCommitFacts::from_state(
                &state,
                vec![event.clone()],
                Vec::new(),
                None,
                initial_transcript_mutation(state.session.session.items()),
            ),
            mutation: super::super::ThreadMutation::SnapshotAndQueue,
        })
        .await;
    match persisted {
        Ok(()) => {}
        Err(error) => {
            let reason = SpawnRollbackReason {
                phase: SpawnRollbackPhase::AgentRegistration,
                message: error.to_string(),
            };
            return match host.lifecycle().rollback_spawn(lease, reason).await {
                Ok(()) => Err(AgentRuntimeError::Repository(error.to_string())),
                Err(rollback_error) => Err(AgentRuntimeError::Lifecycle(format!(
                    "agent registration failed: {error}; spawn rollback failed: {rollback_error}"
                ))),
            };
        }
    }
    let mut thread_snapshot = pl_protocol::ThreadSnapshot::empty(child_id.as_str());
    thread_snapshot.revision = state.session.thread_revision;
    if let Err(error) = runtime.thread_events.replace_snapshot(thread_snapshot) {
        let rollback_reason = SpawnRollbackReason {
            phase: SpawnRollbackPhase::AgentRegistration,
            message: error.to_string(),
        };
        let rollback = host
            .lifecycle()
            .rollback_spawn(lease, rollback_reason)
            .await;
        let (reason, compensation) = match rollback {
            Ok(()) => (error.to_string(), SpawnCompensation::RolledBack),
            Err(rollback_error) => {
                let reason = format!("{error}; spawn rollback failed: {rollback_error}");
                (reason.clone(), SpawnCompensation::Faulted { reason })
            }
        };
        let compensated = persist_spawn_compensation(host, runtime, state, compensation).await?;
        let actor = spawn_agent_loop(
            host.clone(),
            compensated,
            runtime.clone(),
            options.cancel_grace,
            true,
            options.command_capacity,
        );
        actors.write().await.insert(child_id, actor);
        return Err(AgentRuntimeError::ThreadEvents(reason));
    }
    if let Err(error) = host.lifecycle().activate_spawn(&lease).await {
        let rollback_reason = SpawnRollbackReason {
            phase: SpawnRollbackPhase::Activation,
            message: error.to_string(),
        };
        let rollback = host
            .lifecycle()
            .rollback_spawn(lease, rollback_reason)
            .await;
        let (reason, compensation) = match rollback {
            Ok(()) => (error.to_string(), SpawnCompensation::RolledBack),
            Err(rollback_error) => {
                let reason = format!("{error}; spawn rollback failed: {rollback_error}");
                (reason.clone(), SpawnCompensation::Faulted { reason })
            }
        };
        let compensated = persist_spawn_compensation(host, runtime, state, compensation).await?;
        let actor = spawn_agent_loop(
            host.clone(),
            compensated,
            runtime.clone(),
            options.cancel_grace,
            true,
            options.command_capacity,
        );
        actors.write().await.insert(child_id, actor);
        return Err(AgentRuntimeError::Lifecycle(reason));
    }
    runtime.directory.publish_runtime_event(&event);
    host.observer()
        .publish(AgentCommittedEvent::runtime(event))
        .await;
    let actor = spawn_agent_loop(
        host.clone(),
        state.clone(),
        runtime.clone(),
        options.cancel_grace,
        true,
        options.command_capacity,
    );
    actors.write().await.insert(child_id, actor);
    Ok(AgentSpawnResult {
        snapshot: state.snapshot,
        initial_turn_id,
        workspace_assignment,
    })
}

async fn persist_spawn_compensation<H>(
    host: &H,
    runtime: &AgentRuntimeHandle,
    mut state: ThreadActorState,
    compensation: SpawnCompensation,
) -> AgentRuntimeResult<ThreadActorState>
where
    H: AgentRuntimeHost,
{
    let expected_revision = state.snapshot.revision;
    state.snapshot.revision = expected_revision.saturating_add(1);
    state.snapshot.event_sequence = state.snapshot.event_sequence.saturating_add(1);
    match &compensation {
        SpawnCompensation::RolledBack => {
            state.pending_inputs.clear();
            state.active_input = None;
            state.refresh_mailbox_snapshot();
            state
                .snapshot
                .transition(AgentCommand::BeginClose)
                .and_then(|_| state.snapshot.transition(AgentCommand::Close))
                .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        }
        SpawnCompensation::Faulted { reason } => {
            state
                .snapshot
                .transition(AgentCommand::Fault {
                    error: pl_protocol::StateError {
                        code: "agentSpawnCompensationFailed".to_string(),
                        message: reason.clone(),
                        retryable: false,
                    },
                    turn_id: None,
                    classification: AgentFaultClassification::AggregateCorruption,
                })
                .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        }
    }
    state.snapshot.updated_at = unix_timestamp();
    let event = AgentRuntimeEvent {
        agent_id: state.snapshot.identity.id.clone(),
        sequence: state.snapshot.event_sequence,
        created_at: state.snapshot.updated_at,
        kind: match compensation {
            SpawnCompensation::RolledBack => AgentRuntimeEventKind::StateChanged {
                snapshot: Box::new(state.snapshot.clone()),
            },
            SpawnCompensation::Faulted { reason } => AgentRuntimeEventKind::Faulted {
                reason,
                snapshot: Box::new(state.snapshot.clone()),
            },
        },
    };
    host.repository()
        .commit(ThreadCommit {
            agent_id: state.snapshot.identity.id.clone(),
            persistence: PersistenceClass::Settlement,
            expected_revision: Some(expected_revision),
            next_state: state.clone(),
            facts: DurableCommitFacts::from_state(
                &state,
                vec![event.clone()],
                Vec::new(),
                None,
                None,
            ),
            mutation: super::super::ThreadMutation::SnapshotAndQueue,
        })
        .await
        .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
    runtime.directory.store_snapshot(state.snapshot.clone());
    host.observer()
        .publish(AgentCommittedEvent::runtime(event))
        .await;
    Ok(state)
}
