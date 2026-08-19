use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use pl_protocol::{
    AgentRuntimeDelta, InferenceBillingRecord, McpHealthSnapshot, ThreadRuntimeSnapshot,
    ThreadRuntimeUsage, ThreadSnapshot,
};
use tokio::sync::{Mutex, mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::{AgentSession, TurnEngine, TurnOptions, TurnRequest};

use super::{
    AgentExecutionPolicy, AgentRuntimeHandle, AgentSnapshot, DurableMailboxEnvelope, ThreadId,
    TurnId,
};

#[derive(Debug, Clone)]
pub(crate) struct TurnBudgetRefreshHandle {
    sender: watch::Sender<Option<Instant>>,
}

impl TurnBudgetRefreshHandle {
    pub(crate) fn refresh(&self) {
        self.sender.send_replace(Some(Instant::now()));
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TurnBudgetRefreshReceiver {
    receiver: Arc<StdMutex<watch::Receiver<Option<Instant>>>>,
}

impl TurnBudgetRefreshReceiver {
    pub(crate) fn take_latest(&self) -> Option<Instant> {
        let mut receiver = match self.receiver.lock() {
            Ok(receiver) => receiver,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !receiver.has_changed().unwrap_or(false) {
            return None;
        }
        *receiver.borrow_and_update()
    }
}

pub(crate) fn turn_budget_refresh_channel() -> (TurnBudgetRefreshHandle, TurnBudgetRefreshReceiver)
{
    let (sender, receiver) = watch::channel(None);
    (
        TurnBudgetRefreshHandle { sender },
        TurnBudgetRefreshReceiver {
            receiver: Arc::new(StdMutex::new(receiver)),
        },
    )
}

/// turn 完成后对 canonical Thread 的提交策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentSessionCommitPolicy {
    /// 提交本轮产生的用户、模型和工具上下文。
    #[default]
    Persist,
    /// 仅提交 outcome、usage 和 trace，丢弃本轮对 session context 的修改。
    DiscardTurn,
}

/// 宿主准备一次 turn 时可读取的稳定上下文。
#[derive(Debug, Clone)]
pub struct AgentTurnPreparationContext {
    pub snapshot: AgentSnapshot,
    pub turn_id: TurnId,
    pub thread_id: ThreadId,
    pub input: DurableMailboxEnvelope,
    pub leading_inputs: Vec<DurableMailboxEnvelope>,
    pub session: AgentSession,
    pub trace_sequence: u64,
    pub runtime: AgentRuntimeHandle,
    pub cancellation_token: CancellationToken,
    pub(crate) mailbox: AgentTurnMailboxHandle,
    pub(crate) budget_refresh: TurnBudgetRefreshReceiver,
}

struct AgentTurnMailboxState {
    receiver: mpsc::UnboundedReceiver<DurableMailboxEnvelope>,
    unacknowledged_mail_ids: Vec<String>,
}

/// 活动 turn 的进程内接收端；durable truth 仍由 actor mailbox 与 checkpoint CAS 持有。
#[derive(Clone)]
pub(crate) struct AgentTurnMailboxHandle {
    state: Arc<Mutex<AgentTurnMailboxState>>,
}

impl AgentTurnMailboxHandle {
    pub(crate) fn new(
        receiver: mpsc::UnboundedReceiver<DurableMailboxEnvelope>,
        initial_mail_ids: Vec<String>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(AgentTurnMailboxState {
                receiver,
                unacknowledged_mail_ids: initial_mail_ids,
            })),
        }
    }

    pub(crate) async fn drain(&self) -> Vec<DurableMailboxEnvelope> {
        let mut state = self.state.lock().await;
        let mut inputs = Vec::new();
        while let Ok(input) = state.receiver.try_recv() {
            if !state
                .unacknowledged_mail_ids
                .iter()
                .any(|mail_id| mail_id == &input.mail_id)
            {
                state.unacknowledged_mail_ids.push(input.mail_id.clone());
            }
            inputs.push(input);
        }
        inputs
    }

    pub(crate) async fn pending_acknowledgements(&self) -> Vec<String> {
        self.state.lock().await.unacknowledged_mail_ids.clone()
    }

    pub(crate) async fn acknowledge(&self, mail_ids: &[String]) {
        if mail_ids.is_empty() {
            return;
        }
        self.state
            .lock()
            .await
            .unacknowledged_mail_ids
            .retain(|mail_id| !mail_ids.iter().any(|acknowledged| acknowledged == mail_id));
    }
}

impl std::fmt::Debug for AgentTurnMailboxHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentTurnMailboxHandle")
            .finish_non_exhaustive()
    }
}

