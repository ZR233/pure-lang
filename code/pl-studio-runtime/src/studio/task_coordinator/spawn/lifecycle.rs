use std::path::PathBuf;

use crate::{PureError, Result as PureResult, WorktreeCreateSpec};
use anyhow::{Result, bail};

use super::super::{AgentOutcomeStatus, AllocateExecutor, CreateAgentOutcome, TaskCoordinator};
use crate::studio::task_coordinator::owned_path::OwnedPath;

/// Studio Task 产品层为一次 child spawn 固定的业务输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioTaskSpawnRequest {
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
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

    #[cfg(test)]
    pub(crate) fn test_without_worktree() -> Self {
        Self::without_worktree()
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
        let Some(run) = self
            .store
            .list_active_task_runs()
            .await
            .map_err(store_spawn_error)?
            .into_iter()
            .find(|run| run.session_id == request.session_id)
        else {
            return Ok(StudioTaskSpawnPreparation::without_worktree());
        };
        let outcome = self
            .store
            .create_explorer_outcome(
                &request.session_id,
                CreateAgentOutcome {
                    task_run_id: run.id,
                    work_unit_id: None,
                    agent_id: request.agent_id.clone(),
                    owner_path: "/root".to_string(),
                    initiated_by: "planner".to_string(),
                    requested_by_call_id: request.requested_by_call_id.clone(),
                    role: "explorer".to_string(),
                    status: AgentOutcomeStatus::Queued,
                    attempt: 1,
                },
            )
            .await
            .map_err(store_spawn_error)?
            .ok_or_else(|| spawn_error("active task run disappeared"))?;
        Ok(StudioTaskSpawnPreparation::with_token(outcome.id))
    }

    async fn prepare_executor_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
    ) -> PureResult<StudioTaskSpawnPreparation> {
        let owned_paths = normalize_owned_paths(&request.owned_paths)
            .map_err(|error| spawn_error(error.to_string()))?;
        let _mutation_guard = self.lock_branch_mutation().await;
        let _allocation_guard = self.allocation_lock.lock().await;
        let allocation = self
            .store
            .allocate_executor(AllocateExecutor {
                session_id: request.session_id.clone(),
                title: request.task_name.clone(),
                owned_paths,
                agent_id: request.agent_id.clone(),
                owner_path: "/root".to_string(),
                requested_by_call_id: request.requested_by_call_id.clone(),
            })
            .await
            .map_err(store_spawn_error)?;
        debug_assert_eq!(allocation.outcome.agent_id, request.agent_id);
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
        let (_, outcome) = self
            .store
            .authorize_reviewer_spawn(
                &request.session_id,
                &request.requested_by_call_id,
                &request.agent_id,
            )
            .await
            .map_err(store_spawn_error)?;
        Ok(StudioTaskSpawnPreparation::with_token(outcome.id))
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
            "explorer" | "reviewer" => {
                if let Some(token) = preparation.lifecycle_token() {
                    self.store
                        .update_spawned_outcome(
                            token,
                            &request.agent_id,
                            AgentOutcomeStatus::Running,
                            None,
                        )
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
            "explorer" => {
                if let Some(token) = preparation.lifecycle_token() {
                    self.store
                        .update_spawned_outcome(
                            token,
                            &request.agent_id,
                            AgentOutcomeStatus::Failed,
                            Some(error.to_string()),
                        )
                        .await
                        .map_err(store_spawn_error)?;
                }
            }
            "reviewer" => {
                self.store
                    .fail_reviewer_spawn(
                        &request.session_id,
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
    ) -> PureResult<()> {
        if request.role != "executor" {
            return Ok(());
        }
        let token = preparation
            .lifecycle_token()
            .ok_or_else(|| spawn_error("task executor close requires an exact lifecycle token"))?;
        self.store
            .cancel_executor_for_discard(&request.session_id, token, &request.agent_id)
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
    use super::{normalize_owned_paths, owned_paths_overlap};

    #[test]
    fn owned_path_overlap_uses_product_path_rules() {
        assert!(owned_paths_overlap(&["src/**".into()], &["src/lib.rs".into()]).unwrap());
        assert!(!owned_paths_overlap(&["src".into()], &["src/lib.rs".into()]).unwrap());
        assert!(!owned_paths_overlap(&["src".into()], &["tests".into()]).unwrap());
    }

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
