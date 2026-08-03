use std::collections::BTreeMap;

use tokio::sync::{mpsc, oneshot};

use super::agent_loop::{AgentLoopCommand, AgentLoopHandle, spawn_agent_loop};
use super::directory::AgentDirectoryHandle;
use super::host::{AgentCommitObserver, AgentLifecycleAdapter, AgentStateRepository};
use super::runtime::{AgentRuntimeOptions, RestoredInputPolicy};
use super::state::{AgentRuntimeError, unix_timestamp};
use super::{
    AgentCommit, AgentCommitOutcome, AgentCommittedEvent, AgentCurrentSessionSubmitRequest,
    AgentId, AgentProgressCheckpoint, AgentProgressStage, AgentRegistration, AgentRuntimeEvent,
    AgentRuntimeEventKind, AgentRuntimeHandle, AgentRuntimeHost, AgentRuntimeResult,
    AgentSessionDigest, AgentSnapshot, AgentSpawnRequest, AgentSpawnResult, AgentSubmitRequest,
    PendingAgentInput, RestoredAgentRuntime, SpawnLifecycleRequest, TurnId,
};
use crate::SessionEventHub;

mod spawn;
use spawn::{register_agent, spawn_child_agent};

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
    RecordSessionFacts {
        agent_id: AgentId,
        session_id: super::SessionId,
        facts: Vec<crate::SessionEventFact>,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    ReportProgress {
        agent_id: AgentId,
        stage: AgentProgressStage,
        summary: String,
        next_step: String,
        reply: oneshot::Sender<AgentRuntimeResult<AgentProgressCheckpoint>>,
    },
    ReadSession {
        agent_id: AgentId,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSessionDigest>>,
    },
    Close {
        agent_id: AgentId,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    Snapshot {
        agent_id: AgentId,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    List {
        reply: oneshot::Sender<AgentRuntimeResult<Vec<AgentSnapshot>>>,
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
    let directory =
        AgentDirectoryHandle::new(restored.iter().map(|agent| agent.state.snapshot.clone()));
    let handle = AgentRuntimeHandle::new(sender, session_events.handle(), directory);
    let mut actors = BTreeMap::new();
    for restored_agent in restored {
        let id = restored_agent.state.snapshot.identity.id.clone();
        actors.insert(
            id,
            spawn_agent_loop(
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
    Ok(handle)
}

async fn run_coordinator<H>(
    host: H,
    runtime: AgentRuntimeHandle,
    mut actors: BTreeMap<AgentId, AgentLoopHandle>,
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
                route(
                    &actors,
                    &agent_id,
                    AgentLoopCommand::Submit { request, reply },
                )
                .await;
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
                    AgentLoopCommand::SubmitCurrentSession {
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
                    AgentLoopCommand::CancelTurn { turn_id, reply },
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
                    AgentLoopCommand::SetActivity {
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
                    AgentLoopCommand::Checkpoint { checkpoint, reply },
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
                    AgentLoopCommand::RecordSessionFacts {
                        session_id,
                        facts,
                        reply,
                    },
                )
                .await;
            }
            CoordinatorCommand::ReportProgress {
                agent_id,
                stage,
                summary,
                next_step,
                reply,
            } => {
                route(
                    &actors,
                    &agent_id,
                    AgentLoopCommand::ReportProgress {
                        stage,
                        summary,
                        next_step,
                        reply,
                    },
                )
                .await;
            }
            CoordinatorCommand::ReadSession { agent_id, reply } => {
                route(&actors, &agent_id, AgentLoopCommand::ReadSession { reply }).await;
            }
            CoordinatorCommand::Close { agent_id, reply } => {
                let result = close_agent_tree(&actors, &agent_id).await;
                let _ = reply.send(result);
            }
            CoordinatorCommand::Snapshot { agent_id, reply } => {
                route(&actors, &agent_id, AgentLoopCommand::Snapshot { reply }).await;
            }
            CoordinatorCommand::List { reply } => {
                let _ = reply.send(list_snapshots(&actors).await);
            }
            CoordinatorCommand::StartRestoredInputs { reply } => {
                let _ = reply.send(start_pending_inputs(&actors).await);
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
        let _ = actor.send(AgentLoopCommand::Shutdown { reply }).await;
    }
}

async fn shutdown_agents(actors: &BTreeMap<AgentId, AgentLoopHandle>) -> AgentRuntimeResult<()> {
    let mut first_error = None;
    for actor in actors.values() {
        let (reply, receiver) = oneshot::channel();
        let result = match actor.send(AgentLoopCommand::Shutdown { reply }).await {
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
    actors: &BTreeMap<AgentId, AgentLoopHandle>,
) -> AgentRuntimeResult<()> {
    for actor in actors.values() {
        let (reply, receiver) = oneshot::channel();
        actor
            .send(AgentLoopCommand::StartPendingInputs { reply })
            .await?;
        receiver
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)??;
    }
    Ok(())
}

async fn snapshot_for(
    actors: &BTreeMap<AgentId, AgentLoopHandle>,
    agent_id: &AgentId,
) -> AgentRuntimeResult<AgentSnapshot> {
    let actor = actors
        .get(agent_id)
        .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))?;
    let (reply, receiver) = oneshot::channel();
    actor.send(AgentLoopCommand::Snapshot { reply }).await?;
    receiver
        .await
        .map_err(|_| AgentRuntimeError::ChannelClosed)?
}

async fn close_agent_tree(
    actors: &BTreeMap<AgentId, AgentLoopHandle>,
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
    actors: &BTreeMap<AgentId, AgentLoopHandle>,
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
    actors: &BTreeMap<AgentId, AgentLoopHandle>,
    agent_id: &AgentId,
) -> AgentRuntimeResult<AgentSnapshot> {
    let actor = actors
        .get(agent_id)
        .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))?;
    let (reply, receiver) = oneshot::channel();
    actor.send(AgentLoopCommand::Close { reply }).await?;
    receiver
        .await
        .map_err(|_| AgentRuntimeError::ChannelClosed)?
}

async fn route(
    actors: &BTreeMap<AgentId, AgentLoopHandle>,
    agent_id: &AgentId,
    command: AgentLoopCommand,
) {
    let Some(actor) = actors.get(agent_id) else {
        reject_missing(command, agent_id.clone());
        return;
    };
    if actor.send(command).await.is_err() {
        // Command reply is dropped and the public handle reports ChannelClosed.
    }
}

fn reject_missing(command: AgentLoopCommand, agent_id: AgentId) {
    let error = AgentRuntimeError::NotFound(agent_id);
    match command {
        AgentLoopCommand::Submit { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::SubmitCurrentSession { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::CancelTurn { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::SetActivity { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::Checkpoint { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::RecordSessionFacts { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::ReportProgress { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::ReadSession { reply } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::Snapshot { reply } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::StartPendingInputs { reply } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::Close { reply } => {
            let _ = reply.send(Err(error));
        }
        AgentLoopCommand::TurnFinished(_) | AgentLoopCommand::Shutdown { .. } => {}
    }
}

async fn list_snapshots(
    actors: &BTreeMap<AgentId, AgentLoopHandle>,
) -> AgentRuntimeResult<Vec<AgentSnapshot>> {
    let mut snapshots = Vec::with_capacity(actors.len());
    for actor in actors.values() {
        let (reply, receiver) = oneshot::channel();
        actor.send(AgentLoopCommand::Snapshot { reply }).await?;
        snapshots.push(
            receiver
                .await
                .map_err(|_| AgentRuntimeError::ChannelClosed)??,
        );
    }
    snapshots.sort_by(|left, right| left.identity.id.cmp(&right.identity.id));
    Ok(snapshots)
}