/// 宿主为 runtime 准备好的可执行 turn。
#[derive(Debug)]
pub struct PreparedAgentTurn {
    pub(crate) engine: TurnEngine,
    pub(crate) request: TurnRequest,
    pub(crate) options: TurnOptions,
    pub(crate) policy: AgentExecutionPolicy,
    pub(crate) session_commit: AgentSessionCommitPolicy,
    pub(crate) pinned_context: Vec<crate::PinnedContextSection>,
    pub(crate) session_runtime: Option<PreparedSessionRuntime>,
}

impl PreparedAgentTurn {
    /// 创建 prepared turn；runtime 会覆盖 turn id 与 cancellation token。
    pub fn new(
        engine: TurnEngine,
        request: TurnRequest,
        options: TurnOptions,
        policy: AgentExecutionPolicy,
    ) -> Self {
        Self {
            engine,
            request,
            options,
            policy,
            session_commit: AgentSessionCommitPolicy::Persist,
            pinned_context: Vec::new(),
            session_runtime: None,
        }
    }

    /// 设置 turn 完成后的 canonical session 提交策略。
    pub fn with_session_commit(mut self, policy: AgentSessionCommitPolicy) -> Self {
        self.session_commit = policy;
        self
    }

    /// 在模型 turn 启动前写入产品提供的 canonical pinned context。
    pub fn with_pinned_context(mut self, section: crate::PinnedContextSection) -> Self {
        self.pinned_context.push(section);
        self
    }

    /// 声明本轮已解析并实际安装的模型、MCP 与 LSP runtime 元数据。
    ///
    /// runtime 会在模型执行前把它合并进 canonical session snapshot；产品 host 不得自行
    /// 分配 session event sequence 或广播另一套 UI 事件。
    pub fn with_session_runtime(mut self, runtime: PreparedSessionRuntime) -> Self {
        self.session_runtime = Some(runtime);
        self
    }

    pub(crate) fn with_runtime_context(
        mut self,
        turn_id: &TurnId,
        cancellation: CancellationToken,
        checkpoint: AgentTurnCheckpointHandle,
        mailbox: AgentTurnMailboxHandle,
        budget_refresh: TurnBudgetRefreshReceiver,
    ) -> Self {
        self.request.turn_id = Some(turn_id.to_string());
        self.options.cancellation_token = Some(cancellation);
        self.options.execution_policy = Some(self.policy.clone());
        self.options.checkpoint = Some(checkpoint);
        self.options.mailbox = Some(mailbox);
        self.options.budget_refresh = Some(budget_refresh);
        self
    }
}

/// 产品 host 为一次 turn 准备完成后交给 PL 投影的 session runtime 元数据。
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedSessionRuntime {
    pub model: String,
    pub context_window: Option<u64>,
    pub active_mcp_servers: Vec<String>,
    pub active_lsp_servers: Vec<String>,
    pub agent_count: u32,
    pub mcp_health: Option<McpHealthSnapshot>,
    /// 本 Turn 工具 lease 冻结的注册表全局发布代数；仅诊断，不参与缓存轮换。
    pub tool_registry_revision: Option<u64>,
    /// 本 Turn 工具 lease 冻结的 deferred Tool Search catalog 指纹；仅诊断。
    pub tool_catalog_hash: Option<String>,
}

