use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc, oneshot};

use super::agent_loop::{AgentLoopCommand, AgentLoopHandle, spawn_agent_loop};
use super::directory::AgentDirectoryHandle;
use super::host::{AgentCommitObserver, AgentLifecycleAdapter, ThreadRepository};
use super::runtime::{AgentRuntimeOptions, RestoredInputPolicy};
use super::state::{AgentRuntimeError, unix_timestamp};
use super::{
    AgentCommittedEvent, AgentRegistration, AgentRuntimeEvent, AgentRuntimeEventKind,
    AgentRuntimeHandle, AgentRuntimeHost, AgentRuntimeResult, AgentSnapshot, AgentSpawnRequest,
    AgentSpawnResult, AgentState, DurableCommitFacts, DurableMailboxEnvelope, RestoredAgentRuntime,
    SpawnLifecycleRequest, SpawnRollbackPhase, SpawnRollbackReason, ThreadCommit, ThreadId, TurnId,
};
use crate::ThreadEventBus;

mod spawn;
use spawn::{register_agent, spawn_child_agent};

pub(crate) type AgentRegistry = Arc<RwLock<BTreeMap<ThreadId, AgentLoopHandle>>>;

pub(crate) enum CoordinatorCommand {
    Register {
        registration: AgentRegistration,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    /// 惰性驻留：从 repository 恢复的单个 Thread 注册 actor（幂等）。
    RestoreAgent {
        agent: Box<RestoredAgentRuntime>,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    /// LRU 淘汰：只允许空闲（无活动 Turn、无 pending input）的驻留 actor。
    EvictAgent {
        agent_id: ThreadId,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Spawn {
        request: AgentSpawnRequest,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSpawnResult>>,
    },
    Close {
        agent_id: ThreadId,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    /// 完成关闭事务并释放整棵 Thread actor 树及事件投影。
    Retire {
        agent_id: ThreadId,
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
            CoordinatorCommand::RestoreAgent { agent, reply } => {
                let result = restore_agent(&host, &runtime, &actors, *agent, options).await;
                let _ = reply.send(result);
            }
            CoordinatorCommand::EvictAgent { agent_id, reply } => {
                let result = evict_agent(&actors, &runtime, &agent_id).await;
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
            CoordinatorCommand::Retire { agent_id, reply } => {
                let result = retire_agent_tree(&actors, &runtime, &agent_id).await;
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
    agent_id: &ThreadId,
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
    agent_id: &ThreadId,
) -> AgentRuntimeResult<AgentSnapshot> {
    let close_order = agent_tree_snapshots(actors, agent_id).await?;
    close_snapshots(actors, agent_id, &close_order).await
}

async fn retire_agent_tree(
    actors: &AgentRegistry,
    runtime: &AgentRuntimeHandle,
    agent_id: &ThreadId,
) -> AgentRuntimeResult<AgentSnapshot> {
    let close_order = agent_tree_snapshots(actors, agent_id).await?;
    let target = close_snapshots(actors, agent_id, &close_order).await?;
    for snapshot in close_order {
        evict_agent(actors, runtime, &snapshot.identity.id).await?;
    }
    Ok(target)
}

async fn agent_tree_snapshots(
    actors: &AgentRegistry,
    agent_id: &ThreadId,
) -> AgentRuntimeResult<Vec<AgentSnapshot>> {
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
    Ok(close_order)
}

async fn close_snapshots(
    actors: &AgentRegistry,
    agent_id: &ThreadId,
    close_order: &[AgentSnapshot],
) -> AgentRuntimeResult<AgentSnapshot> {
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
    parents: &BTreeMap<ThreadId, Option<ThreadId>>,
    candidate: &ThreadId,
    ancestor: &ThreadId,
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
    agent_id: &ThreadId,
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

/// 把 repository 恢复出的单个 Thread 注册为驻留 actor（幂等）。
///
/// 与启动恢复共用 [`recover_interrupted_turns`]：遗留 active Turn 在这里收束。
/// 已驻留时直接返回当前 snapshot，不重复注册。
async fn restore_agent<H>(
    host: &H,
    runtime: &AgentRuntimeHandle,
    actors: &AgentRegistry,
    agent: RestoredAgentRuntime,
    options: AgentRuntimeOptions,
) -> AgentRuntimeResult<AgentSnapshot>
where
    H: AgentRuntimeHost,
{
    let id = agent.state.snapshot.identity.id.clone();
    if let Ok(existing) = snapshot_for(actors, &id).await {
        return Ok(existing);
    }
    let thread_events = runtime.thread_events.clone();
    if let Some(restored) = agent.thread_snapshot.as_ref() {
        thread_events
            .replace_snapshot(restored.snapshot.clone())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
    }
    let recovered = super::runtime::recover_interrupted_turns(host, &thread_events, vec![agent])
        .await?
        .pop()
        .expect("recover preserves the restored agent count");
    let snapshot = recovered.state.snapshot.clone();
    runtime.directory.store_snapshot(snapshot.clone());
    let actor = spawn_agent_loop(
        host.clone(),
        recovered.state,
        runtime.clone(),
        options.cancel_grace,
        // 惰性驻留不自动执行模型：pending input 等待显式 submit/命令放行。
        false,
        options.command_capacity,
    );
    actors.write().await.insert(id, actor);
    Ok(snapshot)
}

/// 淘汰一个空闲驻留 actor：通知 loop 退出并从 registry/directory 移除。
async fn evict_agent(
    actors: &AgentRegistry,
    runtime: &AgentRuntimeHandle,
    agent_id: &ThreadId,
) -> AgentRuntimeResult<()> {
    let snapshot = snapshot_for(actors, agent_id).await?;
    if matches!(
        snapshot.state,
        AgentState::Queued(_)
            | AgentState::Running(_)
            | AgentState::WaitingTool(_)
            | AgentState::WaitingInteraction(_)
            | AgentState::Cancelling(_)
    ) || snapshot.pending_inputs > 0
    {
        return Err(AgentRuntimeError::InvalidInput(format!(
            "agent {agent_id} is busy and cannot be evicted"
        )));
    }
    let actor = actors
        .read()
        .await
        .get(agent_id)
        .cloned()
        .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))?;
    let (reply, receiver) = oneshot::channel();
    actor
        .send(AgentLoopCommand::Shutdown { reply })
        .await
        .map_err(|_| AgentRuntimeError::ChannelClosed)?;
    receiver
        .await
        .map_err(|_| AgentRuntimeError::ChannelClosed)??;
    actors.write().await.remove(agent_id);
    runtime.directory.remove(agent_id);
    runtime
        .thread_events
        .remove_thread(agent_id.as_str())
        .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
    Ok(())
}
