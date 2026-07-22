use crate::InteractionRequest;
use anyhow::Result;

use crate::McpRuntimeHandle;
use crate::config::ConfigStore;
use crate::studio::agent_host::{
    StudioAgentResources, StudioAgentRuntime, StudioContinuationService,
};
use crate::studio::records::SessionRecord;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{
    InteractionRuntime, StudioProductEventRuntime, StudioRuntimeState, StudioStore,
};

mod lifecycle;
mod mcp_health;
mod plan_confirmation;
mod prompt_runner;
mod session_service;

/// Studio UI 提交 prompt 的请求。
///
/// runtime 只负责产品投影；turn ID、FIFO、取消与 canonical session 全部由
/// `pl_core::AgentRuntime` 管理。
pub struct StudioSubmitPromptRequest {
    pub session_id: String,
    pub prompt: String,
    pub attachment_ids: Vec<String>,
    pub options: StudioSubmitPromptOptions,
}

/// Studio UI 提交 prompt 的附加选项。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudioSubmitPromptOptions {
    pub user_prompt: StudioUserPromptPresentation,
    pub lifecycle: Option<StudioPlanImplementationLifecycle>,
}

/// 用户 prompt 在 Studio timeline 中的展示方式。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum StudioUserPromptPresentation {
    #[default]
    Normal,
    SyntheticVisible {
        visible_prompt: String,
    },
    SyntheticIgnored {
        visible_prompt: String,
    },
}

/// 计划实施 turn 的生命周期关联。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioPlanImplementationLifecycle {
    pub session_id: String,
    pub plan_id: String,
}

/// Studio UI 提交 prompt 后得到的 framework turn 信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioSubmitPromptResponse {
    pub session_id: String,
    pub turn_id: String,
    pub cursor: u64,
}

/// Studio UI 请求停止当前会话 turn 后的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioStopPromptResponse {
    pub session_id: String,
    pub stopped: bool,
}

/// Studio UI resolve interaction 后的核心响应。
#[derive(Debug, Clone, PartialEq)]
pub struct StudioResolveInteractionResponse {
    pub session_id: String,
    pub interaction: InteractionRequest,
    pub sessions: Vec<SessionRecord>,
}

#[derive(Clone)]
pub struct StudioRuntime {
    store: StudioStore,
    config_store: ConfigStore,
    mcp_runtime: McpRuntimeHandle,
    mcp_health_watcher: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    lsp_runtime: pl_lsp::LspRuntimeRegistry,
    interactions: InteractionRuntime,
    product_events: StudioProductEventRuntime,
    runtime_state: StudioRuntimeState,
    agent_framework: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<StudioAgentRuntime>>>>,
    agent_resources: StudioAgentResources,
    continuations: StudioContinuationService,
    task_coordinator: std::sync::Arc<TaskCoordinator>,
    lifecycle_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    #[cfg(test)]
    initialization_entry_barrier: Option<std::sync::Arc<tokio::sync::Barrier>>,
}

impl StudioRuntime {
    /// Returns whether a turn or durable task prevents a safe application update.
    pub async fn is_busy_for_update(&self) -> Result<bool> {
        if !self.runtime_snapshot().active_turns.is_empty() {
            return Ok(true);
        }
        Ok(!self.store.list_active_task_runs().await?.is_empty())
    }

    pub async fn session_task_view(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::StudioTaskRuntime>> {
        super::task_projection::load_task_runtime(&self.store, session_id).await
    }
}

#[cfg(test)]
mod tests;
