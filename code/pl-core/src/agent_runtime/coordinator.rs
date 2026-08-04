use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc, oneshot};

use super::agent_loop::{AgentLoopCommand, AgentLoopHandle, spawn_agent_loop};
use super::directory::AgentDirectoryHandle;
use super::host::{AgentCommitObserver, AgentLifecycleAdapter, ThreadRepository};
use super::runtime::{AgentRuntimeOptions, RestoredInputPolicy};
use super::state::{AgentRuntimeError, unix_timestamp};
use super::{
    AgentCommittedEvent, AgentId, AgentRegistration, AgentRuntimeEvent, AgentRuntimeEventKind,
    AgentRuntimeHandle, AgentRuntimeHost, AgentRuntimeResult, AgentSnapshot, AgentSpawnRequest,
    AgentSpawnResult, DurableCommitFacts, DurableMailboxEnvelope, RestoredAgentRuntime,
    SpawnLifecycleRequest, ThreadCommit, ThreadCommitOutcome, TurnId,
};
use crate::ThreadEventBus;

mod spawn;
use spawn::{register_agent, spawn_child_agent};

pub(crate) type AgentRegistry = Arc<RwLock<BTreeMap<AgentId, AgentLoopHandle>>>;

pub(crate) enum CoordinatorCommand {
    Register {
        registration: AgentRegistration,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    Spawn {
        request: AgentSpawnRequest,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSpawnResult>>,
    },
    Close {
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
    thread_events: ThreadEventBus,
) -> AgentRuntimeResult<AgentRuntimeHandle>
where
    H: AgentRuntimeHost,
{
    let (sender, receiver) = mpsc::channel(options.command_capacity.max(1));
    let directory =
        AgentDirectoryHandle::new(restored.iter().map(|agent| agent.state.snapshot.clone()));
    let actors = Arc::new(RwLock::new(BTreeMap::new()));
    let handle = AgentRuntimeHandle::new(sender, actors.clone(), thread_events.handle(), directory);
    for restored_agent in restored {
        let id = restored_agent.state.snapshot.identity.id.clone();
        let actor = spawn_agent_loop(
            host.clone(),
            restored_agent.state,
            handle.clone(),
            options.cancel_grace,
            options.restored_inputs == RestoredInputPolicy::Start,
            options.command_capacity,
        );
        actors
            .try_write()
            .expect("restored actor registry is not shared before coordinator spawn")
            .insert(id, actor);
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
    actors: AgentRegistry,
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
                let result = register_agent(&host, &runtime, &actors, registration, options).await;
                let _ = reply.send(result);
            }
            CoordinatorCommand::Spawn { request, reply } => {
                let result = spawn_child_agent(&host, &runtime, &actors, request, options).await;
                let _ = reply.send(result);
            }
            CoordinatorCommand::Close { agent_id, reply } => {
                let result = close_agent_tree(&actors, &agent_id).await;
                let _ = reply.send(result);
            }
            CoordinatorCommand::List { reply } => {
                let _ = reply.send(list_snapshots(&actors).await);
            }
            CoordinatorCommand::StartRestoredInputs { reply } => {
                let _ = reply.send(start_pending_inputs(&actors).await);
            }
            CoordinatorCommand::Shutdown { reply } => {
                let result = shutdown_agents(&actors).await;
                actors.write().await.clear();
                let _ = reply.send(result);
                break;
            }
        }
    }
    for actor in actor_handles(&actors).await {
        let (reply, _receiver) = oneshot::channel();
        let _ = actor.send(AgentLoopCommand::Shutdown { reply }).await;
    }
}

async fn shutdown_agents(actors: &AgentRegistry) -> AgentRuntimeResult<()> {
    let mut first_error = None;
    for actor in actor_handles(actors).await {
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

async fn start_pending_inputs(actors: &AgentRegistry) -> AgentRuntimeResult<()> {
    for actor in actor_handles(actors).await {
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
    actors: &AgentRegistry,
    agent_id: &AgentId,
) -> AgentRuntimeResult<AgentSnapshot> {
    let actor = actors
        .read()
        .await
        .get(agent_id)
        .cloned()
        .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))?;
    let (reply, receiver) = oneshot::channel();
    actor.send(AgentLoopCommand::Snapshot { reply }).await?;
    receiver
        .await
        .map_err(|_| AgentRuntimeError::ChannelClosed)?
}

async fn close_agent_tree(
    actors: &AgentRegistry,
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
    actors: &AgentRegistry,
    agent_id: &AgentId,
) -> AgentRuntimeResult<AgentSnapshot> {
    let actor = actors
        .read()
        .await
        .get(agent_id)
        .cloned()
        .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))?;
    let (reply, receiver) = oneshot::channel();
    actor.send(AgentLoopCommand::Close { reply }).await?;
    receiver
        .await
        .map_err(|_| AgentRuntimeError::ChannelClosed)?
}

async fn list_snapshots(actors: &AgentRegistry) -> AgentRuntimeResult<Vec<AgentSnapshot>> {
    let actor_handles = actor_handles(actors).await;
    let mut snapshots = Vec::with_capacity(actor_handles.len());
    for actor in actor_handles {
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

async fn actor_handles(actors: &AgentRegistry) -> Vec<AgentLoopHandle> {
    actors.read().await.values().cloned().collect()
}
