use std::path::PathBuf;

use crate::{PureError, Result as PureResult, WorktreeCreateSpec};
use anyhow::{Context, Result};

use super::super::{AllocateExecutor, ExecutorAllocation, TaskCoordinator};
use super::{TaskExecutorBlueprint, TaskExecutorHandoff, TaskSpawnFailure, TaskSpawnResource};
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
    pub(crate) blueprint: Option<TaskExecutorBlueprint>,
}

/// Studio Task 在 framework saga prepare 阶段预留的资源事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioTaskSpawnPreparation {
    worktree: Option<WorktreeCreateSpec>,
    lifecycle_token: Option<String>,
    handoff: Option<TaskExecutorHandoff>,
    initial_context: Vec<pl_core::PinnedContextSection>,
}

impl StudioTaskSpawnPreparation {
    fn without_worktree() -> Self {
        Self {
            worktree: None,
            lifecycle_token: None,
            handoff: None,
            initial_context: Vec::new(),
        }
    }

    fn with_token(token: impl Into<String>) -> Self {
        Self {
            worktree: None,
            lifecycle_token: Some(token.into()),
            handoff: None,
            initial_context: Vec::new(),
        }
    }

    fn with_worktree_and_token(
        worktree: WorktreeCreateSpec,
        token: impl Into<String>,
        handoff: TaskExecutorHandoff,
    ) -> Self {
        Self {
            worktree: Some(worktree),
            lifecycle_token: Some(token.into()),
            handoff: Some(handoff),
            initial_context: Vec::new(),
        }
    }

    pub(crate) fn worktree_spec(&self) -> Option<&WorktreeCreateSpec> {
        self.worktree.as_ref()
    }

    pub(crate) fn lifecycle_token(&self) -> Option<&str> {
        self.lifecycle_token.as_deref()
    }

    pub(crate) fn initial_context(&self) -> &[pl_core::PinnedContextSection] {
        &self.initial_context
    }

    pub(crate) fn task_run_id(&self) -> Option<&str> {
        self.handoff
            .as_ref()
            .map(|handoff| handoff.ownership.task_run_id.as_str())
    }

    pub(crate) fn spawn_resource(&self) -> Option<TaskSpawnResource> {
        self.worktree.as_ref().map(|worktree| TaskSpawnResource {
            repo_root: worktree.repo_root.to_string_lossy().to_string(),
            path: worktree.path.to_string_lossy().to_string(),
            branch: worktree.branch.clone(),
            base_ref: "HEAD".to_string(),
        })
    }

    fn finalize_executor_worktree(&mut self, actual_base_commit: &str) -> Result<()> {
        let worktree = self
            .worktree
            .as_mut()
            .context("executor spawn preparation has no worktree spec")?;
        let handoff = self
            .handoff
            .as_mut()
            .context("executor spawn preparation has no handoff")?;
        worktree.base_commit = actual_base_commit.to_string();
        handoff.repository.base_commit = actual_base_commit.to_string();
        self.initial_context = vec![handoff.to_context_section()?];
        Ok(())
    }
}