impl PreparedSessionRuntime {
    /// 以实际发送给 provider 的模型标识创建元数据；其余能力由 host 按已安装资源补充。
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            context_window: None,
            active_mcp_servers: Vec::new(),
            active_lsp_servers: Vec::new(),
            agent_count: 1,
            mcp_health: None,
            tool_registry_revision: None,
            tool_catalog_hash: None,
        }
    }

    pub fn with_context_window(mut self, context_window: u64) -> Self {
        self.context_window = Some(context_window);
        self
    }

    pub fn with_mcp_servers(mut self, active_servers: Vec<String>) -> Self {
        self.active_mcp_servers = active_servers;
        self
    }

    pub fn with_mcp_health(mut self, health: McpHealthSnapshot) -> Self {
        self.mcp_health = Some(health);
        self
    }

    pub fn with_lsp(mut self, active_servers: Vec<String>) -> Self {
        self.active_lsp_servers = active_servers;
        self
    }

    pub fn with_agent_count(mut self, agent_count: u32) -> Self {
        self.agent_count = agent_count.max(1);
        self
    }

    /// 声明本 Turn 工具 lease 冻结的注册表代数与 deferred catalog 指纹。
    ///
    /// 两个值只作诊断投影，与 prompt cache 快照中的同名字段是独立事实层；host
    /// 通常从最近一次冻结的 prompt snapshot 读取并透传。
    pub fn with_tool_diagnostics(
        mut self,
        registry_revision: Option<u64>,
        catalog_hash: Option<String>,
    ) -> Self {
        self.tool_registry_revision = registry_revision;
        self.tool_catalog_hash = catalog_hash;
        self
    }

    pub(crate) fn merge_with(
        &self,
        thread_id: &ThreadId,
        current: &ThreadSnapshot,
        updated_at: i64,
    ) -> ThreadRuntimeSnapshot {
        let usage = current.runtime.as_ref().map_or_else(
            || ThreadRuntimeUsage {
                model: self.model.clone(),
                context_window: self.context_window,
                latest_context_tokens: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                cached_prompt_tokens: 0,
                cache_write_tokens: 0,
                cache_miss_tokens: 0,
                reasoning_tokens: 0,
                inference_count: 0,
                total_tokens: 0,
                cache_hit_rate: None,
                estimated_costs: Vec::new(),
                estimated_cache_savings: Vec::new(),
                has_unpriced_usage: false,
                prompt_generation: None,
                prompt_cache_policy: None,
                prefix_changed_reason: None,
                updated_at,
            },
            |current_runtime| {
                let mut usage = current_runtime.usage.clone();
                usage.model.clone_from(&self.model);
                usage.context_window = self.context_window;
                usage.updated_at = updated_at;
                usage
            },
        );
        let active_skills = current
            .runtime
            .as_ref()
            .map_or_else(Vec::new, |value| value.active_skills.clone());
        ThreadRuntimeSnapshot {
            thread_id: thread_id.to_string(),
            usage,
            todo: current
                .runtime
                .as_ref()
                .and_then(|value| value.todo.clone()),
            active_skills,
            active_mcp_servers: self.active_mcp_servers.clone(),
            active_lsp_servers: self.active_lsp_servers.clone(),
            progress: current
                .runtime
                .as_ref()
                .and_then(|value| value.progress.clone()),
            mcp_health: self.mcp_health.clone(),
            tool_registry_revision: self.tool_registry_revision,
            tool_catalog_hash: self.tool_catalog_hash.clone(),
            updated_at,
        }
    }
}

#[cfg(test)]
mod prepared_session_runtime_tests {
    use pl_protocol::{ThreadRuntimeSnapshot, ThreadRuntimeUsage, ThreadSnapshot};
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn prepared_runtime_replaces_resources_without_resetting_cumulative_usage() {
        let mut current = ThreadSnapshot::empty("session");
        current.runtime = Some(ThreadRuntimeSnapshot {
            thread_id: "session".to_string(),
            usage: ThreadRuntimeUsage {
                model: "old-model".to_string(),
                context_window: Some(10),
                latest_context_tokens: 7,
                prompt_tokens: 5,
                completion_tokens: 2,
                cached_prompt_tokens: 1,
                cache_write_tokens: 0,
                cache_miss_tokens: 4,
                reasoning_tokens: 0,
                inference_count: 1,
                total_tokens: 7,
                cache_hit_rate: Some(0.2),
                estimated_costs: Vec::new(),
                estimated_cache_savings: Vec::new(),
                has_unpriced_usage: false,
                prompt_generation: None,
                prompt_cache_policy: None,
                prefix_changed_reason: None,
                updated_at: 1,
            },
            active_skills: vec!["review".to_string()],
            active_mcp_servers: vec!["old-mcp".to_string()],
            active_lsp_servers: vec!["old-lsp".to_string()],
            todo: None,
            progress: None,
            mcp_health: None,
            tool_registry_revision: Some(3),
            tool_catalog_hash: Some("stale-catalog".to_string()),
            updated_at: 1,
        });
        let prepared = PreparedSessionRuntime::new("new-model")
            .with_context_window(128_000)
            .with_mcp_servers(vec!["search".to_string()])
            .with_lsp(vec!["rust-analyzer".to_string()])
            .with_agent_count(2)
            .with_tool_diagnostics(Some(9), Some("catalog-v9".to_string()));

        let merged = prepared.merge_with(&ThreadId::new("session").unwrap(), &current, 9);

        assert_eq!(merged.usage.model, "new-model");
        assert_eq!(merged.usage.context_window, Some(128_000));
        assert_eq!(merged.usage.total_tokens, 7);
        assert_eq!(merged.active_skills, vec!["review".to_string()]);
        assert_eq!(merged.active_mcp_servers, vec!["search".to_string()]);
        assert_eq!(merged.active_lsp_servers, vec!["rust-analyzer".to_string()]);
        assert_eq!(merged.tool_registry_revision, Some(9));
        assert_eq!(merged.tool_catalog_hash.as_deref(), Some("catalog-v9"));
        assert_eq!(merged.updated_at, 9);
    }
}

