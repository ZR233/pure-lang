use std::collections::{BTreeMap, BTreeSet};

use crate::InteractionRequest;
use anyhow::Result;
use futures::FutureExt;

use crate::McpRuntimeHandle;
use crate::config::ConfigRuntime;
use crate::studio::agent_host::{StudioAgentResources, StudioAgentRuntime, root_agent_id};
use crate::studio::records::ThreadRecord;
use crate::studio::{
    InteractionService, ProductEventBus, StudioActiveTurn, StudioRuntimeState, StudioStore,
};
use pl_protocol::studio::StudioPromptInput;

mod attachment_drafts;
mod history;
mod interaction_continuation;
mod lifecycle;
mod lsp_state;
mod mcp_health;
mod model_performance;
mod prompt_runner;
mod provider_usage;
mod remote_helper;
mod residency;
mod settings_api;
mod shutdown_progress;
mod skill_catalog;
mod ssh;
mod state_query;
mod thread_service;
mod thread_title;
mod updater;

pub(crate) use model_performance::ModelPerformanceOwner;
pub(in crate::studio) use model_performance::{MODEL_PERFORMANCE_OWNER_ID, ModelPerformanceState};
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
/// `pl_core::AgentRuntime` 管理。
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
    pub options: StudioSubmitPromptOptions,
}

/// Studio UI 提交 prompt 的附加选项。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudioSubmitPromptOptions {
    pub presentation: pl_core::MessagePresentation,
    pub turn_policy: pl_core::AgentTurnSubmitPolicy,
}

/// Studio UI 提交 prompt 后得到的 framework turn 信息。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioSubmitPromptResponse {
    pub thread_id: String,
    pub turn_id: String,
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

#[derive(Clone)]
pub struct StudioRuntime {
    instance_lock: super::runtime_lock::RuntimeLockOwner,
    store: StudioStore,
    config_runtime: ConfigRuntime,
    external_runtimes: StudioExternalRuntimes,
    agent_facility: StudioAgentFacility,
    residency: residency::ThreadResidency,
    shutdown_progress: ShutdownProgressBus,
    runtime_state: StudioRuntimeState,
    recovery: crate::studio::StudioRecoveryRegistry,
    skills: SkillCatalogRuntime,
    thread_modes: pl_core::ThreadModeManager,
    provider_usage: ProviderUsageRuntime,
    model_performance: ModelPerformanceOwner,
    updater: StudioUpdateRuntime,
    activation: ProjectActivationRuntime,
    attachment_drafts: attachment_drafts::AttachmentDraftRuntime,
    ssh_manager: std::sync::Arc<pl_core::remote::SshManager>,
    lifecycle_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    title_tasks: ThreadTitleTasks,
}

#[derive(Clone)]
struct StudioExternalRuntimes {
    mcp: McpRuntimeHandle,
    mcp_state: mcp_health::McpStateRuntime,
    mcp_startup_reconcile: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    mcp_health_watcher: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    lsp: pl_lsp::runtime::LspRuntimeRegistry,
    lsp_state: lsp_state::LspStateRuntime,
    lsp_state_watcher: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

#[derive(Clone)]
struct StudioAgentFacility {
    framework: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<StudioAgentRuntime>>>>,
    resources: StudioAgentResources,
    tool_manager: pl_core::ToolManager,
    interactions: InteractionService,
    product_events: ProductEventBus,
    /// agent framework 的 write-behind writer 句柄；framework 被 take 后关机仍能排空。
    persistence: std::sync::Arc<
        tokio::sync::Mutex<Option<crate::studio::agent_host::StudioAgentRepository>>,
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

    /// 返回本次启动构造阶段产生的配置恢复报告。
    pub fn startup_config_recovery(&self) -> Option<crate::config::ConfigRecoveryReport> {
        self.config_runtime.startup_recovery()
    }

