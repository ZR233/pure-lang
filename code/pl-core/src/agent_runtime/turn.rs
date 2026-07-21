use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_protocol::{
    McpHealthSnapshot, SessionRuntimeSnapshot, SessionRuntimeUsage, SessionViewSnapshot,
};
use tokio_util::sync::CancellationToken;

use crate::{AgentKernel, AgentSession, TurnOptions, TurnRequest};

use super::{
    AgentExecutionPolicy, AgentRuntimeHandle, AgentSnapshot, PendingAgentInput, SessionId, TurnId,
};

/// turn 完成后对 canonical session 的提交策略。
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
    pub session_id: SessionId,
    pub input: PendingAgentInput,
    pub session: AgentSession,
    pub trace_sequence: u64,
    pub runtime: AgentRuntimeHandle,
    pub cancellation_token: CancellationToken,
}

/// 宿主为 runtime 准备好的可执行 turn。
#[derive(Debug)]
pub struct PreparedAgentTurn {
    pub(crate) kernel: AgentKernel,
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
        kernel: AgentKernel,
        request: TurnRequest,
        options: TurnOptions,
        policy: AgentExecutionPolicy,
    ) -> Self {
        Self {
            kernel,
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
    ) -> Self {
        self.request.turn_id = Some(turn_id.to_string());
        self.options.cancellation_token = Some(cancellation);
        self.options.execution_policy = Some(self.policy.clone());
        self.options.checkpoint = Some(checkpoint);
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

    pub(crate) fn merge_with(
        &self,
        session_id: &SessionId,
        current: &SessionViewSnapshot,
        updated_at: i64,
    ) -> SessionRuntimeSnapshot {
        let usage = current.runtime.as_ref().map_or_else(
            || SessionRuntimeUsage {
                model: self.model.clone(),
                context_window: self.context_window,
                latest_context_tokens: 0,
                prompt_tokens: 0,
                completion_tokens: 0,
                cached_prompt_tokens: 0,
                total_tokens: 0,
                cache_hit_rate: None,
                estimated_costs: Vec::new(),
                has_unpriced_usage: false,
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
        let mut active_skills = current
            .runtime
            .as_ref()
            .map_or_else(Vec::new, |value| value.active_skills.clone());
        for activation in &current.activated_skills {
            if !active_skills.contains(&activation.name) {
                active_skills.push(activation.name.clone());
            }
        }
        SessionRuntimeSnapshot {
            session_id: session_id.to_string(),
            usage,
            active_skills,
            active_mcp_servers: self.active_mcp_servers.clone(),
            active_lsp_servers: self.active_lsp_servers.clone(),
            agent_count: self
                .agent_count
                .max(current.agents.len().try_into().unwrap_or(u32::MAX))
                .max(1),
            mcp_health: self.mcp_health.clone(),
            updated_at,
        }
    }
}

#[cfg(test)]
mod prepared_session_runtime_tests {
    use pl_protocol::{SessionRuntimeSnapshot, SessionRuntimeUsage, SessionViewSnapshot};
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn prepared_runtime_replaces_resources_without_resetting_cumulative_usage() {
        let mut current = SessionViewSnapshot::empty("session");
        current.runtime = Some(SessionRuntimeSnapshot {
            session_id: "session".to_string(),
            usage: SessionRuntimeUsage {
                model: "old-model".to_string(),
                context_window: Some(10),
                latest_context_tokens: 7,
                prompt_tokens: 5,
                completion_tokens: 2,
                cached_prompt_tokens: 1,
                total_tokens: 7,
                cache_hit_rate: Some(0.2),
                estimated_costs: Vec::new(),
                has_unpriced_usage: false,
                updated_at: 1,
            },
            active_skills: vec!["review".to_string()],
            active_mcp_servers: vec!["old-mcp".to_string()],
            active_lsp_servers: vec!["old-lsp".to_string()],
            agent_count: 1,
            mcp_health: None,
            updated_at: 1,
        });
        let prepared = PreparedSessionRuntime::new("new-model")
            .with_context_window(128_000)
            .with_mcp_servers(vec!["search".to_string()])
            .with_lsp(vec!["rust-analyzer".to_string()])
            .with_agent_count(2);

        let merged = prepared.merge_with(&SessionId::new("session").unwrap(), &current, 9);

        assert_eq!(merged.usage.model, "new-model");
        assert_eq!(merged.usage.context_window, Some(128_000));
        assert_eq!(merged.usage.total_tokens, 7);
        assert_eq!(merged.active_skills, vec!["review".to_string()]);
        assert_eq!(merged.active_mcp_servers, vec!["search".to_string()]);
        assert_eq!(merged.active_lsp_servers, vec!["rust-analyzer".to_string()]);
        assert_eq!(merged.agent_count, 2);
        assert_eq!(merged.updated_at, 9);
    }
}

/// mid-turn durable session checkpoint 的触发原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnCheckpointReason {
    WorkingSetChanged,
    BeforeInference,
    ContextCompacted,
    Terminal,
}

/// worker 交给 actor 做 active-turn 与 sequence 校验的 session checkpoint。
#[derive(Debug, Clone)]
pub struct AgentTurnCheckpoint {
    pub turn_id: TurnId,
    pub session_id: SessionId,
    pub sequence: u64,
    pub session: AgentSession,
    pub reason: TurnCheckpointReason,
}

/// TurnEngine 使用的 durable checkpoint 命令句柄。
#[derive(Clone)]
pub struct AgentTurnCheckpointHandle {
    runtime: AgentRuntimeHandle,
    agent_id: super::AgentId,
    turn_id: TurnId,
    session_id: SessionId,
    sequence: Arc<AtomicU64>,
}

impl AgentTurnCheckpointHandle {
    pub(crate) fn new(
        runtime: AgentRuntimeHandle,
        agent_id: super::AgentId,
        turn_id: TurnId,
        session_id: SessionId,
    ) -> Self {
        Self {
            runtime,
            agent_id,
            turn_id,
            session_id,
            sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn checkpoint(
        &self,
        session: AgentSession,
        reason: TurnCheckpointReason,
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
                    session_id: self.session_id.clone(),
                    sequence,
                    session,
                    reason,
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
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}
