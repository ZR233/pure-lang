use crate::InteractionRequest;
use anyhow::Result;

use crate::config::ConfigRuntime;
use crate::studio::records::ThreadRecord;
use crate::studio::{ProductEventBus, StudioActiveTurn, StudioRuntimeState, StudioStore};
use pl_protocol::studio::StudioPromptInput;
use pl_tool::mcp::McpRuntimeHandle;

mod attachment_drafts;
mod background_task;
mod history;
mod lifecycle;
mod lsp_state;
mod mcp_health;
mod model_performance;
mod model_refresh;
mod prompt_runner;
mod provider_usage;
mod rejected_tools;
mod residency;
mod settings_api;
mod shutdown_progress;
mod skill_catalog;
mod ssh;
mod state_query;
mod thread_mode;
mod thread_service;
mod thread_stream;
pub(in crate::studio) mod timeline;
mod tool_refresh;
pub use thread_stream::StudioThreadSubscription;
mod thread_observation;
mod thread_title;
mod updater;

pub(crate) use model_performance::ModelPerformanceOwner;
pub(crate) use provider_usage::ProviderUsageRuntime;
pub use provider_usage::{ProviderUsageStateData, ProviderUsageStateSnapshot};
pub(crate) use shutdown_progress::ShutdownProgressBus;
pub(crate) use skill_catalog::SkillCatalogRuntime;
pub use skill_catalog::{SkillSearchResult, SkillsStateSnapshot};
use thread_title::ThreadTitleTasks;
pub(crate) use updater::StudioUpdateRuntime;
pub use updater::*;

/// Studio UI 提交 prompt 的请求。
///
/// runtime 只负责产品投影；Turn ID、FIFO、取消与 canonical Thread 全部由
/// `pl_core::thread::ThreadHandle` 对应的 owner 管理。
pub struct StudioSubmitPromptRequest {
    pub thread_id: String,
    pub input: StudioPromptInput,
    pub options: StudioSubmitPromptOptions,
}

/// Creates a root Thread with the requested mode and submits its first prompt as one product command.
pub struct StudioStartNewThreadRequest {
    pub project_id: String,
    pub title: Option<String>,
    pub input: StudioPromptInput,
    pub mode: pl_protocol::ThreadModeId,
    pub workspace_mode: pl_protocol::ThreadWorkspaceMode,
    pub options: StudioSubmitPromptOptions,
}

/// Studio UI 提交 prompt 的附加选项。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudioSubmitPromptOptions {
    pub presentation: pl_protocol::MessagePresentation,
}

/// Studio 输入受理回执；实际 Turn 关联由 Thread 事实流提供。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioSubmitPromptResponse {
    pub thread_id: String,
    pub input_id: String,
    pub cursor: u64,
}

/// Result of creating a root Thread and accepting its first Turn.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioStartNewThreadResponse {
    pub thread: ThreadRecord,
    pub submission: StudioSubmitPromptResponse,
}

/// Result of archiving a root Thread tree.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioArchiveThreadResult {
    pub archived_root_id: String,
    pub removed_thread_ids: Vec<String>,
    pub next_root: Option<ThreadRecord>,
}

/// Studio UI 请求停止当前 Thread Turn 后的结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioStopPromptResponse {
    pub thread_id: String,
    pub stopped: bool,
}

/// Result of validating and interrupting the expected active Turn.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioInterruptPromptResponse {
    pub thread_id: String,
    pub turn_id: String,
    pub interrupted: bool,
}

/// Studio UI resolve interaction 后的核心响应。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioResolveInteractionResponse {
    pub thread_id: String,
    pub interaction: InteractionRequest,
}

/// 归档前「结束会话树活动工作」的有界等待上界。
///
/// 中断当前 Turn 与丢弃未消费输入只需取消进程内取消令牌；运行中任务的收束需要等其
/// 拥有的资源（例如工具进程树）终止，因此上界留出余量；但仍必须有限：超过上界仍未
/// 收束说明现场无法安全结束，归档必须显式失败并保留会话（design/01 §1.4）。
pub(in crate::studio::runtime) const ARCHIVE_TREE_SETTLE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(10);

