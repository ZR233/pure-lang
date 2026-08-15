use std::collections::BTreeMap;
use std::fmt;

use tokio::sync::{mpsc, oneshot};

use pl_protocol::{ThreadSnapshot, ThreadSubscriptionRequest};

use super::agent_loop::{AgentLoopCommand, AgentLoopHandle};
use super::coordinator::{AgentRegistry, CoordinatorCommand};
use super::directory::{AgentDirectoryHandle, AgentDirectorySnapshot, AgentDirectorySubscription};
use super::{
    ActiveKind, AgentActivityState, AgentCurrentSessionSubmitRequest, AgentDirectoryWaitMessage,
    AgentDirectoryWaitReason, AgentDirectoryWaitResult, AgentId,
    AgentInteractionContinuationRequest, AgentLifecycleState, AgentProgressCheckpoint,
    AgentProgressStage, AgentRegistration, AgentRuntimeResult, AgentSessionDigest, AgentSnapshot,
    AgentSpawnRequest, AgentSpawnResult, AgentSubmissionPage, AgentSubmitRequest,
    AgentTurnCheckpoint, AgentWaitResult, ConversationRecoveryPreview, ConversationRecoveryRequest,
    ConversationRecoveryResult, ConversationRecoveryTarget, RestoredAgentRuntime, ThreadId, TurnId,
};
use crate::agent_runtime::state::AgentRuntimeError;
use crate::{AgentRoleId, ThreadEventBusHandle, ThreadEventSubscription};

/// 不包含 host 泛型的 cloneable runtime 命令句柄。
///
/// 产品 facade 与协作工具只能通过该句柄访问 agent 状态机，不能直接持有 loop state。
#[derive(Clone)]
pub struct AgentRuntimeHandle {
    pub(crate) sender: mpsc::Sender<CoordinatorCommand>,
    pub(crate) actors: AgentRegistry,
    pub(crate) thread_events: ThreadEventBusHandle,
    pub(crate) directory: AgentDirectoryHandle,
}

impl AgentRuntimeHandle {
    pub(crate) fn new(
        sender: mpsc::Sender<CoordinatorCommand>,
        actors: AgentRegistry,
        thread_events: ThreadEventBusHandle,
        directory: AgentDirectoryHandle,
    ) -> Self {
        Self {
            sender,
            actors,
            thread_events,
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

    /// 惰性驻留：把 repository 恢复出的单个 Thread 注册为 actor（幂等）。
    pub async fn restore_agent(
        &self,
        agent: RestoredAgentRuntime,
    ) -> AgentRuntimeResult<AgentSnapshot> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::RestoreAgent {
            agent: Box::new(agent),
            reply,
        })
        .await?;
        receive(receiver).await?
    }

