use std::path::{Path, PathBuf};

use crate::{PureError, Result as PureResult, WorktreeCreateSpec};
use anyhow::{Result, bail};

use super::super::{AllocateExecutor, TaskCoordinator};
use crate::studio::task_coordinator::owned_path::OwnedPath;

/// Studio Task 产品层为一次 child spawn 固定的业务输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioTaskSpawnRequest {
    pub(crate) agent_id: String,
    pub(crate) root_thread_id: String,
    pub(crate) task_name: String,
    pub(crate) role: String,
    pub(crate) owned_paths: Vec<String>,
    pub(crate) requested_by_call_id: String,
}

/// Studio Task 在 framework saga prepare 阶段预留的资源事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioTaskSpawnPreparation {
    worktree: Option<WorktreeCreateSpec>,
    lifecycle_token: Option<String>,
}

impl StudioTaskSpawnPreparation {
    fn without_worktree() -> Self {
        Self {
            worktree: None,
            lifecycle_token: None,
        }
    }

    fn with_token(token: impl Into<String>) -> Self {
        Self {
            worktree: None,
            lifecycle_token: Some(token.into()),
        }
    }

    fn with_worktree_and_token(worktree: WorktreeCreateSpec, token: impl Into<String>) -> Self {
        Self {
            worktree: Some(worktree),
            lifecycle_token: Some(token.into()),
        }
    }

    pub(crate) fn worktree_spec(&self) -> Option<&WorktreeCreateSpec> {
        self.worktree.as_ref()
    }

    pub(crate) fn lifecycle_token(&self) -> Option<&str> {
        self.lifecycle_token.as_deref()
    }
}

impl TaskCoordinator {
    pub(crate) async fn prepare_agent_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
    ) -> PureResult<StudioTaskSpawnPreparation> {
        match request.role.as_str() {
            "explorer" => self.prepare_explorer_spawn(request).await,
            "executor" => self.prepare_executor_spawn(request).await,
            "reviewer" => self.prepare_reviewer_spawn(request).await,
            role => Err(spawn_error(format!("task {role} creation is harness-only"))),
        }
    }

    async fn prepare_explorer_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
    ) -> PureResult<StudioTaskSpawnPreparation> {
        if !request.owned_paths.is_empty() {
            return Err(spawn_error("explorer must not declare ownedPaths"));
        }
        let Some(_run) = self
            .store
            .list_active_task_runs()
            .await
            .map_err(store_spawn_error)?
            .into_iter()
            .find(|run| run.root_thread_id == request.root_thread_id)
        else {
            return Ok(StudioTaskSpawnPreparation::without_worktree());
        };
        Ok(StudioTaskSpawnPreparation::without_worktree())
    }

    async fn prepare_executor_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
    ) -> PureResult<StudioTaskSpawnPreparation> {
        let owned_paths = normalize_owned_paths(&request.owned_paths)
            .map_err(|error| spawn_error(error.to_string()))?;
        let _mutation_guard = self.lock_branch_mutation().await;
        let _allocation_guard = self.allocation_lock.lock().await;
        let run = self
            .store
            .read_active_task_run_for_root_thread(&request.root_thread_id)
            .await
            .map_err(store_spawn_error)?;
        reject_bare_existing_directories(Path::new(&run.workspace_root), &owned_paths)
            .map_err(|error| spawn_error(error.to_string()))?;
        self.ensure_executor_design_contract(&run)
            .map_err(store_spawn_error)?;
        let allocation = self
            .store
            .allocate_executor(AllocateExecutor {
                thread_id: request.root_thread_id.clone(),
                title: request.task_name.clone(),
                owned_paths,
                agent_id: request.agent_id.clone(),
                requested_by_call_id: request.requested_by_call_id.clone(),
            })
            .await
            .map_err(store_spawn_error)?;
        debug_assert_eq!(
            allocation.work_unit.executor_thread_id.as_deref(),
            Some(request.agent_id.as_str())
        );
        let work_unit_id = allocation.work_unit.id.clone();
        Ok(StudioTaskSpawnPreparation::with_worktree_and_token(
            WorktreeCreateSpec {
                repo_root: PathBuf::from(&allocation.run.workspace_root),
                path: PathBuf::from(&allocation.work_unit.worktree_path),
                branch: allocation.work_unit.branch,
                base_commit: allocation.work_unit.base_commit,
            },
            work_unit_id,
        ))
    }

    async fn prepare_reviewer_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
    ) -> PureResult<StudioTaskSpawnPreparation> {
        if !request.owned_paths.is_empty() {
            return Err(spawn_error("reviewer must not declare ownedPaths"));
        }
        let round = self
            .store
            .authorize_reviewer_spawn(
                &request.root_thread_id,
                &request.requested_by_call_id,
                &request.agent_id,
            )
            .await
            .map_err(store_spawn_error)?;
        Ok(StudioTaskSpawnPreparation::with_token(round.id))
    }

    pub(crate) async fn activate_agent_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
        preparation: &StudioTaskSpawnPreparation,
    ) -> PureResult<()> {
        match request.role.as_str() {
            "executor" => {
                let token = preparation.lifecycle_token().ok_or_else(|| {
                    spawn_error("executor spawn preparation has no allocation token")
                })?;
                self.store
                    .activate_executor(token, &request.agent_id)
                    .await
                    .map_err(store_spawn_error)?;
            }
            "explorer" => {}
            "reviewer" => {
                if let Some(token) = preparation.lifecycle_token() {
                    self.store
                        .activate_reviewer(token, &request.agent_id)
                        .await
                        .map_err(store_spawn_error)?;
                }
            }
            role => {
                return Err(spawn_error(format!(
                    "task {role} activation is not supported"
                )));
            }
        }
        Ok(())
    }

    pub(crate) async fn rollback_agent_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
        preparation: &StudioTaskSpawnPreparation,
        error: &str,
    ) -> PureResult<()> {
        match request.role.as_str() {
            "executor" => {
                let token = preparation.lifecycle_token().ok_or_else(|| {
                    spawn_error("executor spawn preparation has no allocation token")
                })?;
                self.store
                    .fail_executor(token, &request.agent_id, error)
                    .await
                    .map_err(store_spawn_error)?;
            }
            "explorer" => {}
            "reviewer" => {
                self.store
                    .fail_reviewer_spawn(
                        &request.root_thread_id,
                        Some(&request.agent_id),
                        &request.requested_by_call_id,
                        error,
                    )
                    .await
                    .map_err(store_spawn_error)?;
            }
            role => {
                return Err(spawn_error(format!(
                    "task {role} rollback is not supported"
                )));
            }
        }
        Ok(())
    }

    pub(crate) async fn commit_agent_close(
        &self,
        request: &StudioTaskSpawnRequest,
        preparation: &StudioTaskSpawnPreparation,
    ) -> PureResult<super::super::ExecutorCloseDisposition> {
        if request.role != "executor" {
            return Ok(super::super::ExecutorCloseDisposition::Discard);
        }
        let token = preparation
            .lifecycle_token()
            .ok_or_else(|| spawn_error("task executor close requires an exact lifecycle token"))?;
        self.store
            .settle_executor_close(&request.root_thread_id, token, &request.agent_id)
            .await
            .map_err(store_spawn_error)
    }

    pub(crate) async fn prepare_agent_close(
        &self,
        request: &StudioTaskSpawnRequest,
        preparation: &StudioTaskSpawnPreparation,
    ) -> PureResult<super::super::ExecutorCloseDisposition> {
        if request.role != "executor" {
            return Ok(super::super::ExecutorCloseDisposition::Discard);
        }
        let token = preparation
            .lifecycle_token()
            .ok_or_else(|| spawn_error("task executor close requires an exact lifecycle token"))?;
        self.store
            .preflight_executor_close(&request.root_thread_id, token, &request.agent_id)
            .await
            .map_err(store_spawn_error)
    }
}