#[derive(Clone)]
pub struct StudioRuntime {
    startup_observer: std::sync::Arc<dyn Fn(crate::StudioStartupStage) + Send + Sync>,
    thread_observations: thread_observation::ThreadObservations,
    settings_updates: tokio::sync::watch::Sender<crate::config::ConfigRuntimeSnapshot>,
    settings_refresh: background_task::BackgroundTaskSlot,
    tool_refresh: background_task::BackgroundTaskSlot,
    rejected_tools: rejected_tools::RejectedToolOwners,
    tool_catalog_updates: std::sync::Arc<tokio::sync::Notify>,
    threads: crate::thread_assembler::StudioThreadAssembler,
    thread_factory: crate::studio::thread_factory::StudioThreadFactory,
    instance_lock: super::runtime_lock::RuntimeLockOwner,
    store: StudioStore,
    config_runtime: ConfigRuntime,
    external_runtimes: StudioExternalRuntimes,
    agent_facility: StudioAgentFacility,
    residency: residency::ThreadResidency,
    shutdown_progress: ShutdownProgressBus,
    runtime_state: StudioRuntimeState,
    recovery: crate::studio::StudioRecoveryRegistry,
    recovery_task: background_task::BackgroundTaskSlot,
    #[cfg(test)]
    recovery_gate: std::sync::Arc<tokio::sync::Mutex<()>>,
    /// 测试可注入的归档收束窗口；生产固定使用 `ARCHIVE_TREE_SETTLE_TIMEOUT`。
    #[cfg(test)]
    archive_settle_timeout: std::time::Duration,
    skills: SkillCatalogRuntime,
    thread_modes: crate::mode::ThreadModeManager,
    provider_usage: ProviderUsageRuntime,
    model_performance: ModelPerformanceOwner,
    updater: StudioUpdateRuntime,
    activation: ProjectActivationRuntime,
    attachment_drafts: attachment_drafts::AttachmentDraftRuntime,
    ssh_manager: std::sync::Arc<pl_tool::remote::SshManager>,
    lifecycle_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    title_tasks: ThreadTitleTasks,
}

#[derive(Clone)]
struct StudioExternalRuntimes {
    mcp: McpRuntimeHandle,
    mcp_state: mcp_health::McpStateRuntime,
    mcp_startup_reconcile: background_task::BackgroundTaskSlot,
    mcp_health_watcher: background_task::BackgroundTaskSlot,
    lsp: pl_lsp::runtime::LspRuntimeRegistry,
    lsp_state: lsp_state::LspStateRuntime,
    lsp_state_watcher: background_task::BackgroundTaskSlot,
}

#[derive(Clone)]
struct StudioAgentFacility {
    worktrees: crate::studio::agent_host::worktree_lease::WorktreeLeaseOwner,
    product_events: ProductEventBus,
    /// agent framework 的 write-behind writer 句柄；framework 被 take 后关机仍能排空。
    persistence: std::sync::Arc<
        tokio::sync::Mutex<Option<crate::studio::agent_host::ThreadWriteBehindWriter>>,
    >,
}

#[derive(Clone, Default)]
struct ProjectActivationRuntime {
    command_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    applied: std::sync::Arc<tokio::sync::RwLock<Option<ProjectActivation>>>,
}

#[derive(Clone, PartialEq, Eq)]
struct ProjectActivation {
    project_id: String,
    fingerprint: String,
}

impl StudioRuntime {
    /// 归档前结束会话树活动工作的有界等待上界；测试可注入更短的窗口。
    fn archive_settle_timeout(&self) -> std::time::Duration {
        #[cfg(test)]
        {
            self.archive_settle_timeout
        }
        #[cfg(not(test))]
        {
            ARCHIVE_TREE_SETTLE_TIMEOUT
        }
    }

    /// 返回当前配置目录中可用的 Agent Profile 快照。
    pub fn read_agent_profiles(&self) -> Result<crate::config::AgentProfileCatalog> {
        Ok(self.config_runtime.agent_profiles_for_settings()?)
    }

