use std::collections::HashMap;
#[cfg(test)]
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};
use tokio::sync::{Mutex, RwLock};

use crate::AgentSupervisor;
use crate::CompileMode;

#[derive(Clone)]
pub(super) struct TaskAgentRuntimeRegistry {
    entries: Arc<Mutex<HashMap<String, Arc<Mutex<TaskAgentRuntime>>>>>,
    lifecycle: Arc<RwLock<()>>,
    #[cfg(test)]
    test_control: Arc<TaskAgentRuntimeTestControl>,
}

struct TaskAgentRuntime {
    repository_identity: String,
    lifecycle_epoch: u64,
    generation: TaskAgentGeneration,
    supervisor: AgentSupervisor,
}

#[derive(Clone, PartialEq, Eq)]
enum TaskAgentGeneration {
    Planning,
    TaskRun(String),
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct TaskAgentRuntimeTestBarrier {
    entered: Arc<tokio::sync::Barrier>,
    release: Arc<tokio::sync::Barrier>,
    used: Arc<AtomicBool>,
}

#[cfg(test)]
impl TaskAgentRuntimeTestBarrier {
    pub(super) fn new() -> Self {
        Self {
            entered: Arc::new(tokio::sync::Barrier::new(2)),
            release: Arc::new(tokio::sync::Barrier::new(2)),
            used: Arc::new(AtomicBool::new(false)),
        }
    }

    async fn pause_once(&self) {
        if !self.used.swap(true, Ordering::SeqCst) {
            self.entered.wait().await;
            self.release.wait().await;
        }
    }

    pub(super) async fn wait_until_entered(&self) {
        self.entered.wait().await;
    }

    pub(super) async fn release(&self) {
        self.release.wait().await;
    }
}

#[cfg(test)]
#[derive(Default)]
struct TaskAgentRuntimeTestControl {
    request_barriers: Mutex<HashMap<String, TaskAgentRuntimeTestBarrier>>,
    generation_barriers: Mutex<HashMap<String, TaskAgentRuntimeTestBarrier>>,
    generation_failures: Mutex<HashSet<String>>,
    shutdown_barrier: Mutex<Option<TaskAgentRuntimeTestBarrier>>,
}

impl TaskAgentRuntimeRegistry {
    pub(super) fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(())),
            #[cfg(test)]
            test_control: Arc::new(TaskAgentRuntimeTestControl::default()),
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
        let _lifecycle = self.lifecycle.read().await;
        let repository_identity = repository_identity(repository);
        let requested_generation = task_run_id
            .map(|task_run_id| TaskAgentGeneration::TaskRun(task_run_id.to_string()))
            .unwrap_or(TaskAgentGeneration::Planning);
        let runtime = {
            let mut entries = self.entries.lock().await;
            entries
                .entry(session_id.to_string())
                .or_insert_with(|| {
                    Arc::new(Mutex::new(TaskAgentRuntime {
                        repository_identity: repository_identity.clone(),
                        lifecycle_epoch,
                        generation: requested_generation.clone(),
                        supervisor: AgentSupervisor::default(),
                    }))
                })
                .clone()
        };
        #[cfg(test)]
        self.pause_request_before_cell_lock(session_id).await;
        let mut runtime = runtime.lock().await;
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

        #[cfg(test)]
        self.pause_generation_rotation(session_id).await?;
        runtime
            .supervisor
            .quiesce_preserving_worktrees("task generation changed")
            .await?;
        let supervisor = AgentSupervisor::default();
        *runtime = TaskAgentRuntime {
            repository_identity,
            lifecycle_epoch,
            generation: requested_generation,
            supervisor: supervisor.clone(),
        };
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
        let _lifecycle = self.lifecycle.write().await;
        #[cfg(test)]
        self.pause_shutdown().await;
        let runtimes = self
            .entries
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut failures = Vec::new();
        for runtime in runtimes {
            let runtime = runtime.lock().await;
            if let Err(error) = runtime
                .supervisor
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

    #[cfg(test)]
    pub(super) async fn pause_next_generation_rotation(
        &self,
        session_id: &str,
        barrier: TaskAgentRuntimeTestBarrier,
    ) {
        self.test_control
            .generation_barriers
            .lock()
            .await
            .insert(session_id.to_string(), barrier);
    }

    #[cfg(test)]
    pub(super) async fn pause_next_request_before_cell_lock(
        &self,
        session_id: &str,
        barrier: TaskAgentRuntimeTestBarrier,
    ) {
        self.test_control
            .request_barriers
            .lock()
            .await
            .insert(session_id.to_string(), barrier);
    }

    #[cfg(test)]
    pub(super) async fn fail_next_generation_rotation(&self, session_id: &str) {
        self.test_control
            .generation_failures
            .lock()
            .await
            .insert(session_id.to_string());
    }

    #[cfg(test)]
    pub(super) async fn pause_next_shutdown(&self, barrier: TaskAgentRuntimeTestBarrier) {
        *self.test_control.shutdown_barrier.lock().await = Some(barrier);
    }

    #[cfg(test)]
    async fn pause_generation_rotation(&self, session_id: &str) -> Result<()> {
        let barrier = self
            .test_control
            .generation_barriers
            .lock()
            .await
            .remove(session_id);
        if let Some(barrier) = barrier {
            barrier.pause_once().await;
        }
        if self
            .test_control
            .generation_failures
            .lock()
            .await
            .remove(session_id)
        {
            bail!("injected generation rotation failure for session {session_id}");
        }
        Ok(())
    }

    #[cfg(test)]
    async fn pause_request_before_cell_lock(&self, session_id: &str) {
        let barrier = self
            .test_control
            .request_barriers
            .lock()
            .await
            .remove(session_id);
        if let Some(barrier) = barrier {
            barrier.pause_once().await;
        }
    }

    #[cfg(test)]
    async fn pause_shutdown(&self) {
        let barrier = self.test_control.shutdown_barrier.lock().await.take();
        if let Some(barrier) = barrier {
            barrier.pause_once().await;
        }
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
