use std::collections::BTreeMap;
use std::fmt;

use tokio::sync::{mpsc, oneshot};

use pl_protocol::{SessionSubscriptionRequest, SessionViewSnapshot};

use super::coordinator::CoordinatorCommand;
use super::directory::{AgentDirectoryHandle, AgentDirectorySnapshot, AgentDirectorySubscription};
use super::{
    AgentActivityState, AgentCurrentSessionSubmitRequest, AgentDirectoryWaitReason,
    AgentDirectoryWaitResult, AgentId, AgentLifecycleState, AgentProgressCheckpoint,
    AgentProgressStage, AgentRegistration, AgentRuntimeResult, AgentSessionDigest, AgentSnapshot,
    AgentSpawnRequest, AgentSpawnResult, AgentSubmitRequest, AgentTurnCheckpoint, AgentWaitResult,
    SessionId, TurnId,
};
use crate::agent_runtime::state::AgentRuntimeError;
use crate::{SessionEventHubHandle, SessionEventSubscription};

/// 不包含 host 泛型的 cloneable runtime 命令句柄。
///
/// 产品 facade 与协作工具只能通过该句柄访问 agent 状态机，不能直接持有 loop state。
#[derive(Clone)]
pub struct AgentRuntimeHandle {
    pub(crate) sender: mpsc::Sender<CoordinatorCommand>,
    pub(crate) session_events: SessionEventHubHandle,
    pub(crate) directory: AgentDirectoryHandle,
}