    /// 原子创建或保存一个用户 Agent Profile TOML。
    pub fn save_user_agent_profile(
        &self,
        expected_settings_revision: u64,
        profile_id: &str,
        profile: &crate::config::UserAgentProfile,
    ) -> Result<pl_protocol::studio::StudioSettingsSnapshot> {
        let state = self.config_runtime.save_user_agent_profile(
            expected_settings_revision,
            profile_id,
            profile,
        )?;
        self.publish_settings_state(state.clone())?;
        settings_api::settings_snapshot(state)
    }

    /// 启用或禁用不可编辑、不可删除的系统 Agent Profile。
    pub fn set_system_agent_enabled(
        &self,
        expected_settings_revision: u64,
        profile_id: &str,
        enabled: bool,
    ) -> Result<pl_protocol::studio::StudioSettingsSnapshot> {
        if !crate::config::is_system_profile_id(profile_id) {
            anyhow::bail!("`{profile_id}` is not a system Agent Profile");
        }
        let profile_id = profile_id.to_string();
        let state = self
            .config_runtime
            .update(expected_settings_revision, |config| {
                let mut config = config.clone();
                if enabled {
                    config.disabled_system_agents.remove(&profile_id);
                } else {
                    config.disabled_system_agents.insert(profile_id.clone());
                }
                Ok(config)
            })?;
        self.publish_settings_state(state.clone())?;
        settings_api::settings_snapshot(state)
    }

    /// 立即重试待落库事实；查询和停止路径不需要调用本命令。
    pub async fn retry_persistence(&self) -> Result<crate::PersistenceStateSnapshot> {
        let persistence = self.agent_facility.persistence.lock().await.clone();
        let Some(persistence) = persistence else {
            return Ok(self.agent_facility.product_events.persistence_state());
        };
        persistence.retry_now();
        self.store.thread_persistence().retry_now();
        Ok(self.agent_facility.product_events.persistence_state())
    }

    /// 进程级持久化队列压力与逐 Thread 水位。
    ///
    /// 唯一事实源是持有全部 per-Thread writer 的持久化协调器；本入口只是把它已观测到的
    /// 真实值投影为协议形态，不读取、不聚合、也不编造任何 GUI 本地计数。队列字节、最老
    /// 待保存年龄、在途字节与最近错误都直接来自协调器，缺失即为未知而非零。
    pub fn persistence_queue_snapshot(&self) -> pl_protocol::PersistenceQueueSnapshot {
        self.store.thread_persistence().queue_snapshot()
    }

    /// Returns whether an active turn prevents a safe application update.
    pub async fn is_busy_for_update(&self) -> Result<bool> {
        Ok(!self.derive_active_turns().await?.is_empty())
    }

    /// 从 agent framework 派生当前所有活动 turn。
    ///
    /// 活动 turn 列表不再手工维护：canonical source 是每个 agent 的
    /// 旧 Agent 镜像字段。这里聚合整棵 agent tree 的活动 turn，
    /// 用于 idle 判断。UI 不消费此列表（它从 per-thread 流读取 busy 状态）。
    async fn derive_active_turns(&self) -> Result<Vec<StudioActiveTurn>> {
        Ok(self
            .threads
            .observed_threads()
            .into_iter()
            .flat_map(|(thread_id, handle)| {
                handle
                    .snapshot()
                    .turns
                    .iter()
                    .filter(|turn| turn.state == pl_core::thread::TurnState::Running)
                    .map(|turn| StudioActiveTurn {
                        thread_id: thread_id.clone(),
                        turn_id: turn.turn_id.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    async fn close_project_agent_trees(&self, thread_ids: &[String]) -> Result<()> {
        let mut roots = std::collections::BTreeSet::new();
        for id in thread_ids {
            roots.insert(self.read_owned_thread(id).await?.root_thread_id);
        }
        for root in roots {
            self.threads.close_tree(&root).await?;
        }
        Ok(())
    }
}

// Legacy Task orchestration tests were removed with the fixed Task runtime.
