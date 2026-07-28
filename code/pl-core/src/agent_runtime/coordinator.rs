use std::collections::BTreeMap;

use tokio::sync::{mpsc, oneshot};

use super::actor::{ActorCommand, AgentActorHandle, spawn_agent_actor};
use super::event_hub::AgentEventHubHandle;
use super::host::{AgentCommitObserver, AgentLifecycleAdapter, AgentStateRepository};
use super::runtime::{AgentRuntimeOptions, RestoredInputPolicy};
use super::state::{AgentRuntimeError, unix_timestamp};
use super::{
    AgentCommit, AgentCommitOutcome, AgentCommittedEvent, AgentCurrentSessionSubmitRequest,
    AgentId, AgentRegistration, AgentRuntimeEvent, AgentRuntimeEventKind, AgentRuntimeHandle,
    AgentRuntimeHost, AgentRuntimeResult, AgentSnapshot, AgentSpawnRequest, AgentSpawnResult,
    AgentSubmitRequest, PendingAgentInput, RestoredAgentRuntime, SpawnLifecycleRequest, TurnId,
};
use crate::SessionEventHub;

mod spawn;
mod waiting_agents;
use spawn::{register_agent, spawn_child_agent};
use waiting_agents::spawn_waiting_agents_supervisor;

pub(crate) enum CoordinatorCommand {
    Register {
        registration: AgentRegistration,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    Submit {
        agent_id: AgentId,
        request: AgentSubmitRequest,
        reply: oneshot::Sender<AgentRuntimeResult<TurnId>>,
    },
    SubmitCurrentSession {
        agent_id: AgentId,
        request: AgentCurrentSessionSubmitRequest,
        reply: oneshot::Sender<AgentRuntimeResult<TurnId>>,
    },
    Spawn {
        request: AgentSpawnRequest,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSpawnResult>>,
    },
    CancelTurn {
        agent_id: AgentId,
        turn_id: TurnId,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    SetActivity {
        agent_id: AgentId,
        turn_id: TurnId,
        activity: super::AgentActivityState,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Checkpoint {
        agent_id: AgentId,
        checkpoint: super::AgentTurnCheckpoint,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    OpenSession {
        agent_id: AgentId,
        session: super::AgentSessionState,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    RecordSessionFacts {
        agent_id: AgentId,
        session_id: super::SessionId,
        facts: Vec<crate::SessionEventFact>,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Close {
        agent_id: AgentId,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    Snapshot {
        agent_id: AgentId,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    WakeAccepted {
        agent_id: AgentId,
        wake_id: Option<super::AgentWakeId>,
        signal_ids: Vec<String>,
        reply: oneshot::Sender<AgentRuntimeResult<bool>>,
    },
    AcceptWakeSignals {
        agent_id: AgentId,
        turn_id: TurnId,
        signal_ids: Vec<String>,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    List {
        reply: oneshot::Sender<AgentRuntimeResult<Vec<AgentSnapshot>>>,
    },
    EnterWaitingAgents {
        agent_id: AgentId,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    StartRestoredInputs {
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Shutdown {
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
}

pub(crate) fn spawn_coordinator<H>(
    host: H,
    restored: Vec<RestoredAgentRuntime>,
    options: AgentRuntimeOptions,
    session_events: SessionEventHub,
) -> AgentRuntimeResult<AgentRuntimeHandle>
where
    H: AgentRuntimeHost,
{
    let (sender, receiver) = mpsc::channel(options.command_capacity.max(1));
    let agent_events =
        AgentEventHubHandle::new(restored.iter().map(|agent| agent.state.snapshot.clone()));
    let handle = AgentRuntimeHandle::new(sender, session_events.handle(), agent_events);
    let mut actors = BTreeMap::new();
    for restored_agent in restored {
        let id = restored_agent.state.snapshot.identity.id.clone();
        actors.insert(
            id,
            spawn_agent_actor(
                host.clone(),
                restored_agent.state,
                handle.clone(),
                options.cancel_grace,
                options.restored_inputs == RestoredInputPolicy::Start,
                options.command_capacity,
            ),
        );
    }
    tokio::spawn(run_coordinator(
        host,
        handle.clone(),
        actors,
        receiver,
        options,
    ));
    spawn_waiting_agents_supervisor(handle.clone(), options.child_inactivity_timeout);
    Ok(handle)
}

async fn run_coordinator<H>(
    host: H,
    runtime: AgentRuntimeHandle,
    mut actors: BTreeMap<AgentId, AgentActorHandle>,
    mut receiver: mpsc::Receiver<CoordinatorCommand>,
    options: AgentRuntimeOptions,
) where
    H: AgentRuntimeHost,
{
    while let Some(command) = receiver.recv().await {
        match command {
            CoordinatorCommand::Register {
                registration,
                reply,
            } => {
                let result =
                    register_agent(&host, &runtime, &mut actors, registration, options).await;
                let _ = reply.send(result);
            }
            CoordinatorCommand::Submit {
                agent_id,
                request,
                reply,
            } => {
                route(&actors, &agent_id, ActorCommand::Submit { request, reply }).await;
            }
            CoordinatorCommand::SubmitCurrentSession {
                agent_id,
                request,
                reply,
            } => {
                let root_agent_id = match root_agent_id_for(&actors, &agent_id).await {
                    Ok(root_agent_id) => root_agent_id,
                    Err(error) => {
                        let _ = reply.send(Err(error));
                        continue;
                    }
                };
                route(
                    &actors,
                    &agent_id,
                    ActorCommand::SubmitCurrentSession {
                        root_agent_id,
                        request,
                        reply,
                    },
                )
                .await;
            }
            CoordinatorCommand::Spawn { request, reply } => {
                let result =
                    spawn_child_agent(&host, &runtime, &mut actors, request, options).await;
                let _ = reply.send(result);
            }
            CoordinatorCommand::CancelTurn {
                agent_id,
                turn_id,
                reply,
            } => {
                route(
                    &actors,
                    &agent_id,
                    ActorCommand::CancelTurn { turn_id, reply },
                )
                .await;
            }
            CoordinatorCommand::SetActivity {
                agent_id,
                turn_id,
                activity,
                reply,
            } => {
                route(
                    &actors,
                    &agent_id,
                    ActorCommand::SetActivity {
                        turn_id,
                        activity,
                        reply,
                    },
                )
                .await;
            }
            CoordinatorCommand::Checkpoint {
                agent_id,
                checkpoint,
                reply,
            } => {
                route(
                    &actors,
                    &agent_id,
                    ActorCommand::Checkpoint { checkpoint, reply },
                )
                .await;
            }
            CoordinatorCommand::OpenSession {
                agent_id,
                session,
                reply,
            } => {
                route(
                    &actors,
                    &agent_id,
                    ActorCommand::OpenSession { session, reply },
                )
                .await;
            }
            CoordinatorCommand::RecordSessionFacts {
                agent_id,
                session_id,
                facts,
                reply,
            } => {
                route(
                    &actors,
                    &agent_id,
                    ActorCommand::RecordSessionFacts {
                        session_id,
                        facts,
                        reply,
                    },
                )
                .await;
            }
            CoordinatorCommand::Close { agent_id, reply } => {
                let result = close_agent_tree(&actors, &agent_id).await;
                let _ = reply.send(result);
            }
            CoordinatorCommand::Snapshot { agent_id, reply } => {
                route(&actors, &agent_id, ActorCommand::Snapshot { reply }).await;
            }
            CoordinatorCommand::WakeAccepted {
                agent_id,
                wake_id,
                signal_ids,
                reply,
            } => {
                route(
                    &actors,
                    &agent_id,
                    ActorCommand::WakeAccepted {
                        wake_id,
                        signal_ids,
                        reply,
                    },
                )
                .await;
            }
            CoordinatorCommand::AcceptWakeSignals {
                agent_id,
                turn_id,
                signal_ids,
                reply,
            } => {
                route(
                    &actors,
                    &agent_id,
                    ActorCommand::AcceptWakeSignals {
                        turn_id,
                        signal_ids,
                        reply,
                    },
                )
                .await;
            }
            CoordinatorCommand::EnterWaitingAgents { agent_id, reply } => {
                route(
                    &actors,
                    &agent_id,
                    ActorCommand::EnterWaitingAgents { reply },
                )
                .await;
            }
            CoordinatorCommand::StartRestoredInputs { reply } => {
                let _ = reply.send(start_pending_inputs(&actors).await);
            }
            CoordinatorCommand::List { reply } => {
                let _ = reply.send(list_snapshots(&actors).await);
            }
            CoordinatorCommand::Shutdown { reply } => {
                let result = shutdown_agents(&actors).await;
                actors.clear();
                let _ = reply.send(result);
                break;
            }
        }
    }
    for actor in actors.values() {
        let (reply, _receiver) = oneshot::channel();
        let _ = actor.send(ActorCommand::Shutdown { reply }).await;
    }
}

async fn shutdown_agents(actors: &BTreeMap<AgentId, AgentActorHandle>) -> AgentRuntimeResult<()> {
    let mut first_error = None;
    for actor in actors.values() {
        let (reply, receiver) = oneshot::channel();
        let result = match actor.send(ActorCommand::Shutdown { reply }).await {
            Ok(()) => receiver
                .await
                .map_err(|_| AgentRuntimeError::ChannelClosed)
                .and_then(|result| result),
            Err(error) => Err(error),
        };
        if first_error.is_none() {
            first_error = result.err();
        }
    }
    first_error.map_or(Ok(()), Err)
}

async fn start_pending_inputs(
    actors: &BTreeMap<AgentId, AgentActorHandle>,
) -> AgentRuntimeResult<()> {
    for actor in actors.values() {
        let (reply, receiver) = oneshot::channel();
        actor
            .send(ActorCommand::StartPendingInputs { reply })
            .await?;
        receiver
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)??;
    }
    Ok(())
}

async fn snapshot_for(
    actors: &BTreeMap<AgentId, AgentActorHandle>,
    agent_id: &AgentId,
) -> AgentRuntimeResult<AgentSnapshot> {
    let actor = actors
        .get(agent_id)
        .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))?;
    let (reply, receiver) = oneshot::channel();
    actor.send(ActorCommand::Snapshot { reply }).await?;
    receiver
        .await
        .map_err(|_| AgentRuntimeError::ChannelClosed)?
}

async fn close_agent_tree(
    actors: &BTreeMap<AgentId, AgentActorHandle>,
    agent_id: &AgentId,
) -> AgentRuntimeResult<AgentSnapshot> {
    let snapshots = list_snapshots(actors).await?;
    if !snapshots
        .iter()
        .any(|snapshot| snapshot.identity.id == *agent_id)
    {
        return Err(AgentRuntimeError::NotFound(agent_id.clone()));
    }
    let parents = snapshots
        .iter()
        .map(|snapshot| {
            (
                snapshot.identity.id.clone(),
                snapshot.identity.parent_id.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut close_order = snapshots
        .into_iter()
        .filter(|snapshot| {
            snapshot.identity.id == *agent_id
                || has_ancestor(&parents, &snapshot.identity.id, agent_id)
        })
        .collect::<Vec<_>>();
    close_order.sort_by(|left, right| {
        right
            .identity
            .depth
            .cmp(&left.identity.depth)
            .then_with(|| left.identity.id.cmp(&right.identity.id))
    });

    let mut target = None;
    for snapshot in close_order {
        let closed = close_actor(actors, &snapshot.identity.id).await?;
        if snapshot.identity.id == *agent_id {
            target = Some(closed);
        }
    }
    target.ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))
}

async fn root_agent_id_for(
    actors: &BTreeMap<AgentId, AgentActorHandle>,
    agent_id: &AgentId,
) -> AgentRuntimeResult<AgentId> {
    let snapshots = list_snapshots(actors).await?;
    let parents = snapshots
        .iter()
        .map(|snapshot| {
            (
                snapshot.identity.id.clone(),
                snapshot.identity.parent_id.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if !parents.contains_key(agent_id) {
        return Err(AgentRuntimeError::NotFound(agent_id.clone()));
    }
    let mut current = agent_id.clone();
    let mut remaining = parents.len();
    while let Some(parent) = parents.get(&current).cloned().flatten() {
        if remaining == 0 {
            return Err(AgentRuntimeError::Lifecycle(
                "agent parent graph contains a cycle".to_string(),
            ));
        }
        remaining -= 1;
        current = parent;
        if !parents.contains_key(&current) {
            return Err(AgentRuntimeError::Lifecycle(format!(
                "agent parent {} is missing while resolving root for {agent_id}",
                current.as_str()
            )));
        }
    }
    Ok(current)
}

fn has_ancestor(
    parents: &BTreeMap<AgentId, Option<AgentId>>,
    candidate: &AgentId,
    ancestor: &AgentId,
) -> bool {
    let mut current = parents.get(candidate).cloned().flatten();
    let mut remaining = parents.len();
    while let Some(parent) = current {
        if parent == *ancestor {
            return true;
        }
        if remaining == 0 {
            return false;
        }
        remaining -= 1;
        current = parents.get(&parent).cloned().flatten();
    }
    false
}

async fn close_actor(
    actors: &BTreeMap<AgentId, AgentActorHandle>,
    agent_id: &AgentId,
) -> AgentRuntimeResult<AgentSnapshot> {
    let actor = actors
        .get(agent_id)
        .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))?;
    let (reply, receiver) = oneshot::channel();
    actor.send(ActorCommand::Close { reply }).await?;
    receiver
        .await
        .map_err(|_| AgentRuntimeError::ChannelClosed)?
}

async fn route(
    actors: &BTreeMap<AgentId, AgentActorHandle>,
    agent_id: &AgentId,
    command: ActorCommand,
) {
    let Some(actor) = actors.get(agent_id) else {
        reject_missing(command, agent_id.clone());
        return;
    };
    if actor.send(command).await.is_err() {
        // Command reply is dropped and the public handle reports ChannelClosed.
    }
}

fn reject_missing(command: ActorCommand, agent_id: AgentId) {
    let error = AgentRuntimeError::NotFound(agent_id);
    match command {
        ActorCommand::Submit { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::SubmitCurrentSession { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::CancelTurn { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::SetActivity { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::Checkpoint { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::OpenSession { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::RecordSessionFacts { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::Snapshot { reply } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::WakeAccepted { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::AcceptWakeSignals { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::EnterWaitingAgents { reply } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::StartPendingInputs { reply } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::Close { reply } => {
            let _ = reply.send(Err(error));
        }
        ActorCommand::TurnFinished(_) | ActorCommand::Shutdown { .. } => {}
    }
}

async fn list_snapshots(
    actors: &BTreeMap<AgentId, AgentActorHandle>,
) -> AgentRuntimeResult<Vec<AgentSnapshot>> {
    let mut snapshots = Vec::with_capacity(actors.len());
    for actor in actors.values() {
        let (reply, receiver) = oneshot::channel();
        actor.send(ActorCommand::Snapshot { reply }).await?;
        snapshots.push(
            receiver
                .await
                .map_err(|_| AgentRuntimeError::ChannelClosed)??,
        );
    }
    snapshots.sort_by(|left, right| left.identity.id.cmp(&right.identity.id));
    Ok(snapshots)
}