impl TaskCoordinator {
    pub(crate) async fn prepare_agent_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
    ) -> PureResult<StudioTaskSpawnPreparation> {
        self.task_runtime
            .ensure_accepts_new_work()
            .map_err(|error| spawn_error(error.to_string()))?;
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
        if !self
            .task_runtime
            .has_active_task(&request.root_thread_id)
            .await
        {
            return Ok(StudioTaskSpawnPreparation::without_worktree());
        }
        Ok(StudioTaskSpawnPreparation::without_worktree())
    }

    async fn prepare_executor_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
    ) -> PureResult<StudioTaskSpawnPreparation> {
        let scope_hints = normalize_scope_hints(&request.scope_hints)
            .map_err(|error| spawn_error(error.to_string()))?;
        let blueprint = request
            .blueprint
            .clone()
            .ok_or_else(|| spawn_error("executor spawn has no implementation blueprint"))?
            .normalize_and_validate()
            .map_err(|error| spawn_error(error.to_string()))?;
        if blueprint.task_name != request.task_name || blueprint.scope.scope_hints != scope_hints {
            return Err(spawn_error(
                "executor blueprint does not match its canonical spawn allocation",
            ));
        }
        let allocation = self
            .reserve_executor_spawn(AllocateExecutor {
                thread_id: request.root_thread_id.clone(),
                title: request.task_name.clone(),
                scope_hints: scope_hints.clone(),
                agent_id: request.agent_id.clone(),
                requested_by_call_id: request.requested_by_call_id.clone(),
            })
            .await?;
        if allocation.work_unit.executor_thread_id.as_deref() != Some(request.agent_id.as_str()) {
            return Err(spawn_error(
                "executor spawn identity does not match the canonical active allocation",
            ));
        }
        let work_unit_id = allocation.work_unit.id.clone();
        let handoff = TaskExecutorHandoff::new(
            &allocation.run,
            &allocation.work_unit,
            request.root_thread_id.clone(),
            blueprint,
        )
        .map_err(|error| spawn_error(error.to_string()))?;
        Ok(StudioTaskSpawnPreparation::with_worktree_and_token(
            WorktreeCreateSpec {
                repo_root: PathBuf::from(&allocation.run.workspace_root),
                path: PathBuf::from(&allocation.work_unit.worktree_path),
                branch: allocation.work_unit.branch.clone(),
                base_commit: allocation.work_unit.base_commit.clone(),
            },
            work_unit_id,
            handoff,
        ))
    }

    pub(crate) async fn reserve_executor_spawn(
        &self,
        input: AllocateExecutor,
    ) -> PureResult<ExecutorAllocation> {
        let _mutation_guard = self.lock_branch_mutation().await;
        let _allocation_guard = self.allocation_lock.lock().await;
        self.task_runtime
            .allocate_executor(input)
            .await
            .map_err(store_spawn_error)
    }

    async fn prepare_reviewer_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
    ) -> PureResult<StudioTaskSpawnPreparation> {
        if !request.scope_hints.is_empty() {
            return Err(spawn_error("reviewer must not declare scopeHints"));
        }
        let round = self
            .task_runtime
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
                self.task_runtime
                    .update_executor_allocation(
                        token,
                        &request.agent_id,
                        crate::studio::task_coordinator::WorkUnitCommand::Activate,
                    )
                    .await
                    .map_err(store_spawn_error)?;
            }
            "explorer" => {}
            "reviewer" => {
                if let Some(token) = preparation.lifecycle_token() {
                    self.task_runtime
                        .apply_review_command(
                            token,
                            crate::studio::task_coordinator::ReviewRoundCommand::Start {
                                reviewer_thread_id: request.agent_id.clone(),
                            },
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

    pub(crate) async fn finalize_executor_worktree(
        &self,
        request: &StudioTaskSpawnRequest,
        preparation: &mut StudioTaskSpawnPreparation,
        actual_base_commit: &str,
    ) -> PureResult<()> {
        let token = preparation
            .lifecycle_token()
            .ok_or_else(|| spawn_error("executor worktree has no allocation token"))?
            .to_string();
        preparation
            .finalize_executor_worktree(actual_base_commit)
            .map_err(|error| spawn_error(error.to_string()))?;
        self.task_runtime
            .record_executor_worktree_base(&token, &request.agent_id, actual_base_commit)
            .await
            .map_err(store_spawn_error)?;
        Ok(())
    }

    pub(crate) async fn rollback_agent_spawn(
        &self,
        request: &StudioTaskSpawnRequest,
        preparation: &StudioTaskSpawnPreparation,
        failure: TaskSpawnFailure,
    ) -> PureResult<()> {
        match request.role.as_str() {
            "executor" => {
                let token = preparation.lifecycle_token().ok_or_else(|| {
                    spawn_error("executor spawn preparation has no allocation token")
                })?;
                self.task_runtime
                    .update_executor_allocation(
                        token,
                        &request.agent_id,
                        crate::studio::task_coordinator::WorkUnitCommand::FailSpawn {
                            failure: Box::new(failure),
                        },
                    )
                    .await
                    .map_err(store_spawn_error)?;
            }
            "explorer" => {}
            "reviewer" => {
                self.task_runtime
                    .fail_reviewer_spawn(
                        &request.root_thread_id,
                        Some(&request.agent_id),
                        &request.requested_by_call_id,
                        &failure.message,
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
        self.task_runtime
            .executor_close_disposition(&request.root_thread_id, token, &request.agent_id, true)
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
        self.task_runtime
            .executor_close_disposition(&request.root_thread_id, token, &request.agent_id, false)
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
