use std::collections::{BTreeMap, BTreeSet};

use crate::InteractionRequest;
use anyhow::Result;

use crate::McpRuntimeHandle;
use crate::config::ConfigStore;
use crate::studio::agent_host::{StudioAgentResources, StudioAgentRuntime, root_agent_id};
use crate::studio::records::ThreadRecord;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{
    InteractionRuntime, StudioActiveTurn, StudioProductEventRuntime, StudioRecoveryCleanupPreview,
    StudioRecoveryIssueAction, StudioRuntimeSnapshot, StudioRuntimeState, StudioStore,
};

mod history;
mod interaction_continuation;
mod lifecycle;
mod mcp_health;
mod plan_confirmation;
mod prompt_runner;
mod task_recovery;
mod thread_service;

/// Studio UI 提交 prompt 的请求。
///
/// runtime 只负责产品投影；Turn ID、FIFO、取消与 canonical Thread 全部由
/// `pl_core::AgentRuntime` 管理。
pub struct StudioSubmitPromptRequest {
    pub thread_id: String,
    pub prompt: String,
    pub attachment_ids: Vec<String>,
    pub options: StudioSubmitPromptOptions,
}

/// Studio UI 提交 prompt 的附加选项。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudioSubmitPromptOptions {
    pub presentation: pl_core::MailboxPresentation,
    pub lifecycle: Option<StudioPlanImplementationLifecycle>,
    pub turn_policy: pl_core::AgentTurnSubmitPolicy,
}

/// 计划实施 turn 的生命周期关联。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioPlanImplementationLifecycle {
    pub thread_id: String,
    pub plan_id: String,
}

/// Studio UI 提交 prompt 后得到的 framework turn 信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioSubmitPromptResponse {
    pub thread_id: String,
    pub turn_id: String,
    pub cursor: u64,
}

/// Studio UI 请求停止当前 Thread Turn 后的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioStopPromptResponse {
    pub thread_id: String,
    pub stopped: bool,
}

/// Studio UI resolve interaction 后的核心响应。
#[derive(Debug, Clone, PartialEq)]
pub struct StudioResolveInteractionResponse {
    pub thread_id: String,
    pub interaction: InteractionRequest,
    pub threads: Vec<ThreadRecord>,
}

#[derive(Clone)]
pub struct StudioRuntime {
    store: StudioStore,
    config_store: ConfigStore,
    external_runtimes: StudioExternalRuntimes,
    agent_facility: StudioAgentFacility,
    runtime_state: StudioRuntimeState,
    recovery: crate::studio::StudioRecoveryRegistry,
    task_coordinator: std::sync::Arc<TaskCoordinator>,
    lifecycle_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    #[cfg(test)]
    initialization_entry_barrier: Option<std::sync::Arc<tokio::sync::Barrier>>,
}

#[derive(Clone)]
struct StudioExternalRuntimes {
    mcp: McpRuntimeHandle,
    mcp_health_watcher: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    lsp: pl_lsp::LspRuntimeRegistry,
}

#[derive(Clone)]
struct StudioAgentFacility {
    framework: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<StudioAgentRuntime>>>>,
    resources: StudioAgentResources,
    interactions: InteractionRuntime,
    product_events: StudioProductEventRuntime,
}

impl StudioRuntime {
    /// Returns whether a turn or durable task prevents a safe application update.
    pub async fn is_busy_for_update(&self) -> Result<bool> {
        if !self.derive_active_turns().await?.is_empty() {
            return Ok(true);
        }
        Ok(!self.store.list_active_task_runs().await?.is_empty())
    }

