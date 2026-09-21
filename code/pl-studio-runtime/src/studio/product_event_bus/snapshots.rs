//! 低频产品状态快照：agent、settings、recovery、MCP、LSP、skills、thread mode
//! catalog、provider usage、model performance、updater 与 persistence 状态的
//! 读取、维护与事件发射。

use tokio::sync::watch;

use crate::{
    PersistenceStateSnapshot, ProviderUsageStateSnapshot, SkillsStateSnapshot,
    StudioAgentDirectoryData, StudioAgentDirectoryEntry, StudioAgentDirectoryState,
    StudioLspStateSnapshot, StudioMcpStateSnapshot, StudioModelPerformanceSnapshot,
    StudioProductEventEnvelope, StudioProductEventKind, StudioRecoveryStateSnapshot,
    StudioSettingsStateSnapshot, StudioUpdateStateSnapshot,
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

    pub fn emit_thread_mode_catalog(
        &self,
        state: pl_protocol::ThreadModeCatalogSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::ThreadModeCatalogChanged(state))
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
    ) {
        let bus = self.clone();
        let mut threads = bus.store.thread_persistence().subscribe();
        bus.update_persistence(state.borrow().clone(), threads.borrow().clone());
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
            }
        });
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
