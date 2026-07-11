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
    generation: TaskAgentGeneration,
    supervisor: AgentSupervisor,
}

#[derive(PartialEq, Eq)]
enum TaskAgentGeneration {
    Planning,
    TaskRun(String),
}

impl TaskAgentRuntimeRegistry {
    pub(super) fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    pub(super) async fn supervisor_for_task(
        &self,
        session_id: &str,
        repository: &Path,
        lifecycle_epoch: u64,
    ) -> Result<AgentSupervisor> {
        self.supervisor_for_task_generation(session_id, repository, lifecycle_epoch, None)
            .await
    }

    pub(super) async fn supervisor_for_task_generation(
        &self,
        session_id: &str,
        repository: &Path,
        lifecycle_epoch: u64,
        task_run_id: Option<&str>,
    ) -> Result<AgentSupervisor> {
        let repository_identity = repository_identity(repository);
        let requested_generation = task_run_id
            .map(|task_run_id| TaskAgentGeneration::TaskRun(task_run_id.to_string()))
            .unwrap_or(TaskAgentGeneration::Planning);
        let mut entries = self.entries.lock().await;
        if let Some(runtime) = entries.get_mut(session_id) {
            if runtime.repository_identity != repository_identity {
                bail!("task agent runtime repository changed for session {session_id}");
            }
            if runtime.lifecycle_epoch != lifecycle_epoch {
                bail!("task agent runtime epoch changed for session {session_id}");
            }
            match (&runtime.generation, &requested_generation) {
                (TaskAgentGeneration::Planning, TaskAgentGeneration::TaskRun(_)) => {
                    runtime.generation = requested_generation;
                    return Ok(runtime.supervisor.clone());
                }
                (current, requested) if current == requested => {
                    return Ok(runtime.supervisor.clone());
                }
                (TaskAgentGeneration::TaskRun(_), _)
                | (TaskAgentGeneration::Planning, TaskAgentGeneration::Planning) => {}
            }
        }
        let retired = entries.remove(session_id);
        drop(entries);
        if let Some(retired) = retired
            && let Err(error) = retired
                .supervisor
                .quiesce_preserving_worktrees("task generation changed")
                .await
        {
            self.entries
                .lock()
                .await
                .insert(session_id.to_string(), retired);
            return Err(error.into());
        }
        let supervisor = AgentSupervisor::default();
        self.entries.lock().await.insert(
            session_id.to_string(),
            TaskAgentRuntime {
                repository_identity,
                lifecycle_epoch,
                generation: requested_generation,
                supervisor: supervisor.clone(),
            },
        );
        Ok(supervisor)
    }

    #[cfg(test)]
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

    pub(super) async fn supervisor_for_mode_generation(
        &self,
        mode: CompileMode,
        session_id: &str,
        repository: &Path,
        lifecycle_epoch: u64,
        task_run_id: Option<&str>,
    ) -> Result<Option<AgentSupervisor>> {
        match mode {
            CompileMode::Simple => Ok(None),
            CompileMode::Task => self
                .supervisor_for_task_generation(
                    session_id,
                    repository,
                    lifecycle_epoch,
                    task_run_id,
                )
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