    /// 从 agent framework 派生当前所有活动 turn。
    ///
    /// 活动 turn 列表不再手工维护：canonical source 是每个 agent 的
    /// `AgentSnapshot.active_turn_id`。这里聚合所有 root thread 的活动 turn，
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
            if snapshot.identity.parent_id.is_some() {
                continue;
            }
            let Some(turn_id) = snapshot.active_turn_id.clone() else {
                continue;
            };
            turns.push(StudioActiveTurn {
                thread_id: snapshot.identity.id.to_string(),
                turn_id: turn_id.to_string(),
            });
        }
        Ok(turns)
    }

    /// 测试用：返回派生的活动 turn 列表。
    #[cfg(test)]
    pub(in crate::studio) async fn active_turns_for_test(&self) -> Vec<StudioActiveTurn> {
        self.derive_active_turns().await.unwrap_or_default()
    }

    pub async fn thread_task_view(
        &self,
        thread_id: &str,
    ) -> Result<Option<crate::StudioTaskRuntime>> {
        super::task_projection::load_task_runtime(&self.store, thread_id).await
    }

    pub async fn preview_recovery_issue_cleanup(
        &self,
        issue_id: &str,
    ) -> Result<StudioRecoveryCleanupPreview> {
        let issue = self
            .recovery
            .get(issue_id)
            .ok_or_else(|| anyhow::anyhow!("recovery issue is no longer active"))?;
        self.task_coordinator.preview_recovery_cleanup(&issue).await
    }

    pub async fn preview_project_cleanup(
        &self,
        project_id: &str,
    ) -> Result<StudioRecoveryCleanupPreview> {
        self.task_coordinator
            .preview_project_cleanup(project_id)
            .await
    }

    pub async fn cleanup_project(
        &self,
        project_id: &str,
        expected_revision: &str,
    ) -> Result<StudioRuntimeSnapshot> {
        let issue = self
            .task_coordinator
            .project_cleanup_issue(project_id)
            .await?;
        self.cleanup_project_issue(issue, expected_revision).await
    }

    pub async fn cleanup_recovery_issue(
        &self,
        issue_id: &str,
        expected_revision: &str,
    ) -> Result<StudioRuntimeSnapshot> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let issue = self
            .recovery
            .get(issue_id)
            .ok_or_else(|| anyhow::anyhow!("recovery issue is no longer active"))?;
        if issue.action == StudioRecoveryIssueAction::RemoveProject {
            return self.cleanup_project_issue(issue, expected_revision).await;
        }
        if let Some(thread_id) = issue.thread_id.as_deref()
            && self.thread_is_busy(thread_id).await?
        {
            anyhow::bail!("recovery cleanup requires an idle Thread");
        }
        let authorization = self
            .task_coordinator
            .validate_recovery_cleanup(&issue, expected_revision)
            .await?;
        self.task_coordinator
            .execute_recovery_cleanup(&issue, &authorization)
            .await?;
        if let Some(thread_id) = issue.thread_id.as_deref() {
            self.close_project_agent_trees(&[thread_id.to_string()])
                .await?;
            let emitter = self.interaction_emitter(thread_id.to_string());
            self.agent_facility
                .interactions
                .cancel_thread(thread_id, "recovery context reset", emitter)
                .await?;
            self.store.reset_agent_sessions_for_root(thread_id).await?;
        }
        let _ = self.recovery.remove(issue_id);
        self.runtime_snapshot().await
    }

    pub async fn retry_recovery_issue(&self, issue_id: &str) -> Result<StudioRuntimeSnapshot> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let issue = self
            .recovery
            .get(issue_id)
            .ok_or_else(|| anyhow::anyhow!("recovery issue is no longer active"))?;
        if issue.action != StudioRecoveryIssueAction::Retry {
            anyhow::bail!("recovery issue does not authorize retry");
        }
        self.task_coordinator.retry_recovery_issue(&issue).await?;
        let _ = self.recovery.remove(issue_id);
        self.runtime_snapshot().await
    }

    async fn cleanup_project_issue(
        &self,
        issue: crate::StudioRecoveryIssue,
        expected_revision: &str,
    ) -> Result<StudioRuntimeSnapshot> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let project_id = issue
            .project_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("project cleanup has no project"))?;
        let preview = self
            .task_coordinator
            .validate_recovery_cleanup(&issue, expected_revision)
            .await?;
        let mut thread_ids = self.store.list_project_thread_ids(project_id).await?;
        thread_ids.sort();
        thread_ids.dedup();
        let root_thread_ids = thread_ids.iter().cloned().collect::<BTreeSet<_>>();
        self.agent_facility
            .resources
            .begin_cleanup_takeover(&root_thread_ids)
            .await;
        self.close_project_agent_trees(&thread_ids).await?;
        for thread_id in &thread_ids {
            let emitter = self.interaction_emitter(thread_id.clone());
            self.agent_facility
                .interactions
                .cancel_thread(thread_id, "project cleaned up", emitter)
                .await?;
        }
        self.task_coordinator
            .execute_recovery_cleanup(&issue, &preview)
            .await?;
        self.store.quarantine_project(project_id).await?;
        self.agent_facility
            .resources
            .complete_cleanup_takeover(&root_thread_ids)
            .await;
        let _ = self.recovery.remove_for_project(project_id);
        self.runtime_snapshot().await
    }

    async fn close_project_agent_trees(&self, thread_ids: &[String]) -> Result<()> {
        let runtime = self.agent_framework().await?.handle();
        let root_agent_ids = thread_ids
            .iter()
            .map(|thread_id| root_agent_id(thread_id))
            .collect::<BTreeSet<_>>();
        for root_agent_id in &root_agent_ids {
            close_agent_if_present(&runtime, root_agent_id.clone()).await?;
        }

        let snapshots = runtime
            .list()
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
                    snapshot.lifecycle,
                    pl_core::AgentLifecycleState::Closing | pl_core::AgentLifecycleState::Closed
                ) && has_project_root(&parents, &snapshot.identity.id, &root_agent_ids)
            })
            .collect::<Vec<_>>();
        descendants.sort_by_key(|snapshot| snapshot.identity.depth);
        for descendant in descendants {
            close_agent_if_present(&runtime, descendant.identity.id).await?;
        }
        Ok(())
    }
}

async fn close_agent_if_present(
    runtime: &pl_core::AgentRuntimeHandle,
    agent_id: pl_core::AgentId,
) -> Result<()> {
    match runtime.close(agent_id).await {
        Ok(_) | Err(pl_core::AgentRuntimeError::NotFound(_)) => Ok(()),
        Err(error) => Err(anyhow::anyhow!(error)),
    }
}

fn has_project_root(
    parents: &BTreeMap<pl_core::AgentId, Option<pl_core::AgentId>>,
    agent_id: &pl_core::AgentId,
    roots: &BTreeSet<pl_core::AgentId>,
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

#[cfg(test)]
mod tests;
