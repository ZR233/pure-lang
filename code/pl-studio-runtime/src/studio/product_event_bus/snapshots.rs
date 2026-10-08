//! 低频产品状态快照：agent、settings、recovery、MCP、LSP、skills、thread mode
//! catalog、provider usage、model performance、updater 与 persistence 状态的
//! 读取、维护与事件发射。

use std::sync::atomic::Ordering;

use tokio::sync::watch;

use crate::{
    PersistenceStateSnapshot, ProviderUsageStateSnapshot, SkillsStateSnapshot,
    StudioAgentDirectoryData, StudioAgentDirectoryEntry, StudioAgentDirectoryState,
    StudioAgentProfilesStateSnapshot, StudioLspStateSnapshot, StudioMcpStateSnapshot,
    StudioModelPerformanceSnapshot, StudioPersistenceQueueStateSnapshot,
    StudioProductEventEnvelope, StudioProductEventKind, StudioRecoveryStateSnapshot,
    StudioSessionCostsState, StudioSettingsStateSnapshot, StudioUpdateStateSnapshot,
};

use super::ProductEventBus;

impl ProductEventBus {
    pub async fn read_agent_directory(&self) -> StudioAgentDirectoryState {
        StudioAgentDirectoryState {
            state: self.resource(
                &self.revisions.agent,
                StudioAgentDirectoryData {
                    agents: self.agents.lock().await.values().cloned().collect(),
                },
            ),
        }
    }