impl AgentRuntimeHandle {
    pub(crate) fn new(
        sender: mpsc::Sender<CoordinatorCommand>,
        session_events: SessionEventHubHandle,
        directory: AgentDirectoryHandle,
    ) -> Self {
        Self {
            sender,
            session_events,
            directory,
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

    /// 向 agent 提交显式输入；活动 turn 收到 steer，空闲 agent 启动下一 turn。
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

    /// 由目标 loop 原子解析唯一 canonical session 后提交输入。
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

    /// 中断与 id 精确匹配的活动 turn。
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

    /// 将产品关联出的公共事实交给目标 agent 串行持久化和投影。
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

    /// 更新调用 agent 的显式进度 checkpoint。
    pub async fn report_progress(
        &self,
        agent_id: AgentId,
        stage: AgentProgressStage,
        summary: String,
        next_step: String,
    ) -> AgentRuntimeResult<AgentProgressCheckpoint> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::ReportProgress {
            agent_id,
            stage,
            summary,
            next_step,
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    /// 读取目标 agent 唯一 canonical session 的有界、过滤摘要。
    pub async fn read_agent_session(
        &self,
        agent_id: AgentId,
    ) -> AgentRuntimeResult<AgentSessionDigest> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::ReadSession { agent_id, reply })
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

    /// 读取 Agent Directory 的 canonical snapshot。
    pub fn directory_snapshot(&self) -> AgentDirectorySnapshot {
        self.directory.directory_snapshot()
    }

    /// 订阅 Agent Directory 的单一 revision watch。
    pub fn subscribe_directory(&self) -> AgentDirectorySubscription {
        self.directory.subscribe()
    }

    /// 等待任一目标出现新 progress、interaction 或 terminal 事实。
    pub async fn wait_agents(
        &self,
        targets: Vec<AgentId>,
    ) -> AgentRuntimeResult<AgentDirectoryWaitResult> {
        if targets.is_empty() {
            return Err(AgentRuntimeError::InvalidInput(
                "wait_agents requires at least one target".to_string(),
            ));
        }
        let mut subscription = self.directory.subscribe();
        let baseline = target_snapshots(&self.directory.directory_snapshot(), &targets)?;
        if let Some(result) = current_wait_result(baseline.values()) {
            return Ok(result);
        }

        loop {
            subscription.changed().await?;
            let current = target_snapshots(&self.directory.directory_snapshot(), &targets)?;
            if let Some(result) = changed_wait_result(&baseline, &current) {
                return Ok(result);
            }
        }
    }

    /// 等待 agent 进入 Idle 且队列为空；只由 directory watch 驱动。
    pub async fn wait_until_idle(&self, agent_id: AgentId) -> AgentRuntimeResult<AgentWaitResult> {
        let mut subscription = self.directory.subscribe();
        loop {
            let snapshot = self.directory.snapshot(&agent_id)?;
            if snapshot.lifecycle != AgentLifecycleState::Active
                || (snapshot.activity == AgentActivityState::Idle && snapshot.pending_inputs == 0)
            {
                return Ok(AgentWaitResult {
                    last_turn: snapshot.last_turn.clone(),
                    snapshot,
                });
            }
            subscription.changed().await?;
        }
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

    /// 停止 coordinator 和全部 agent loop。
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

fn target_snapshots(
    directory: &AgentDirectorySnapshot,
    targets: &[AgentId],
) -> AgentRuntimeResult<BTreeMap<AgentId, AgentSnapshot>> {
    let snapshots = directory
        .agents
        .iter()
        .map(|snapshot| (snapshot.identity.id.clone(), snapshot.clone()))
        .collect::<BTreeMap<_, _>>();
    targets
        .iter()
        .map(|target| {
            snapshots
                .get(target)
                .cloned()
                .map(|snapshot| (target.clone(), snapshot))
                .ok_or_else(|| AgentRuntimeError::NotFound(target.clone()))
        })
        .collect()
}

fn current_wait_result<'a>(
    snapshots: impl Iterator<Item = &'a AgentSnapshot>,
) -> Option<AgentDirectoryWaitResult> {
    let snapshots = snapshots.cloned().collect::<Vec<_>>();
    let terminal = snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.lifecycle != AgentLifecycleState::Active
                || (snapshot.activity == AgentActivityState::Idle
                    && snapshot.pending_inputs == 0
                    && snapshot.last_turn.is_some())
        })
        .cloned()
        .collect::<Vec<_>>();
    if !terminal.is_empty() {
        return Some(AgentDirectoryWaitResult {
            reason: AgentDirectoryWaitReason::Terminal,
            agents: terminal,
        });
    }
    let interactions = snapshots
        .into_iter()
        .filter(|snapshot| snapshot.activity == AgentActivityState::WaitingInteraction)
        .collect::<Vec<_>>();
    (!interactions.is_empty()).then_some(AgentDirectoryWaitResult {
        reason: AgentDirectoryWaitReason::Interaction,
        agents: interactions,
    })
}

fn changed_wait_result(
    baseline: &BTreeMap<AgentId, AgentSnapshot>,
    current: &BTreeMap<AgentId, AgentSnapshot>,
) -> Option<AgentDirectoryWaitResult> {
    let changed = current
        .iter()
        .filter_map(|(id, snapshot)| {
            let previous = baseline.get(id)?;
            let reason = if snapshot.lifecycle != previous.lifecycle
                || snapshot.last_turn != previous.last_turn
            {
                AgentDirectoryWaitReason::Terminal
            } else if snapshot.activity == AgentActivityState::WaitingInteraction
                && previous.activity != AgentActivityState::WaitingInteraction
            {
                AgentDirectoryWaitReason::Interaction
            } else if snapshot.progress != previous.progress {
                AgentDirectoryWaitReason::Progress
            } else {
                return None;
            };
            Some((reason, snapshot.clone()))
        })
        .collect::<Vec<_>>();
    let reason = changed
        .iter()
        .map(|(reason, _)| *reason)
        .min_by_key(|reason| match reason {
            AgentDirectoryWaitReason::Terminal => 0,
            AgentDirectoryWaitReason::Interaction => 1,
            AgentDirectoryWaitReason::Progress => 2,
        })?;
    Some(AgentDirectoryWaitResult {
        reason,
        agents: changed
            .into_iter()
            .filter_map(|(candidate, snapshot)| (candidate == reason).then_some(snapshot))
            .collect(),
    })
}

async fn receive<T>(receiver: oneshot::Receiver<T>) -> AgentRuntimeResult<T> {
    receiver.await.map_err(|_| AgentRuntimeError::ChannelClosed)
}