    /// LRU 淘汰一个空闲驻留 actor；busy（活动 Turn/pending input）时拒绝。
    pub async fn evict_agent(&self, agent_id: AgentId) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(CoordinatorCommand::EvictAgent { agent_id, reply })
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
        self.send_to_actor(&agent_id, AgentLoopCommand::Submit { request, reply })
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
        let root_agent_id = root_agent_id_for(&self.directory.directory_snapshot(), &agent_id)?;
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::SubmitCurrentSession {
                root_agent_id,
                request,
                reply,
            },
        )
        .await?;
        receive(receiver).await?
    }

    /// 原子提交 resolved Interaction 与不可 steer 的后续 mailbox 输入。
    pub async fn submit_interaction_continuation(
        &self,
        agent_id: AgentId,
        request: AgentInteractionContinuationRequest,
    ) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        let root_agent_id = root_agent_id_for(&self.directory.directory_snapshot(), &agent_id)?;
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::SubmitInteractionContinuation {
                root_agent_id,
                request: Box::new(request),
                reply,
            },
        )
        .await?;
        receive(receiver).await?
    }

    /// Reconfigures the role of an active, idle agent with an empty input queue.
    ///
    /// Product hosts use this narrow transition when a durable root Thread
    /// changes execution mode. Running turns and queued input keep the role
    /// that they were created with and therefore reject reconfiguration.
    pub async fn reconfigure_idle_role(
        &self,
        agent_id: AgentId,
        role: AgentRoleId,
    ) -> AgentRuntimeResult<AgentSnapshot> {
        let (reply, receiver) = oneshot::channel();
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::ReconfigureIdleRole { role, reply },
        )
        .await?;
        receive(receiver).await?
    }

    /// 只读预览同一 Thread 的对话尾部回退或局部重建。
    pub async fn preview_conversation_recovery(
        &self,
        agent_id: AgentId,
        target: ConversationRecoveryTarget,
    ) -> AgentRuntimeResult<ConversationRecoveryPreview> {
        let (reply, receiver) = oneshot::channel();
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::PreviewConversationRecovery { target, reply },
        )
        .await?;
        receive(receiver).await?
    }

    /// 以 recovery id 幂等提交 conversation recovery。
    pub async fn recover_conversation(
        &self,
        agent_id: AgentId,
        request: ConversationRecoveryRequest,
    ) -> AgentRuntimeResult<ConversationRecoveryResult> {
        let (reply, receiver) = oneshot::channel();
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::RecoverConversation { request, reply },
        )
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
        self.send_to_actor(&agent_id, AgentLoopCommand::CancelTurn { turn_id, reply })
            .await?;
        receive(receiver).await?
    }

    pub(crate) async fn set_activity(
        &self,
        agent_id: AgentId,
        turn_id: TurnId,
        kind: ActiveKind,
    ) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::SetActivity {
                turn_id,
                kind,
                reply,
            },
        )
        .await?;
        receive(receiver).await?
    }

    pub(crate) async fn checkpoint_turn(
        &self,
        agent_id: AgentId,
        checkpoint: AgentTurnCheckpoint,
    ) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::Checkpoint {
                checkpoint: Box::new(checkpoint),
                reply,
            },
        )
        .await?;
        receive(receiver).await?
    }

    /// 将产品关联出的公共事实交给目标 agent 串行持久化和投影。
    pub async fn record_thread_facts(
        &self,
        agent_id: AgentId,
        thread_id: ThreadId,
        facts: Vec<crate::ThreadNotificationFact>,
    ) -> AgentRuntimeResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::RecordThreadFacts {
                thread_id,
                facts,
                reply,
            },
        )
        .await?;
        receive(receiver).await?
    }

    /// 更新调用 agent 的显式进度 checkpoint，并追加一条 durable 阶段提交记录。
    pub async fn report_progress(
        &self,
        agent_id: AgentId,
        stage: AgentProgressStage,
        summary: String,
        next_step: String,
        detail: Option<String>,
    ) -> AgentRuntimeResult<AgentProgressCheckpoint> {
        let (reply, receiver) = oneshot::channel();
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::ReportProgress {
                stage,
                summary,
                next_step,
                detail,
                reply,
            },
        )
        .await?;
        receive(receiver).await?
    }

    /// 读取目标 agent 唯一 canonical session 的有界、过滤摘要。
    pub async fn read_agent_session(
        &self,
        agent_id: AgentId,
    ) -> AgentRuntimeResult<AgentSessionDigest> {
        let (reply, receiver) = oneshot::channel();
        self.send_to_actor(&agent_id, AgentLoopCommand::ReadSession { reply })
            .await?;
        receive(receiver).await?
    }

    /// 读取目标 agent 的 durable 阶段提交历史（分页、不截断、关闭后可查）。
    pub async fn read_submissions(
        &self,
        agent_id: AgentId,
        offset: usize,
        limit: usize,
    ) -> AgentRuntimeResult<AgentSubmissionPage> {
        let (reply, receiver) = oneshot::channel();
        self.send_to_actor(
            &agent_id,
            AgentLoopCommand::ReadSubmissions {
                offset,
                limit,
                reply,
            },
        )
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
        self.send_to_actor(&agent_id, AgentLoopCommand::Snapshot { reply })
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

    /// 等待任一目标出现新 progress、interaction 或 terminal 事实，并返回最新增量消息。
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

    /// 订阅指定 Thread；首帧是 authoritative snapshot，随后进入实时通知 channel。
    pub fn subscribe_thread(
        &self,
        request: ThreadSubscriptionRequest,
    ) -> AgentRuntimeResult<ThreadEventSubscription> {
        self.thread_events
            .subscribe(request)
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))
    }

    /// 读取包含当前 transient overlay 的 authoritative Thread snapshot。
    pub fn thread_snapshot(&self, thread_id: &ThreadId) -> AgentRuntimeResult<ThreadSnapshot> {
        self.thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))
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

    async fn send_to_actor(
        &self,
        agent_id: &AgentId,
        command: AgentLoopCommand,
    ) -> AgentRuntimeResult<()> {
        let actor = self.actor(agent_id).await?;
        actor.send(command).await
    }

    async fn actor(&self, agent_id: &AgentId) -> AgentRuntimeResult<AgentLoopHandle> {
        self.actors
            .read()
            .await
            .get(agent_id)
            .cloned()
            .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))
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
            messages: terminal.into_iter().map(wait_message).collect(),
        });
    }
    let interactions = snapshots
        .into_iter()
        .filter(|snapshot| {
            snapshot.activity == AgentActivityState::Active(ActiveKind::WaitingInteraction)
        })
        .collect::<Vec<_>>();
    (!interactions.is_empty()).then_some(AgentDirectoryWaitResult {
        reason: AgentDirectoryWaitReason::Interaction,
        messages: interactions.into_iter().map(wait_message).collect(),
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
            } else if snapshot.activity
                == AgentActivityState::Active(ActiveKind::WaitingInteraction)
                && previous.activity != AgentActivityState::Active(ActiveKind::WaitingInteraction)
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
        messages: changed
            .into_iter()
            .filter_map(|(candidate, snapshot)| (candidate == reason).then_some(snapshot))
            .map(wait_message)
            .collect(),
    })
}

fn wait_message(snapshot: AgentSnapshot) -> AgentDirectoryWaitMessage {
    let turn_outcome = if snapshot.lifecycle != AgentLifecycleState::Active
        || snapshot.activity == AgentActivityState::Idle
    {
        snapshot.last_turn.map(|outcome| outcome.kind)
    } else {
        None
    };
    AgentDirectoryWaitMessage {
        identity: snapshot.identity,
        lifecycle: snapshot.lifecycle,
        activity: snapshot.activity,
        message: snapshot.progress,
        turn_outcome,
    }
}

async fn receive<T>(receiver: oneshot::Receiver<T>) -> AgentRuntimeResult<T> {
    receiver.await.map_err(|_| AgentRuntimeError::ChannelClosed)
}

fn root_agent_id_for(
    directory: &AgentDirectorySnapshot,
    agent_id: &AgentId,
) -> AgentRuntimeResult<AgentId> {
    let parents = directory
        .agents
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
