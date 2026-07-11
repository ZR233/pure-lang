use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, bail};

use super::git::{RepositorySnapshot, inspect_repository};
use super::{CreateTaskRun, TaskRunPhase, TaskRunRecord};
use crate::studio::ids::new_id;
use crate::studio::store::StudioStore;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BranchKey {
    common_dir: String,
    branch: String,
}

impl BranchKey {
    fn new(common_dir: &Path, branch: &str) -> Self {
        let common_dir = common_dir.to_string_lossy().replace('\\', "/");
        Self {
            common_dir: if cfg!(windows) {
                common_dir.to_lowercase()
            } else {
                common_dir
            },
            branch: branch.to_string(),
        }
    }
}

/// 持久化 Task 模式事实并守护用户当前分支。
pub(crate) struct TaskCoordinator {
    pub(super) store: StudioStore,
    owned_process_leases: Mutex<HashMap<BranchKey, String>>,
    pub(super) allocation_lock: tokio::sync::Mutex<()>,
}

impl TaskCoordinator {
    pub(crate) fn new(store: StudioStore) -> Self {
        Self {
            store,
            owned_process_leases: Mutex::new(HashMap::new()),
            allocation_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) async fn start_confirmed_task(
        &self,
        session_id: &str,
        plan: &str,
        repository: impl AsRef<Path>,
    ) -> Result<TaskRunRecord> {
        if plan.trim().is_empty() {
            bail!("task plan must not be empty");
        }
        let snapshot = inspect_repository(repository, true).await?;
        let key = BranchKey::new(&snapshot.git_common_dir, &snapshot.branch);
        let owner_token = new_id("task-owner");
        acquire_process_lease(&key, &owner_token)?;

        let result = self
            .store
            .create_task_run_with_lease(CreateTaskRun {
                session_id: session_id.to_string(),
                phase: TaskRunPhase::DesignUpdating,
                plan: plan.trim().to_string(),
                workspace_root: snapshot.workspace_root.to_string_lossy().to_string(),
                git_common_dir: snapshot.git_common_dir.to_string_lossy().to_string(),
                branch: snapshot.branch,
                head_commit: snapshot.head,
            })
            .await;
        let (run, _) = match result {
            Ok(result) => result,
            Err(error) => {
                release_process_lease(&key, &owner_token);
                return Err(error);
            }
        };
        replace_process_lease_owner(&key, &owner_token, &run.id)?;
        self.owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, run.id.clone());
        Ok(run)
    }

    pub(crate) async fn recover_active_tasks(&self) -> Result<Vec<TaskRunRecord>> {
        let mut recovered = Vec::new();
        for run in self.store.list_active_task_runs().await? {
            let snapshot = match inspect_repository(&run.workspace_root, true).await {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    self.block_run(&run, format!("repository recovery failed: {error}"))
                        .await?;
                    continue;
                }
            };
            if let Err(reason) = validate_snapshot(&run, &snapshot) {
                self.block_run(&run, reason.to_string()).await?;
                continue;
            }
            let key = BranchKey::new(&snapshot.git_common_dir, &snapshot.branch);
            if let Err(error) = acquire_process_lease(&key, &run.id) {
                self.block_run(&run, error.to_string()).await?;
                continue;
            }
            self.owned_process_leases
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(key, run.id.clone());
            recovered.push(run);
        }
        Ok(recovered)
    }

    pub(crate) async fn verify_expected_head(&self, task_run_id: &str) -> Result<bool> {
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found")?;
        let snapshot = match inspect_repository(&run.workspace_root, true).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.block_run(&run, format!("repository verification failed: {error}"))
                    .await?;
                return Ok(false);
            }
        };
        if let Err(reason) = validate_snapshot(&run, &snapshot) {
            self.block_run(&run, reason.to_string()).await?;
            return Ok(false);
        }
        Ok(true)
    }

    pub(crate) async fn finish_task(
        &self,
        task_run_id: &str,
        phase: TaskRunPhase,
        status_message: Option<String>,
    ) -> Result<TaskRunRecord> {
        if !matches!(
            phase,
            TaskRunPhase::Completed | TaskRunPhase::Failed | TaskRunPhase::Cancelled
        ) {
            bail!("finish_task requires a terminal phase");
        }
        let run = self
            .store
            .transition_task_run(task_run_id, phase, status_message)
            .await?;
        self.store.release_branch_lease(task_run_id).await?;
        self.release_owned_process_lease(task_run_id);
        Ok(run)
    }

    pub(crate) fn suspend(&self) {
        let mut owned = self
            .owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (key, owner) in owned.drain() {
            release_process_lease(&key, &owner);
        }
    }

    pub(crate) async fn block_continuation_failure(
        &self,
        task_run_id: &str,
        reason: String,
    ) -> Result<()> {
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found while blocking continuation failure")?;
        if !run.phase.is_terminal() {
            self.block_run(&run, reason).await?;
        }
        Ok(())
    }

    pub(super) async fn block_run(&self, run: &TaskRunRecord, reason: String) -> Result<()> {
        self.store
            .transition_task_run(&run.id, TaskRunPhase::Blocked, Some(reason))
            .await?;
        self.release_owned_process_lease(&run.id);
        release_process_lease(
            &BranchKey::new(Path::new(&run.git_common_dir), &run.branch),
            &run.id,
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn process_lease_is_held(&self, run: &TaskRunRecord) -> bool {
        process_leases()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&BranchKey::new(Path::new(&run.git_common_dir), &run.branch))
            .is_some_and(|owner| owner == &run.id)
    }

    fn release_owned_process_lease(&self, task_run_id: &str) {
        let mut owned = self
            .owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = owned
            .iter()
            .find_map(|(key, owner)| (owner == task_run_id).then(|| key.clone()));
        if let Some(key) = key {
            owned.remove(&key);
            release_process_lease(&key, task_run_id);
        }
    }
}

impl Drop for TaskCoordinator {
    fn drop(&mut self) {
        self.suspend();
    }
}

fn validate_snapshot(run: &TaskRunRecord, snapshot: &RepositorySnapshot) -> Result<()> {
    let expected_key = BranchKey::new(Path::new(&run.git_common_dir), &run.branch);
    let actual_key = BranchKey::new(&snapshot.git_common_dir, &snapshot.branch);
    if expected_key != actual_key {
        bail!("task branch changed outside the coordinator");
    }
    if snapshot.head != run.expected_head {
        bail!(
            "task HEAD drifted: expected {}, actual {}",
            run.expected_head,
            snapshot.head
        );
    }
    Ok(())
}

fn process_leases() -> &'static Mutex<HashMap<BranchKey, String>> {
    static LEASES: OnceLock<Mutex<HashMap<BranchKey, String>>> = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn acquire_process_lease(key: &BranchKey, owner: &str) -> Result<()> {
    let mut leases = process_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = leases.get(key) {
        bail!("branch is already owned by task {existing}");
    }
    leases.insert(key.clone(), owner.to_string());
    Ok(())
}

fn replace_process_lease_owner(key: &BranchKey, current: &str, next: &str) -> Result<()> {
    let mut leases = process_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let owner = leases
        .get_mut(key)
        .context("process branch lease disappeared")?;
    if owner != current {
        bail!("process branch lease owner changed unexpectedly");
    }
    *owner = next.to_string();
    Ok(())
}

fn release_process_lease(key: &BranchKey, owner: &str) {
    let mut leases = process_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if leases.get(key).is_some_and(|current| current == owner) {
        leases.remove(key);
    }
}