/// mid-turn durable Thread checkpoint 的触发原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnCheckpointReason {
    WorkingSetChanged,
    BeforeInference,
    InferenceCompleted,
    ContextCompacted,
    MailboxInputConsumed,
    Terminal,
}

/// 一次模型调用完成后必须与上下文原子提交的计费与 runtime 增量。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentInferenceCommit {
    pub billing: InferenceBillingRecord,
    pub runtime_delta: AgentRuntimeDelta,
}

/// worker 交给 actor 做 active-turn 与 sequence 校验的 Thread checkpoint。
#[derive(Debug, Clone)]
pub struct AgentTurnCheckpoint {
    pub turn_id: TurnId,
    pub thread_id: ThreadId,
    pub sequence: u64,
    pub session: AgentSession,
    pub reason: TurnCheckpointReason,
    pub consumed_mail_ids: Vec<String>,
    pub inference: Option<AgentInferenceCommit>,
}

/// TurnEngine 使用的 durable checkpoint 命令句柄。
#[derive(Clone)]
pub struct AgentTurnCheckpointHandle {
    runtime: AgentRuntimeHandle,
    agent_id: super::AgentId,
    turn_id: TurnId,
    thread_id: ThreadId,
    sequence: Arc<AtomicU64>,
}

impl AgentTurnCheckpointHandle {
    pub(crate) fn new(
        runtime: AgentRuntimeHandle,
        agent_id: super::AgentId,
        turn_id: TurnId,
        thread_id: ThreadId,
    ) -> Self {
        Self {
            runtime,
            agent_id,
            turn_id,
            thread_id,
            sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn checkpoint(
        &self,
        session: AgentSession,
        reason: TurnCheckpointReason,
    ) -> super::AgentRuntimeResult<()> {
        self.checkpoint_mailbox(session, reason, Vec::new()).await
    }

    /// 等待 inference 的上下文、usage、价格快照与 runtime 投影完成 durable 提交。
    pub async fn commit_inference(
        &self,
        session: AgentSession,
        inference: AgentInferenceCommit,
    ) -> super::AgentRuntimeResult<()> {
        self.checkpoint_inference_mailbox(session, inference, Vec::new())
            .await
    }

    pub(crate) async fn checkpoint_mailbox(
        &self,
        session: AgentSession,
        reason: TurnCheckpointReason,
        consumed_mail_ids: Vec<String>,
    ) -> super::AgentRuntimeResult<()> {
        self.checkpoint_with(session, reason, consumed_mail_ids, None)
            .await
    }

    pub(crate) async fn checkpoint_inference_mailbox(
        &self,
        session: AgentSession,
        inference: AgentInferenceCommit,
        consumed_mail_ids: Vec<String>,
    ) -> super::AgentRuntimeResult<()> {
        self.checkpoint_with(
            session,
            TurnCheckpointReason::InferenceCompleted,
            consumed_mail_ids,
            Some(inference),
        )
        .await
    }

    async fn checkpoint_with(
        &self,
        session: AgentSession,
        reason: TurnCheckpointReason,
        consumed_mail_ids: Vec<String>,
        inference: Option<AgentInferenceCommit>,
    ) -> super::AgentRuntimeResult<()> {
        let sequence = self
            .sequence
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.runtime
            .checkpoint_turn(
                self.agent_id.clone(),
                AgentTurnCheckpoint {
                    turn_id: self.turn_id.clone(),
                    thread_id: self.thread_id.clone(),
                    sequence,
                    session,
                    reason,
                    consumed_mail_ids,
                    inference,
                },
            )
            .await
    }
}

impl std::fmt::Debug for AgentTurnCheckpointHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentTurnCheckpointHandle")
            .field("agent_id", &self.agent_id)
            .field("turn_id", &self.turn_id)
            .field("thread_id", &self.thread_id)
            .finish_non_exhaustive()
    }
}
