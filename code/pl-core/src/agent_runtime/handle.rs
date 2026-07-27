use std::fmt;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};

use pl_protocol::{SessionSubscriptionRequest, SessionViewSnapshot};

use super::coordinator::CoordinatorCommand;
use super::event_hub::AgentEventHubHandle;
use super::{
    AgentActivityState, AgentCurrentSessionSubmitRequest, AgentId, AgentParentSubscription,
    AgentRegistration, AgentRuntimeResult, AgentSessionState, AgentSnapshot, AgentSpawnRequest,
    AgentSpawnResult, AgentSubmitRequest, AgentTurnCheckpoint, AgentUpdateKind, AgentWaitResult,
    SessionId, TurnId,
};
use crate::agent_runtime::state::AgentRuntimeError;
use crate::{SessionEventHubHandle, SessionEventSubscription};

/// 不包含 host 泛型的 cloneable runtime 命令句柄。
///
/// 产品 facade 与协作工具只能通过该句柄访问 agent 状态机，不能直接持有 actor state。
#[derive(Clone)]
pub struct AgentRuntimeHandle {
    pub(crate) sender: mpsc::Sender<CoordinatorCommand>,
    pub(crate) session_events: SessionEventHubHandle,
    pub(crate) agent_events: AgentEventHubHandle,
}

impl AgentRuntimeHandle {
    pub(crate) fn new(
        sender: mpsc::Sender<CoordinatorCommand>,
        session_events: SessionEventHubHandle,
        agent_events: AgentEventHubHandle,
    ) -> Self {
        Self {
            sender,
            session_events,
            agent_events,
        }
    }

    /// 注册已经准备好外部资源的 root 或恢复 agent。
    pub async fn register(
        &self,
        registration: AgentRegistration,
    ) -> AgentRuntimeResult<AgentSnapshot> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::Register {
            registration,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    /// 向 agent 提交输入并返回预分配的 turn id。
    pub async fn submit(
        &self,
        agent_id: AgentId,
        request: AgentSubmitRequest,
    ) -> AgentRuntimeResult<TurnId> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::Submit {
            agent_id,
            request,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    /// 由目标 actor 原子解析 owner-bound current session 后提交输入。
    pub async fn submit_current_session(
        &self,
        agent_id: AgentId,
        request: AgentCurrentSessionSubmitRequest,
    ) -> AgentRuntimeResult<TurnId> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::SubmitCurrentSession {
            agent_id,
            request,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    /// 通过 host lifecycle saga 创建 child agent。
    pub async fn spawn(&self, request: AgentSpawnRequest) -> AgentRuntimeResult<AgentSpawnResult> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::Spawn { request, reply })
            .await?;
        receive(receiver).await?
    }

    /// 取消与 id 精确匹配的活动 turn。
    pub async fn cancel_turn(&self, agent_id: AgentId, turn_id: TurnId) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::CancelTurn {
            agent_id,
            turn_id,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    pub(crate) async fn set_activity(
        &self,
        agent_id: AgentId,
        turn_id: TurnId,
        activity: AgentActivityState,
    ) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::SetActivity {
            agent_id,
            turn_id,
            activity,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    pub(crate) async fn checkpoint_turn(
        &self,
        agent_id: AgentId,
        checkpoint: AgentTurnCheckpoint,
    ) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::Checkpoint {
            agent_id,
            checkpoint,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    /// 把新的空或已导入 session 纳入 agent canonical state；重复调用保持幂等。
    pub async fn open_session(
        &self,
        agent_id: AgentId,
        session: AgentSessionState,
    ) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::OpenSession {
            agent_id,
            session,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    /// 将产品关联出的公共事实交给目标 agent 串行持久化和投影。
    ///
    /// 调用端不能自行分配 sequence 或广播；事实 source 必须是目标 session owner。
    /// 产品触发的 interaction/plan 等事实未提供 source 时由 actor 补为 owner。
    pub async fn record_session_facts(
        &self,
        agent_id: AgentId,
        session_id: SessionId,
        facts: Vec<crate::SessionEventFact>,
    ) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::RecordSessionFacts {
            agent_id,
            session_id,
            facts,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    /// 关闭 agent 及其产品资源。
    pub async fn close(&self, agent_id: AgentId) -> AgentRuntimeResult<AgentSnapshot> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::Close { agent_id, reply })
            .await?;
        receive(receiver).await?
    }

    /// 读取 agent latest snapshot。
    pub async fn snapshot(&self, agent_id: AgentId) -> AgentRuntimeResult<AgentSnapshot> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::Snapshot { agent_id, reply })
            .await?;
        receive(receiver).await?
    }

    /// 列出 runtime 内全部 agent snapshot。
    pub async fn list(&self) -> AgentRuntimeResult<Vec<AgentSnapshot>> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::List { reply }).await?;
        receive(receiver).await?
    }

