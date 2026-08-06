use std::path::PathBuf;

use crate::{PureError, Result as PureResult, WorktreeCreateSpec};
use anyhow::Result;

use super::super::{AllocateExecutor, TaskCoordinator};
use crate::studio::task_coordinator::scope_hint::ScopeHint;

/// Studio Task 产品层为一次 child spawn 固定的业务输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioTaskSpawnRequest {
    pub(crate) agent_id: String,
    pub(crate) root_thread_id: String,
    pub(crate) task_name: String,
    pub(crate) role: String,
    pub(crate) scope_hints: Vec<String>,
    pub(crate) requested_by_call_id: String,
    pub(crate) review_round_id: Option<String>,
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
        if !request.scope_hints.is_empty() {
            return Err(spawn_error("explorer must not declare scopeHints"));
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
        let scope_hints = normalize_scope_hints(&request.scope_hints)
            .map_err(|error| spawn_error(error.to_string()))?;
        let _mutation_guard = self.lock_branch_mutation().await;
        let _allocation_guard = self.allocation_lock.lock().await;
        let run = self
            .store
            .read_active_task_run_for_root_thread(&request.root_thread_id)
            .await
            .map_err(store_spawn_error)?;
        self.ensure_executor_design_contract(&run)
            .map_err(store_spawn_error)?;
        let allocation = self
            .store
            .allocate_executor(AllocateExecutor {
                thread_id: request.root_thread_id.clone(),
                title: request.task_name.clone(),
                scope_hints,
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
        if !request.scope_hints.is_empty() {
            return Err(spawn_error("reviewer must not declare scopeHints"));
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
        if request.review_round_id.as_deref() != Some(round.id.as_str()) {
            return Err(spawn_error(
                "reviewer spawn intent does not match its durable ReviewRound",
            ));
        }
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

pub(crate) fn normalize_scope_hints(paths: &[String]) -> Result<Vec<String>> {
    let mut canonical = paths
        .iter()
        .map(|path| ScopeHint::parse(path))
        .collect::<Result<Vec<_>>>()?;
    let mut canonical = canonical
        .drain(..)
        .map(ScopeHint::into_canonical)
        .collect::<Vec<_>>();
    canonical.sort();
    canonical.dedup();
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
    use super::normalize_scope_hints;

    #[test]
    fn executor_scope_hints_are_optional_and_canonicalized_before_spawn() {
        assert!(normalize_scope_hints(&[]).unwrap().is_empty());
        assert_eq!(
            normalize_scope_hints(&["tests\\case.rs".into(), "src".into(), "src".into()]).unwrap(),
            vec!["src", "tests/case.rs"]
        );
        for paths in [vec!["../src".into()], vec!["src/*".into()]] {
            assert!(normalize_scope_hints(&paths).is_err(), "{paths:?}");
        }
    }
}
