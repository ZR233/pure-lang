use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, bail};
use tokio::sync::Mutex;

use crate::AgentSupervisor;
use crate::CompileMode;

#[derive(Clone)]
pub(super) struct TaskAgentRuntimeRegistry {
    entries: Arc<Mutex<HashMap<String, TaskAgentRuntime>>>,
}

struct TaskAgentRuntime {
    repository_identity: String,
    lifecycle_epoch: u64,
    supervisor: AgentSupervisor,
}

impl TaskAgentRuntimeRegistry {
    pub(super) fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(super) async fn supervisor_for_task(
        &self,
        session_id: &str,
        repository: &Path,
        lifecycle_epoch: u64,
    ) -> Result<AgentSupervisor> {
        let repository_identity = repository_identity(repository);
        let mut entries = self.entries.lock().await;
        if let Some(runtime) = entries.get(session_id) {
            if runtime.repository_identity != repository_identity {
                bail!("task agent runtime repository changed for session {session_id}");
            }
            if runtime.lifecycle_epoch != lifecycle_epoch {
                bail!("task agent runtime epoch changed for session {session_id}");
            }
            return Ok(runtime.supervisor.clone());
        }
        let supervisor = AgentSupervisor::default();
        entries.insert(
            session_id.to_string(),
            TaskAgentRuntime {
                repository_identity,
                lifecycle_epoch,
                supervisor: supervisor.clone(),
            },
        );
        Ok(supervisor)
    }

    pub(super) async fn supervisor_for_mode(
        &self,
        mode: CompileMode,
        session_id: &str,
        repository: &Path,
        lifecycle_epoch: u64,
    ) -> Result<Option<AgentSupervisor>> {
        match mode {
            CompileMode::Simple => Ok(None),
            CompileMode::Task => self
                .supervisor_for_task(session_id, repository, lifecycle_epoch)
                .await
                .map(Some),
        }
    }

    pub(super) async fn quiesce_and_clear(&self) -> Result<()> {
        let supervisors = self
            .entries
            .lock()
            .await
            .values()
            .map(|runtime| runtime.supervisor.clone())
            .collect::<Vec<_>>();
        let mut failures = Vec::new();
        for supervisor in supervisors {
            if let Err(error) = supervisor
                .quiesce_preserving_worktrees("studio runtime shutdown")
                .await
            {
                failures.push(error.to_string());
            }
        }
        if !failures.is_empty() {
            bail!(
                "task agent runtime shutdown failed: {}",
                failures.join("; ")
            );
        }
        self.entries.lock().await.clear();
        Ok(())
    }

    #[cfg(test)]
    pub(super) async fn len(&self) -> usize {
        self.entries.lock().await.len()
    }
}

fn repository_identity(repository: &Path) -> String {
    let identity = repository.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        identity.to_lowercase()
    } else {
        identity
    }
}