    /// 订阅 direct children；首帧为 canonical snapshots。
    pub fn subscribe_children(&self, parent_agent_id: &AgentId) -> AgentParentSubscription {
        self.agent_events.subscribe_parent(parent_agent_id)
    }

    pub(crate) async fn enter_waiting_agents(&self, agent_id: AgentId) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::EnterWaitingAgents { agent_id, reply })
            .await?;
        receive(receiver).await?
    }

    pub(crate) async fn wake_accepted(
        &self,
        agent_id: AgentId,
        wake_id: Option<super::AgentWakeId>,
        signal_ids: Vec<String>,
    ) -> AgentRuntimeResult<bool> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::WakeAccepted {
            agent_id,
            wake_id,
            signal_ids,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    /// 等待 agent 进入 Idle 且队列为空；使用提交后事件订阅，不占用 actor waiter。
    pub async fn wait_until_idle(&self, agent_id: AgentId) -> AgentRuntimeResult<AgentWaitResult> {
        let mut receiver = self.agent_events.subscribe_snapshots();
        loop {
            let snapshot = self.agent_events.snapshot(&agent_id)?;
            if snapshot.lifecycle != super::AgentLifecycleState::Active
                || (snapshot.activity == AgentActivityState::Idle && snapshot.pending_inputs == 0)
            {
                return Ok(AgentWaitResult {
                    last_turn: snapshot.last_turn.clone(),
                    snapshot,
                });
            }
            match receiver.recv().await {
                Ok(snapshot) if snapshot.identity.id == agent_id => {}
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(AgentRuntimeError::ChannelClosed);
                }
            }
        }
    }

    /// 有界等待 agent 进入 Idle 且队列为空。
    pub async fn wait_timeout(
        &self,
        agent_id: AgentId,
        timeout: Duration,
    ) -> AgentRuntimeResult<AgentWaitResult> {
        tokio::time::timeout(timeout, self.wait_until_idle(agent_id))
            .await
            .map_err(|_| AgentRuntimeError::TimedOut)?
    }

    /// 产品 durable 事实提交后发布 managed child 阶段更新。
    pub fn publish_product_phase(
        &self,
        parent_agent_id: AgentId,
        agent_id: AgentId,
        signal_id: String,
        phase: String,
        summary: Option<String>,
    ) -> AgentRuntimeResult<()> {
        self.agent_events.publish_product_phase(
            parent_agent_id,
            agent_id,
            signal_id,
            phase,
            summary,
        )
    }

    pub(crate) fn publish_progress(
        &self,
        agent_id: &AgentId,
        kind: AgentUpdateKind,
        summary: Option<String>,
        signal_id: String,
    ) -> AgentRuntimeResult<()> {
        self.agent_events
            .publish_progress(agent_id, kind, summary, signal_id)
    }

    /// 订阅指定 session；首帧为 snapshot/replay，随后进入独立实时 channel。
    pub fn subscribe_session(
        &self,
        request: SessionSubscriptionRequest,
    ) -> AgentRuntimeResult<SessionEventSubscription> {
        self.session_events
            .subscribe(request)
            .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))
    }

    /// 读取包含当前 transient overlay 的 authoritative session projection。
    pub fn session_snapshot(
        &self,
        session_id: &SessionId,
    ) -> AgentRuntimeResult<SessionViewSnapshot> {
        self.session_events
            .snapshot(session_id.as_str())
            .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))
    }

    /// host 完成外部资源恢复后，一次性放行启动时暂停的 durable FIFO。
    pub async fn start_restored_inputs(&self) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::StartRestoredInputs { reply })
            .await?;
        receive(receiver).await?
    }

    /// 停止 coordinator 和全部 actor。
    pub async fn shutdown(&self) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::Shutdown { reply }).await?;
        receive(receiver).await?
    }

    async fn send(&self, command: CoordinatorCommand) -> AgentRuntimeResult<()> {
        self.sender
            .send(command)
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)
    }
}

impl fmt::Debug for AgentRuntimeHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRuntimeHandle")
            .finish_non_exhaustive()
    }
}

async fn receive<T>(receiver: oneshot::Receiver<T>) -> AgentRuntimeResult<T> {
    receiver.await.map_err(|_| AgentRuntimeError::ChannelClosed)
}
