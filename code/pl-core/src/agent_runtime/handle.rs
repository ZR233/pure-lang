use std::fmt;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use super::coordinator::CoordinatorCommand;
use super::{
    AgentActivityState, AgentId, AgentRegistration, AgentRuntimeResult, AgentSessionState,
    AgentSnapshot, AgentSpawnRequest, AgentSpawnResult, AgentSubmitRequest, AgentTurnCheckpoint,
    AgentWaitResult, TurnId,
};
use crate::agent_runtime::state::AgentRuntimeError;

/// 不包含 host 泛型的 cloneable runtime 命令句柄。
///
/// 产品 facade 与协作工具只能通过该句柄访问 agent 状态机，不能直接持有 actor state。
#[derive(Clone)]
pub struct AgentRuntimeHandle {
    pub(crate) sender: mpsc::Sender<CoordinatorCommand>,
}

impl AgentRuntimeHandle {
    pub(crate) fn new(sender: mpsc::Sender<CoordinatorCommand>) -> Self {
        Self { sender }
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

    /// 等待 agent 进入 Idle 且队列为空。
    pub async fn wait(&self, agent_id: AgentId) -> AgentRuntimeResult<AgentWaitResult> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::Wait { agent_id, reply })
            .await?;
        receive(receiver).await?
    }

    /// 有界等待 agent 进入 Idle 且队列为空。
    pub async fn wait_timeout(
        &self,
        agent_id: AgentId,
        timeout: Duration,
    ) -> AgentRuntimeResult<AgentWaitResult> {
        tokio::time::timeout(timeout, self.wait(agent_id))
            .await
            .map_err(|_| AgentRuntimeError::TimedOut)?
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