    pub fn emit_agent_directory(
        &self,
        state: StudioAgentDirectoryState,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::AgentDirectoryChanged(state))
    }

    pub async fn update_agent_directory(
        &self,
        agent: StudioAgentDirectoryEntry,
    ) -> StudioProductEventEnvelope {
        self.agents.lock().await.insert(agent.id.clone(), agent);
        self.bump(&self.revisions.agent);
        self.emit_agent_directory(self.read_agent_directory().await)
    }

    pub fn emit_settings_state(
        &self,
        state: StudioSettingsStateSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::SettingsStateChanged(Box::new(
            state,
        )))
    }

    pub fn emit_recovery_state(
        &self,
        state: pl_protocol::ObservedResource<Vec<crate::StudioRecoveryIssue>>,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::RecoveryStateChanged(
            StudioRecoveryStateSnapshot { state },
        ))
    }

    pub fn emit_mcp_state(&self, state: StudioMcpStateSnapshot) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::McpStateChanged(state))
    }

    pub fn emit_lsp_state(&self, state: StudioLspStateSnapshot) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::LspStateChanged(state))
    }

    pub fn emit_skills_state(&self, state: SkillsStateSnapshot) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::SkillsStateChanged(state.into()))
    }

    /// Mode catalog 只在真实注册/装载变化时发布；同一 revision 重复装载不产生事件。
    pub fn emit_thread_mode_catalog(
        &self,
        state: pl_protocol::ThreadModeCatalogSnapshot,
    ) -> Option<StudioProductEventEnvelope> {
        let published = self.mode_catalog_revision.load(Ordering::Acquire);
        if state.revision == 0 || state.revision == published {
            return None;
        }
        self.mode_catalog_revision
            .store(state.revision, Ordering::Release);
        Some(self.emit(StudioProductEventKind::ThreadModeCatalogChanged(state)))
    }

    pub fn emit_provider_usage_state(
        &self,
        state: ProviderUsageStateSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::ProviderUsageStateChanged(state))
    }

    pub fn emit_model_performance_state(
        &self,
        state: StudioModelPerformanceSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::ModelPerformanceStateChanged(state))
    }

    /// 发布一个 root 会话的作用域费用事实；清除（`cost == None`）也是显式事件。
    pub fn emit_session_costs(&self, state: StudioSessionCostsState) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::SessionCostsChanged(state))
    }

    /// 发布配置级 Agent Profiles 资源快照。
    pub fn emit_agent_profiles_state(
        &self,
        state: StudioAgentProfilesStateSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::AgentProfilesStateChanged(Box::new(
            state,
        )))
    }

    pub fn emit_updater_state(
        &self,
        state: StudioUpdateStateSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::UpdaterStateChanged(state))
    }

    pub fn persistence_state(&self) -> PersistenceStateSnapshot {
        self.persistence_snapshot
            .lock()
            .expect("persistence snapshot lock poisoned")
            .clone()
    }

    pub(in crate::studio) fn observe_persistence(
        &self,
        mut state: watch::Receiver<PersistenceStateSnapshot>,
    ) -> tokio::task::JoinHandle<()> {
        let bus = self.clone();
        let mut threads = bus.store.thread_persistence().subscribe();
        bus.update_persistence(state.borrow().clone(), threads.borrow().clone());
        bus.update_persistence_queue();
        // 任务由 runtime 持有并在 shutdown 收束；它自身持有 bus（含 writer），不能依赖
        // sender 关闭退出。
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = state.changed() => if result.is_err() { break; },
                    result = threads.changed() => if result.is_err() { break; },
                }
                bus.update_persistence(
                    state.borrow_and_update().clone(),
                    threads.borrow_and_update().clone(),
                );
                bus.update_persistence_queue();
            }
        })
    }

    /// 当前已发布的持久化队列快照；纯查询。
    ///
    /// 首帧由构造期 observation 显式建立；读取路径不 emit、不推进 revision。
    pub fn persistence_queue_state(&self) -> StudioPersistenceQueueStateSnapshot {
        self.persistence_queue
            .lock()
            .expect("persistence queue lock poisoned")
            .snapshot
            .clone()
    }

    /// 把协调器的真实队列观测发布为 typed 快照；值未变化时不提升 revision。
    ///
    /// 观测在协调器 watch 唤醒时刷新：队列操作数、字节、在途字节、逐 Thread 水位与
    /// calls writer 进度都来自协调器；calls 侧只在指标真变时回灌，避免自唤醒循环。
    fn update_persistence_queue(&self) {
        let coordinator = self.store.thread_persistence();
        coordinator.refresh_calls_metrics(self.store.calls().metrics());
        let queue = coordinator.queue_snapshot();
        let mut current = self
            .persistence_queue
            .lock()
            .expect("persistence queue lock poisoned");
        if current.published.as_ref() == Some(&queue) {
            return;
        }
        current.revision = current.revision.saturating_add(1);
        current.published = Some(queue.clone());
        current.snapshot = StudioPersistenceQueueStateSnapshot {
            revision: current.revision,
            updated_at: super::unix_seconds(),
            queue,
        };
        let snapshot = current.snapshot.clone();
        drop(current);
        self.emit(StudioProductEventKind::PersistenceQueueStateChanged(
            snapshot,
        ));
    }

    fn update_persistence(
        &self,
        mut state: PersistenceStateSnapshot,
        threads: crate::studio::storage::coordinator::ThreadPersistenceSnapshot,
    ) {
        use crate::studio::{BlockedPersistence, FlushingPersistence, PersistenceState};
        let pending = state
            .state
            .pending_commits()
            .saturating_add(threads.pending_commits);
        if let Some(error) = threads.error {
            state.state = PersistenceState::Blocked(BlockedPersistence {
                pending_commits: pending,
                oldest_pending_revision: threads.oldest_pending_revision,
                first_failed_at: super::unix_seconds(),
                error: pl_protocol::StateError {
                    code: "threadPersistenceFailed".into(),
                    message: error,
                    retryable: true,
                },
            });
        } else {
            match &mut state.state {
                PersistenceState::Ready(_) if pending > 0 => {
                    state.state = PersistenceState::Flushing(FlushingPersistence {
                        pending_commits: pending,
                        oldest_pending_revision: threads.oldest_pending_revision,
                    })
                }
                PersistenceState::Ready(value) => value.pending_commits = pending,
                PersistenceState::Flushing(value) => value.pending_commits = pending,
                PersistenceState::Degraded(value) => value.pending_commits = pending,
                PersistenceState::Recovering(value) => value.pending_commits = pending,
                PersistenceState::Blocked(value) => value.pending_commits = pending,
            }
        }
        let mut current = self
            .persistence_snapshot
            .lock()
            .expect("persistence snapshot lock poisoned");
        if state.state == current.state {
            return;
        }
        state.revision = current.revision.saturating_add(1);
        *current = state.clone();
        drop(current);
        self.emit(StudioProductEventKind::PersistenceStateChanged(state));
    }
}
