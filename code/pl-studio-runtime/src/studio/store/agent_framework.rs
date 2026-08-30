use anyhow::{Context, Result};
use pl_core::{AgentCommand, AgentState, AgentStateTransition};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, IntoActiveModel};

use crate::studio::StudioStore;
use crate::studio::entity::thread;
use crate::studio::ids::unix_seconds;

pub(in crate::studio) struct ThreadRuntimeSeed {
    pub thread_revision: u64,
    pub runtime_revision: u64,
    pub event_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::studio) enum UnregisteredThreadFault {
    Faulted,
    RuntimeOwned,
}

impl StudioStore {
    /// 在 actor 首次注册前把已创建的 child Thread 收敛为 canonical Faulted 状态。
    /// 已由 runtime 持有 revision 的 Thread 留给 core spawn compensation 处理。
    pub(in crate::studio) async fn fault_unregistered_child_thread(
        &self,
        thread_id: &str,
        error: &str,
    ) -> Result<UnregisteredThreadFault> {
        let row = thread::Entity::find_by_id(thread_id)
            .one(&self.db)
            .await?
            .with_context(|| format!("spawn compensation Thread not found: {thread_id}"))?;
        if row.runtime_revision.is_some() {
            return Ok(UnregisteredThreadFault::RuntimeOwned);
        }
        let state: AgentState = serde_json::from_str(&row.state_json)?;
        let state = state
            .decide(AgentCommand::Fault {
                error: pl_protocol::StateError {
                    code: "agentRegistrationFailed".to_string(),
                    message: error.to_string(),
                    retryable: false,
                },
                turn_id: None,
                classification: pl_core::AgentFaultClassification::RecoverableRuntime,
            })?
            .next_state;
        let mut active = row.into_active_model();
        active.state_json = Set(serde_json::to_string(&state)?);
        active.updated_at = Set(unix_seconds());
        active.update(&self.db).await?;
        Ok(UnregisteredThreadFault::Faulted)
    }

    pub(in crate::studio) async fn thread_runtime_seed(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadRuntimeSeed>> {
        let Some(row) = thread::Entity::find_by_id(thread_id).one(&self.db).await? else {
            return Ok(None);
        };
        let event_sequence = u64::try_from(row.event_sequence)?;
        Ok(Some(ThreadRuntimeSeed {
            thread_revision: u64::try_from(row.revision)?,
            runtime_revision: event_sequence.saturating_add(1).max(1),
            event_sequence: event_sequence.saturating_add(1).max(1),
        }))
    }
}