pub(crate) fn owned_paths_overlap(left: &[String], right: &[String]) -> Result<bool> {
    let left = left
        .iter()
        .map(|path| OwnedPath::parse(path))
        .collect::<Result<Vec<_>>>()?;
    let right = right
        .iter()
        .map(|path| OwnedPath::parse(path))
        .collect::<Result<Vec<_>>>()?;
    Ok(left
        .iter()
        .any(|left| right.iter().any(|right| left.overlaps(right))))
}

pub(crate) fn normalize_owned_paths(paths: &[String]) -> Result<Vec<String>> {
    if paths.is_empty() {
        bail!("Task executor ownedPaths must not be empty");
    }
    let normalized = paths
        .iter()
        .map(|path| OwnedPath::parse(path))
        .collect::<Result<Vec<_>>>()?;
    for (index, path) in normalized.iter().enumerate() {
        if normalized[index + 1..]
            .iter()
            .any(|other| path.overlaps(other))
        {
            bail!("ownedPaths entries must not overlap");
        }
    }
    let mut canonical = normalized
        .into_iter()
        .map(OwnedPath::into_canonical)
        .collect::<Vec<_>>();
    canonical.sort();
    Ok(canonical)
}

fn reject_bare_existing_directories(workspace_root: &Path, owned_paths: &[String]) -> Result<()> {
    for owned_path in owned_paths {
        if !owned_path.ends_with("/**") && workspace_root.join(owned_path).is_dir() {
            bail!(
                "owned path `{owned_path}` names an existing directory; use `{owned_path}/**` to own its descendants"
            );
        }
    }
    Ok(())
}

fn store_spawn_error(error: impl std::fmt::Display) -> PureError {
    spawn_error(error.to_string())
}

fn spawn_error(error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "spawn_agent".to_string(),
        error: error.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_owned_paths;

    #[test]
    fn executor_owned_paths_are_validated_and_canonicalized_before_spawn() {
        assert_eq!(
            normalize_owned_paths(&["tests\\case.rs".into(), "src/**".into()]).unwrap(),
            vec!["src/**", "tests/case.rs"]
        );
        for paths in [
            Vec::<String>::new(),
            vec!["../src".into()],
            vec!["src/*".into()],
            vec!["src/**".into(), "src/lib.rs".into()],
        ] {
            assert!(normalize_owned_paths(&paths).is_err(), "{paths:?}");
        }
    }
}