    /// 立即重试待落库事实；查询和停止路径不需要调用本命令。
    pub async fn retry_persistence(&self) -> Result<crate::PersistenceStateSnapshot> {
        let persistence = self.agent_facility.persistence.lock().await.clone();
        let Some(persistence) = persistence else {
            return Ok(self.agent_facility.product_events.persistence_state());
        };
        persistence.writer().retry_now();
        Ok(persistence.writer().state_snapshot())
    }

    /// Returns whether an active turn prevents a safe application update.
    pub async fn is_busy_for_update(&self) -> Result<bool> {
        Ok(!self.derive_active_turns().await?.is_empty())
    }

    /// 从 agent framework 派生当前所有活动 turn。
    ///
    /// 活动 turn 列表不再手工维护：canonical source 是每个 agent 的
    /// `AgentSnapshot.active_turn_id`。这里聚合整棵 agent tree 的活动 turn，
    /// 用于 idle 判断。UI 不消费此列表（它从 per-thread 流读取 busy 状态）。
    async fn derive_active_turns(&self) -> Result<Vec<StudioActiveTurn>> {
        let Some(framework) = self.agent_facility.framework.lock().await.clone() else {
            return Ok(Vec::new());
        };
        let runtime = framework.handle();
        let snapshots = runtime
            .list()
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let mut turns = Vec::new();
        for snapshot in snapshots {
            let Some(turn_id) = snapshot.active_turn_id().cloned() else {
                continue;
            };
            turns.push(StudioActiveTurn {
                thread_id: snapshot.identity.id.to_string(),
                turn_id: turn_id.to_string(),
            });
        }
        Ok(turns)
    }

    async fn close_project_agent_trees(&self, thread_ids: &[String]) -> Result<()> {
        // `.boxed()`：把 agent 关闭链的大 future 状态机放堆上，减小 studio
        // runtime 侧 async 帧，避免与 agent loop 帧叠加触发线程栈耗尽。
        let Some(framework) = self.agent_facility.framework.lock().await.clone() else {
            return Ok(());
        };
        let runtime = framework.handle();
        let root_agent_ids = thread_ids
            .iter()
            .map(|thread_id| root_agent_id(thread_id))
            .collect::<BTreeSet<_>>();
        for root_agent_id in &root_agent_ids {
            retire_agent_if_present(&runtime, root_agent_id.clone())
                .boxed()
                .await?;
        }

        let snapshots = runtime
            .list()
            .boxed()
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let parents = snapshots
            .iter()
            .map(|snapshot| {
                (
                    snapshot.identity.id.clone(),
                    snapshot.identity.parent_id.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut descendants = snapshots
            .into_iter()
            .filter(|snapshot| {
                !matches!(
                    snapshot.state,
                    pl_core::AgentState::Closing(_) | pl_core::AgentState::Closed(_)
                ) && has_project_root(&parents, &snapshot.identity.id, &root_agent_ids)
            })
            .collect::<Vec<_>>();
        descendants.sort_by_key(|snapshot| snapshot.identity.depth);
        for descendant in descendants {
            retire_agent_if_present(&runtime, descendant.identity.id)
                .boxed()
                .await?;
        }
        Ok(())
    }
}

async fn retire_agent_if_present(
    runtime: &pl_core::AgentRuntimeHandle,
    agent_id: pl_core::ThreadId,
) -> Result<()> {
    match runtime.retire(agent_id).boxed().await {
        Ok(_) | Err(pl_core::AgentRuntimeError::NotFound(_)) => Ok(()),
        Err(error) => Err(anyhow::anyhow!(error)),
    }
}

fn has_project_root(
    parents: &BTreeMap<pl_core::ThreadId, Option<pl_core::ThreadId>>,
    agent_id: &pl_core::ThreadId,
    roots: &BTreeSet<pl_core::ThreadId>,
) -> bool {
    let mut current = Some(agent_id.clone());
    let mut remaining = parents.len().saturating_add(1);
    while let Some(agent_id) = current {
        if roots.contains(&agent_id) {
            return true;
        }
        if remaining == 0 {
            return false;
        }
        remaining -= 1;
        current = parents.get(&agent_id).cloned().flatten();
    }
    false
}

// Legacy Task orchestration tests were removed with the fixed Task runtime.
