use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Result, bail};
use pl_protocol::{PureError, Result as PureResult};

use super::super::{AgentOutcomeStatus, AllocateExecutor, CreateAgentOutcome, TaskCoordinator};
use crate::agent::{
    AgentCloseDispositionKind, AgentCloseLifecycleRequest, AgentLifecycleHook,
    AgentLifecycleProjection, AgentLifecycleProjectionRequest, AgentSpawnLifecycleRequest,
    AgentSpawnPreparation, WorktreeCreateSpec,
};
use crate::studio::task_coordinator::owned_path::OwnedPath;

#[derive(Clone)]
struct TaskAgentLifecycleHook {
    coordinator: Arc<TaskCoordinator>,
    session_id: String,
}

impl std::fmt::Debug for TaskAgentLifecycleHook {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskAgentLifecycleHook")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

impl TaskCoordinator {
    pub(crate) fn lifecycle_hook(
        self: &Arc<Self>,
        session_id: &str,
    ) -> Arc<dyn AgentLifecycleHook> {
        Arc::new(TaskAgentLifecycleHook {
            coordinator: self.clone(),
            session_id: session_id.to_string(),
        })
    }
}

impl AgentLifecycleHook for TaskAgentLifecycleHook {
    fn prepare_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = PureResult<AgentSpawnPreparation>> + Send + 'a>>
    {
        Box::pin(async move {
            self.validate_owner(request)?;
            match request.role.as_str() {
                "explorer" => {
                    if !request.owned_paths.is_empty() {
                        return Err(spawn_error("explorer must not declare ownedPaths"));
                    }
                    let Some(run) = self
                        .coordinator
                        .store
                        .list_active_task_runs()
                        .await
                        .map_err(|error| spawn_error(error.to_string()))?
                        .into_iter()
                        .find(|run| run.session_id == self.session_id)
                    else {
                        return Ok(AgentSpawnPreparation::without_worktree());
                    };
                    let outcome = self
                        .coordinator
                        .store
                        .create_explorer_outcome(
                            &self.session_id,
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
                        .map_err(|error| spawn_error(error.to_string()))?
                        .ok_or_else(|| spawn_error("active task run disappeared"))?;
                    Ok(AgentSpawnPreparation::with_token(outcome.id))
                }
                "executor" => {
                    let owned_paths = normalize_owned_paths(&request.owned_paths)
                        .map_err(|error| spawn_error(error.to_string()))?;
                    let _mutation_guard = self.coordinator.lock_branch_mutation().await;
                    let _allocation_guard = self.coordinator.allocation_lock.lock().await;
                    let allocation = self
                        .coordinator
                        .store
                        .allocate_executor(AllocateExecutor {
                            session_id: self.session_id.clone(),
                            title: request.task_name.clone(),
                            owned_paths,
                            agent_id: request.agent_id.clone(),
                            owner_path: request.owner_path.clone(),
                            requested_by_call_id: request.requested_by_call_id.clone(),
                        })
                        .await
                        .map_err(|error| spawn_error(error.to_string()))?;
                    debug_assert_eq!(allocation.outcome.agent_id, request.agent_id);
                    let work_unit_id = allocation.work_unit.id.clone();
                    Ok(AgentSpawnPreparation::with_worktree_and_token(
                        WorktreeCreateSpec {
                            repo_root: PathBuf::from(&allocation.run.workspace_root),
                            path: PathBuf::from(&allocation.work_unit.worktree_path),
                            branch: allocation.work_unit.branch,
                            base_commit: allocation.work_unit.base_commit,
                        },
                        work_unit_id,
                    ))
                }
                "reviewer" => {
                    if !request.owned_paths.is_empty() {
                        return Err(spawn_error("reviewer must not declare ownedPaths"));
                    }
                    let (_, outcome) = self
                        .coordinator
                        .store
                        .authorize_reviewer_spawn(
                            &self.session_id,
                            &request.requested_by_call_id,
                            &request.agent_id,
                        )
                        .await
                        .map_err(|error| spawn_error(error.to_string()))?;
                    Ok(AgentSpawnPreparation::with_token(outcome.id))
                }
                role => Err(spawn_error(format!("task {role} creation is harness-only"))),
            }
        })
    }

    fn activate_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
        preparation: &'a AgentSpawnPreparation,
    ) -> Pin<Box<dyn std::future::Future<Output = PureResult<()>> + Send + 'a>> {
        Box::pin(async move {
            match request.role.as_str() {
                "executor" => {
                    let work_unit_id = preparation.lifecycle_token().ok_or_else(|| {
                        spawn_error("executor spawn preparation has no allocation token")
                    })?;
                    self.coordinator
                        .store
                        .activate_executor(work_unit_id, &request.agent_id)
                        .await
                        .map_err(|error| spawn_error(error.to_string()))?;
                }
                "explorer" | "reviewer" => {
                    if let Some(outcome_id) = preparation.lifecycle_token() {
                        self.coordinator
                            .store
                            .update_spawned_outcome(
                                outcome_id,
                                &request.agent_id,
                                AgentOutcomeStatus::Running,
                                None,
                            )
                            .await
                            .map_err(|error| spawn_error(error.to_string()))?;
                    }
                }
                _ => {}
            }
            Ok(())
        })
    }

    fn rollback_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
        preparation: &'a AgentSpawnPreparation,
        error: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = PureResult<()>> + Send + 'a>> {
        Box::pin(async move {
            match request.role.as_str() {
                "executor" => {
                    let work_unit_id = preparation.lifecycle_token().ok_or_else(|| {
                        spawn_error("executor spawn preparation has no allocation token")
                    })?;
                    self.coordinator
                        .store
                        .fail_executor(work_unit_id, &request.agent_id, error)
                        .await
                        .map_err(|failure| spawn_error(failure.to_string()))?;
                }
                "explorer" => {
                    if let Some(outcome_id) = preparation.lifecycle_token() {
                        self.coordinator
                            .store
                            .update_spawned_outcome(
                                outcome_id,
                                &request.agent_id,
                                AgentOutcomeStatus::Failed,
                                Some(error.to_string()),
                            )
                            .await
                            .map_err(|failure| spawn_error(failure.to_string()))?;
                    }
                }
                "reviewer" => {
                    self.coordinator
                        .store
                        .fail_reviewer_spawn(
                            &self.session_id,
                            Some(&request.agent_id),
                            &request.requested_by_call_id,
                            error,
                        )
                        .await
                        .map_err(|failure| spawn_error(failure.to_string()))?;
                }
                _ => {}
            }
            Ok(())
        })
    }

    fn validate_close<'a>(
        &'a self,
        request: &'a AgentCloseLifecycleRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = PureResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if request.role == "executor" && request.disposition == AgentCloseDispositionKind::Merge
            {
                return Err(PureError::ToolExecutionFailed {
                    tool: "close_agent".to_string(),
                    error: "task executor worktrees must be delivered explicitly; close_agent merge=true is not allowed".to_string(),
                });
            }
            Ok(())
        })
    }

    fn project_snapshot<'a>(
        &'a self,
        request: &'a AgentLifecycleProjectionRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = PureResult<AgentLifecycleProjection>> + Send + 'a>>
    {
        Box::pin(async move {
            self.coordinator
                .store
                .project_agent_lifecycle(&request.lifecycle_token, &request.role)
                .await
                .map_err(|error| spawn_error(error.to_string()))
                .map(|projection| projection.unwrap_or_else(|| request.snapshot.clone()))
        })
    }
}

impl TaskAgentLifecycleHook {
    fn validate_owner(&self, request: &AgentSpawnLifecycleRequest) -> PureResult<()> {
        // request.session_id is the tool execution scope (the root turn id).
        // The Studio session boundary is fixed by this per-session hook.
        if request.owner_path != "/root" {
            return Err(spawn_error(
                "only the root Task planner may create task agents",
            ));
        }
        Ok(())
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

fn normalize_owned_paths(paths: &[String]) -> Result<Vec<String>> {
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
    Ok(normalized
        .into_iter()
        .map(OwnedPath::into_canonical)
        .collect())
}

fn spawn_error(error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "spawn_agent".to_string(),
        error: error.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::agent::{
        AgentLifecycleHook, AgentRunSpec, AgentSpawnInput, AgentSpawnLifecycleRequest,
        AgentSupervisor,
    };
    use crate::tool::{
        ListAgentsTool, Tool, ToolContext, ToolInput, WaitAgentTool, WorkspaceAccess,
    };
    use crate::{CompileMode, PureCoreBuilder, StudioStore, TurnBudget, TurnOptions};

    use crate::studio::task_coordinator::{
        AgentOutcomeStatus, TaskCoordinator, TaskRunPhase, WorkUnitStatus,
    };

    use super::{normalize_owned_paths, owned_paths_overlap};
    use crate::studio::task_coordinator::owned_path::OwnedPath;

    #[tokio::test]
    async fn prepare_activate_and_rollback_persist_exact_executor_allocation() {
        let repository = init_repository("spawn-lifecycle");
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project(&repository).await.unwrap();
        let session = store
            .create_session(&project.id, "Task", CompileMode::Task)
            .await
            .unwrap();
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, "implement", &repository)
            .await
            .unwrap();
        let run = store
            .transition_task_run(&run.id, TaskRunPhase::Implementing, None)
            .await
            .unwrap();
        let hook: Arc<dyn AgentLifecycleHook> = coordinator.lifecycle_hook(&session.id);
        let request = AgentSpawnLifecycleRequest {
            agent_id: "agent-7".to_string(),
            agent_path: "/root/implement_core".to_string(),
            owner_path: "/root".to_string(),
            session_id: session.id.clone(),
            task_name: "implement_core".to_string(),
            role: "executor".to_string(),
            owned_paths: vec!["code/pl-core/**".to_string()],
            requested_by_call_id: "call-spawn-7".to_string(),
        };

        let preparation = hook.prepare_spawn(&request).await.unwrap();

        let work_units = store.list_work_units(&run.id).await.unwrap();
        let outcomes = store.list_agent_outcomes(&run.id).await.unwrap();
        assert_eq!(work_units.len(), 1);
        assert_eq!(outcomes.len(), 1);
        let expected_path = crate::agent::worktree::git_compatible_path(
            Path::new(&run.workspace_root)
                .join(".pure")
                .join("worktrees")
                .join(&run.id)
                .join("agent-7"),
        );
        assert_eq!(work_units[0].status, WorkUnitStatus::Pending);
        assert_eq!(work_units[0].agent_id.as_deref(), Some("agent-7"));
        assert_eq!(work_units[0].owned_paths, vec!["code/pl-core/**"]);
        assert_eq!(work_units[0].base_commit, run.expected_head);
        assert_eq!(work_units[0].worktree_path, expected_path.to_string_lossy());
        assert_eq!(
            work_units[0].branch,
            format!("pure-task-{}-agent-7", run.id)
        );
        assert_eq!(work_units[0].attempt, 1);
        assert_eq!(outcomes[0].status, AgentOutcomeStatus::Queued);
        assert_eq!(
            outcomes[0].work_unit_id.as_deref(),
            Some(work_units[0].id.as_str())
        );
        assert_eq!(outcomes[0].owner_path, "/root");
        assert_eq!(outcomes[0].initiated_by, "planner");
        assert_eq!(outcomes[0].requested_by_call_id, "call-spawn-7");

        hook.activate_spawn(&request, &preparation).await.unwrap();

        assert_eq!(
            store.list_work_units(&run.id).await.unwrap()[0].status,
            WorkUnitStatus::Running
        );
        assert_eq!(
            store.list_agent_outcomes(&run.id).await.unwrap()[0].status,
            AgentOutcomeStatus::Running
        );

        hook.rollback_spawn(&request, &preparation, "turn startup failed")
            .await
            .unwrap();

        let work_units = store.list_work_units(&run.id).await.unwrap();
        let outcomes = store.list_agent_outcomes(&run.id).await.unwrap();
        assert_eq!(work_units.len(), 1);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(work_units[0].status, WorkUnitStatus::Failed);
        assert_eq!(outcomes[0].status, AgentOutcomeStatus::Failed);
        assert_eq!(outcomes[0].error.as_deref(), Some("turn startup failed"));
        drop(hook);
        drop(coordinator);
        std::fs::remove_dir_all(repository).ok();
    }

    #[tokio::test]
    async fn executor_owned_paths_must_be_non_empty_valid_and_non_overlapping() {
        let fixture = SpawnFixture::new("owned-path-guards").await;
        for (agent_id, paths, expected) in [
            ("agent-empty", vec![], "must not be empty"),
            ("agent-parent", vec!["../code"], "invalid owned path"),
            (
                "agent-overlap",
                vec!["code/**", "code/pl-core/**"],
                "must not overlap",
            ),
        ] {
            let error = fixture
                .hook
                .prepare_spawn(&fixture.request(agent_id, "invalid", "executor", paths))
                .await
                .expect_err("invalid ownedPaths must be rejected");
            assert!(error.to_string().contains(expected));
        }
        assert!(
            fixture
                .store
                .list_work_units(&fixture.run_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            fixture
                .store
                .list_agent_outcomes(&fixture.run_id)
                .await
                .unwrap()
                .is_empty()
        );
        fixture.cleanup();
    }

    #[test]
    fn owned_paths_reject_trailing_separators_without_directory_glob() {
        for path in ["src/", r"src\"] {
            let error =
                OwnedPath::parse(path).expect_err("a directory must use the explicit /** suffix");
            assert!(error.to_string().contains("invalid owned path"));
        }
    }

    #[test]
    fn owned_path_case_comparison_follows_platform_semantics() {
        let upper = vec!["Src/**".to_string()];
        let lower = vec!["src/**".to_string()];

        assert_eq!(owned_paths_overlap(&upper, &lower).unwrap(), cfg!(windows));
        assert_eq!(
            normalize_owned_paths(&upper).unwrap(),
            vec!["Src/**".to_string()]
        );
    }

    #[tokio::test]
    async fn active_owned_paths_and_fifth_executor_are_rejected() {
        let fixture = SpawnFixture::new("active-guards").await;
        for index in 1..=4 {
            let request = fixture.request(
                &format!("agent-{index}"),
                &format!("task-{index}"),
                "executor",
                vec![&format!("area-{index}/**")],
            );
            let preparation = fixture.hook.prepare_spawn(&request).await.unwrap();
            fixture
                .hook
                .activate_spawn(&request, &preparation)
                .await
                .unwrap();
        }
        let fifth = fixture.request("agent-5", "task-5", "executor", vec!["area-5/**"]);
        let error = fixture
            .hook
            .prepare_spawn(&fifth)
            .await
            .expect_err("fifth active executor must be rejected");
        assert!(error.to_string().contains("at most 4"));

        let overlap_fixture = SpawnFixture::new("overlap-guard").await;
        let owner = overlap_fixture.request("agent-owner", "owner", "executor", vec!["code/**"]);
        let preparation = overlap_fixture.hook.prepare_spawn(&owner).await.unwrap();
        overlap_fixture
            .hook
            .activate_spawn(&owner, &preparation)
            .await
            .unwrap();
        let overlap = overlap_fixture.request(
            "agent-overlap",
            "overlap",
            "executor",
            vec!["code/pl-core/**"],
        );
        let error = overlap_fixture
            .hook
            .prepare_spawn(&overlap)
            .await
            .expect_err("active ownership must be exclusive");
        assert!(error.to_string().contains("overlap active work unit"));
        fixture.cleanup();
        overlap_fixture.cleanup();
    }

    #[tokio::test]
    async fn parallel_executor_allocation_is_serialized_before_capacity_and_overlap_checks() {
        let capacity = SpawnFixture::new("parallel-capacity").await;
        let requests = (1..=5)
            .map(|index| {
                capacity.request(
                    &format!("agent-{index}"),
                    &format!("task-{index}"),
                    "executor",
                    vec![&format!("area-{index}/**")],
                )
            })
            .collect::<Vec<_>>();
        let results = tokio::join!(
            capacity.hook.prepare_spawn(&requests[0]),
            capacity.hook.prepare_spawn(&requests[1]),
            capacity.hook.prepare_spawn(&requests[2]),
            capacity.hook.prepare_spawn(&requests[3]),
            capacity.hook.prepare_spawn(&requests[4]),
        );
        let results = [results.0, results.1, results.2, results.3, results.4];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 4);
        let errors = results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].contains("at most 4"),
            "unexpected error: {errors:?}"
        );
        capacity.cleanup();

        let overlap = SpawnFixture::new("parallel-overlap").await;
        let left = overlap.request("agent-left", "left", "executor", vec!["code/**"]);
        let right = overlap.request("agent-right", "right", "executor", vec!["code/pl-core/**"]);
        let (left, right) = tokio::join!(
            overlap.hook.prepare_spawn(&left),
            overlap.hook.prepare_spawn(&right)
        );
        let results = [left, right];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let error = results
            .iter()
            .find_map(|result| result.as_ref().err())
            .expect("one overlapping allocation must fail")
            .to_string();
        assert!(
            error.contains("overlap active work unit"),
            "unexpected error: {error}"
        );
        overlap.cleanup();
    }

    #[tokio::test]
    async fn fourth_attempt_is_rejected_and_failed_attempts_remain_auditable() {
        let fixture = SpawnFixture::new("attempt-guard").await;
        for index in 1..=3 {
            let request = fixture.request(
                &format!("agent-{index}"),
                "same-task",
                "executor",
                vec!["code/pl-core/**"],
            );
            let preparation = fixture.hook.prepare_spawn(&request).await.unwrap();
            fixture
                .hook
                .activate_spawn(&request, &preparation)
                .await
                .unwrap();
            fixture
                .hook
                .rollback_spawn(&request, &preparation, "failed attempt")
                .await
                .unwrap();
        }
        let fourth = fixture.request("agent-4", "same-task", "executor", vec!["code/pl-core/**"]);
        let error = fixture
            .hook
            .prepare_spawn(&fourth)
            .await
            .expect_err("attempt four must be rejected");
        assert!(error.to_string().contains("1..=3"));
        assert_eq!(
            fixture
                .store
                .list_work_units(&fixture.run_id)
                .await
                .unwrap()
                .iter()
                .map(|unit| unit.attempt)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        fixture.cleanup();
    }

    #[tokio::test]
    async fn explorer_has_durable_outcome_without_work_unit_and_task_merge_close_is_rejected() {
        let fixture = SpawnFixture::new("role-guards").await;
        let explorer = fixture.request("agent-explorer", "inspect", "explorer", vec![]);
        let preparation = fixture.hook.prepare_spawn(&explorer).await.unwrap();
        assert!(
            fixture
                .store
                .list_work_units(&fixture.run_id)
                .await
                .unwrap()
                .is_empty()
        );
        let outcomes = fixture
            .store
            .list_agent_outcomes(&fixture.run_id)
            .await
            .unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].work_unit_id, None);
        assert_eq!(outcomes[0].owner_path, "/root");
        assert_eq!(outcomes[0].role, "explorer");
        assert_eq!(outcomes[0].requested_by_call_id, "call-agent-explorer");
        assert_eq!(outcomes[0].status, AgentOutcomeStatus::Queued);

        fixture
            .hook
            .activate_spawn(&explorer, &preparation)
            .await
            .unwrap();
        assert_eq!(
            fixture
                .store
                .list_agent_outcomes(&fixture.run_id)
                .await
                .unwrap()[0]
                .status,
            AgentOutcomeStatus::Running
        );
        let projection = fixture
            .coordinator
            .record_terminal_agent_state(
                &fixture.session_id,
                &crate::agent::AgentTerminalStateChange {
                    agent_id: explorer.agent_id.clone(),
                    role: explorer.role.clone(),
                    status: crate::AgentStatus::Completed,
                    summary: Some("exploration complete".to_string()),
                    error: None,
                },
            )
            .await
            .unwrap()
            .into_projection()
            .unwrap();
        assert_eq!(projection.status, crate::AgentStatus::Completed);
        assert_eq!(
            fixture
                .store
                .list_agent_outcomes(&fixture.run_id)
                .await
                .unwrap()[0]
                .status,
            AgentOutcomeStatus::Completed
        );

        let merge = crate::agent::AgentCloseLifecycleRequest {
            agent_id: "agent-executor".to_string(),
            agent_path: "/root/executor".to_string(),
            role: "executor".to_string(),
            disposition: crate::agent::AgentCloseDispositionKind::Merge,
        };
        let error = fixture
            .hook
            .validate_close(&merge)
            .await
            .expect_err("Task executor generic merge must be rejected");
        assert!(error.to_string().contains("merge=true is not allowed"));
        fixture.cleanup();
    }

    #[tokio::test]
    async fn task_core_installation_enforces_executor_allocation_before_turn_start() {
        let fixture = SpawnFixture::new("core-installation").await;
        let supervisor = AgentSupervisor::default();
        let mut core = PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
            .unwrap()
            .with_agent_supervisor(supervisor.clone())
            .build();
        fixture
            .coordinator
            .install_tools(&mut core, &fixture.session_id);

        let error = supervisor
            .spawn_agent(
                AgentSpawnInput {
                    task_name: "implement".to_string(),
                    message: "implement".to_string(),
                    role: "executor".to_string(),
                    parent_path: Some("/root".to_string()),
                    session_id: fixture.session_id.clone(),
                    owned_paths: Vec::new(),
                },
                test_run_spec("implement"),
            )
            .await
            .expect_err("installed Task hook must reject missing ownedPaths");

        assert!(error.to_string().contains("ownedPaths must not be empty"));
        assert!(supervisor.list_agents(None).await.unwrap().is_empty());
        fixture.cleanup();
    }

    #[tokio::test]
    async fn installed_hook_creates_exact_worktree_and_rejects_implicit_merge_commit() {
        let fixture = SpawnFixture::new("exact-installed-spawn").await;
        let supervisor = AgentSupervisor::default();
        supervisor
            .enable_worktrees(PathBuf::from(
                fixture
                    .store
                    .read_task_run(&fixture.run_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .workspace_root,
            ))
            .await;
        let mut core = PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
            .unwrap()
            .with_agent_supervisor(supervisor.clone())
            .build();
        fixture
            .coordinator
            .install_tools(&mut core, &fixture.session_id);

        let handle = supervisor
            .spawn_agent(
                AgentSpawnInput {
                    task_name: "implement_exact".to_string(),
                    message: "implement".to_string(),
                    role: "executor".to_string(),
                    parent_path: Some("/root".to_string()),
                    session_id: fixture.session_id.clone(),
                    owned_paths: vec!["code/pl-core/**".to_string()],
                },
                test_run_spec("implement"),
            )
            .await
            .unwrap();
        let work_unit = fixture
            .store
            .list_work_units(&fixture.run_id)
            .await
            .unwrap()
            .remove(0);
        let worktree = handle.worktree.as_ref().expect("Task executor worktree");
        assert_eq!(worktree.path, work_unit.worktree_path);
        assert_eq!(worktree.branch, work_unit.branch);
        assert_eq!(
            git_output(Path::new(&worktree.path), &["rev-parse", "HEAD"]),
            work_unit.base_commit
        );
        std::fs::write(
            Path::new(&worktree.path).join("uncommitted.txt"),
            "pending\n",
        )
        .unwrap();
        let head_before = git_output(Path::new(&worktree.path), &["rev-parse", "HEAD"]);
        let (event_tx, _) = tokio::sync::broadcast::channel(8);

        let error = supervisor
            .close_agent(
                "/root",
                &handle.id,
                "merge",
                &event_tx,
                "call-close-merge".to_string(),
                crate::agent::CloseDisposition::Merge {
                    target_branch: None,
                },
            )
            .await
            .expect_err("Task close merge must fail before implicit commit");

        assert!(error.to_string().contains("merge=true is not allowed"));
        assert_eq!(
            git_output(Path::new(&worktree.path), &["rev-parse", "HEAD"]),
            head_before
        );
        assert!(
            git_output(Path::new(&worktree.path), &["status", "--porcelain"])
                .contains("?? uncommitted.txt")
        );
        assert!(!fixture.repository.join("uncommitted.txt").exists());

        supervisor
            .close_agent(
                "/root",
                &handle.id,
                "discard",
                &event_tx,
                "call-close-discard".to_string(),
                crate::agent::CloseDisposition::Discard,
            )
            .await
            .unwrap();
        assert!(!Path::new(&worktree.path).exists());
        assert!(
            git_output(
                &fixture.repository,
                &["branch", "--list", &work_unit.branch]
            )
            .is_empty()
        );
        fixture.cleanup();
    }

    #[tokio::test]
    async fn installed_hook_projects_durable_waiting_state_into_supervisor_list() {
        let fixture = SpawnFixture::new("durable-list-projection").await;
        let supervisor = AgentSupervisor::default();
        supervisor
            .enable_worktrees(PathBuf::from(
                fixture
                    .store
                    .read_task_run(&fixture.run_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .workspace_root,
            ))
            .await;
        let mut core = PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
            .unwrap()
            .with_agent_supervisor(supervisor.clone())
            .build();
        fixture
            .coordinator
            .install_tools(&mut core, &fixture.session_id);
        let handle = supervisor
            .spawn_agent(
                AgentSpawnInput {
                    task_name: "durable_list".to_string(),
                    message: "implement".to_string(),
                    role: "executor".to_string(),
                    parent_path: Some("/root".to_string()),
                    session_id: fixture.session_id.clone(),
                    owned_paths: vec!["code/pl-core/**".to_string()],
                },
                test_run_spec("implement"),
            )
            .await
            .unwrap();
        supervisor
            .update_status(
                &handle.id,
                crate::AgentStatus::Completed,
                Some("memory complete".to_string()),
                None,
            )
            .await;
        fixture
            .coordinator
            .record_terminal_agent_state(
                &fixture.session_id,
                &crate::agent::AgentTerminalStateChange {
                    agent_id: handle.id.clone(),
                    role: "executor".to_string(),
                    status: crate::AgentStatus::Completed,
                    summary: Some("durable complete".to_string()),
                    error: None,
                },
            )
            .await
            .unwrap();

        let agents = supervisor.list_agents(None).await.unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].status, crate::AgentStatus::Waiting);
        assert_eq!(agents[0].summary.as_deref(), Some("durable complete"));

        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        supervisor
            .close_agent(
                "/root",
                &handle.id,
                "discard",
                &event_tx,
                "call-close".to_string(),
                crate::agent::CloseDisposition::Discard,
            )
            .await
            .unwrap();
        fixture.cleanup();
    }

    #[tokio::test]
    async fn durable_projection_database_failure_fails_list_and_wait_tools() {
        let fixture = SpawnFixture::new("durable-projection-failure").await;
        let supervisor = AgentSupervisor::default();
        supervisor
            .enable_worktrees(PathBuf::from(
                fixture
                    .store
                    .read_task_run(&fixture.run_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .workspace_root,
            ))
            .await;
        let mut core = PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
            .unwrap()
            .with_agent_supervisor(supervisor.clone())
            .build();
        fixture
            .coordinator
            .install_tools(&mut core, &fixture.session_id);
        let handle = supervisor
            .spawn_agent(
                AgentSpawnInput {
                    task_name: "projection_failure".to_string(),
                    message: "implement".to_string(),
                    role: "executor".to_string(),
                    parent_path: Some("/root".to_string()),
                    session_id: fixture.session_id.clone(),
                    owned_paths: vec!["code/pl-core/**".to_string()],
                },
                test_run_spec("implement"),
            )
            .await
            .unwrap();
        fixture
            .store
            .execute_test_sql("ALTER TABLE agent_outcomes RENAME TO unavailable_agent_outcomes")
            .await;
        let context = ToolContext {
            event_tx: tokio::sync::broadcast::channel(8).0,
            options: TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: CompileMode::Task,
            workspace_root: PathBuf::from("."),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            agent_supervisor: supervisor,
            agent_tool_registrar: None,
            lsp_runtime: None,
            parent_session: Arc::new(crate::CoreSession::new()),
        };

        let list_result = ListAgentsTool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                    session_id: fixture.session_id.clone(),
                    tool_id: "call-list".to_string(),
                    revision_base: 0,
                },
                context.clone(),
            )
            .await;
        let wait_result = WaitAgentTool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "target": handle.id,
                        "timeoutMs": 250,
                    }),
                    session_id: fixture.session_id.clone(),
                    tool_id: "call-wait".to_string(),
                    revision_base: 0,
                },
                context,
            )
            .await;

        assert!(
            list_result.is_err(),
            "list_agents must surface projection failure"
        );
        assert!(
            wait_result.is_err(),
            "wait_agent must surface projection failure"
        );
        fixture.cleanup();
    }

    #[tokio::test]
    async fn design_updating_phase_rejects_executor_allocation() {
        let repository = init_repository("phase-guard");
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project(&repository).await.unwrap();
        let session = store
            .create_session(&project.id, "Task", CompileMode::Task)
            .await
            .unwrap();
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, "implement", &repository)
            .await
            .unwrap();
        let hook = coordinator.lifecycle_hook(&session.id);
        let request = AgentSpawnLifecycleRequest {
            agent_id: "agent-1".to_string(),
            agent_path: "/root/implement".to_string(),
            owner_path: "/root".to_string(),
            session_id: session.id,
            task_name: "implement".to_string(),
            role: "executor".to_string(),
            owned_paths: vec!["code/**".to_string()],
            requested_by_call_id: "call-1".to_string(),
        };

        let error = hook
            .prepare_spawn(&request)
            .await
            .expect_err("designUpdating must gate executor allocation");

        assert!(error.to_string().contains("implementing or reworking"));
        assert!(store.list_work_units(&run.id).await.unwrap().is_empty());
        drop(hook);
        drop(coordinator);
        std::fs::remove_dir_all(repository).ok();
    }

    #[tokio::test]
    async fn identical_agent_ids_in_parallel_task_runs_update_only_their_allocation() {
        let left_repository = init_repository("identity-left");
        let right_repository = init_repository("identity-right");
        let store = StudioStore::open_memory().await.unwrap();
        let left_project = store.upsert_project(&left_repository).await.unwrap();
        let right_project = store.upsert_project(&right_repository).await.unwrap();
        let left_session = store
            .create_session(&left_project.id, "Left", CompileMode::Task)
            .await
            .unwrap();
        let right_session = store
            .create_session(&right_project.id, "Right", CompileMode::Task)
            .await
            .unwrap();
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let left_run = coordinator
            .start_confirmed_task(&left_session.id, "left", &left_repository)
            .await
            .unwrap();
        let right_run = coordinator
            .start_confirmed_task(&right_session.id, "right", &right_repository)
            .await
            .unwrap();
        store
            .transition_task_run(&left_run.id, TaskRunPhase::Implementing, None)
            .await
            .unwrap();
        store
            .transition_task_run(&right_run.id, TaskRunPhase::Implementing, None)
            .await
            .unwrap();
        let left_hook = coordinator.lifecycle_hook(&left_session.id);
        let right_hook = coordinator.lifecycle_hook(&right_session.id);
        let request = |turn_id: &str, task_name: &str| AgentSpawnLifecycleRequest {
            agent_id: "agent-1".to_string(),
            agent_path: format!("/root/{task_name}"),
            owner_path: "/root".to_string(),
            session_id: turn_id.to_string(),
            task_name: task_name.to_string(),
            role: "executor".to_string(),
            owned_paths: vec!["code/**".to_string()],
            requested_by_call_id: format!("call-{task_name}"),
        };
        let left_request = request("turn-left", "left");
        let right_request = request("turn-right", "right");
        let left_preparation = left_hook.prepare_spawn(&left_request).await.unwrap();
        let right_preparation = right_hook.prepare_spawn(&right_request).await.unwrap();

        left_hook
            .activate_spawn(&left_request, &left_preparation)
            .await
            .unwrap();
        right_hook
            .activate_spawn(&right_request, &right_preparation)
            .await
            .unwrap();
        coordinator
            .record_terminal_agent_state(
                &left_session.id,
                &crate::agent::AgentTerminalStateChange {
                    agent_id: "agent-1".to_string(),
                    role: "executor".to_string(),
                    status: crate::AgentStatus::Completed,
                    summary: Some("left complete".to_string()),
                    error: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store.list_work_units(&left_run.id).await.unwrap()[0].status,
            WorkUnitStatus::WaitingForDelivery
        );
        assert_eq!(
            store.list_work_units(&right_run.id).await.unwrap()[0].status,
            WorkUnitStatus::Running
        );
        right_hook
            .rollback_spawn(&right_request, &right_preparation, "right failed")
            .await
            .unwrap();

        assert_eq!(
            store.list_work_units(&left_run.id).await.unwrap()[0].status,
            WorkUnitStatus::WaitingForDelivery
        );
        assert_eq!(
            store.list_work_units(&right_run.id).await.unwrap()[0].status,
            WorkUnitStatus::Failed
        );
        assert_eq!(
            store.list_agent_outcomes(&left_run.id).await.unwrap()[0].status,
            AgentOutcomeStatus::WaitingForDelivery
        );
        assert_eq!(
            store.list_agent_outcomes(&right_run.id).await.unwrap()[0]
                .error
                .as_deref(),
            Some("right failed")
        );
        drop(left_hook);
        drop(right_hook);
        drop(coordinator);
        std::fs::remove_dir_all(left_repository).ok();
        std::fs::remove_dir_all(right_repository).ok();
    }

    struct SpawnFixture {
        coordinator: Arc<TaskCoordinator>,
        store: StudioStore,
        hook: Arc<dyn AgentLifecycleHook>,
        session_id: String,
        run_id: String,
        repository: PathBuf,
    }

    impl SpawnFixture {
        async fn new(name: &str) -> Self {
            let repository = init_repository(name);
            let store = StudioStore::open_memory().await.unwrap();
            let project = store.upsert_project(&repository).await.unwrap();
            let session = store
                .create_session(&project.id, "Task", CompileMode::Task)
                .await
                .unwrap();
            let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
            let run = coordinator
                .start_confirmed_task(&session.id, "implement", &repository)
                .await
                .unwrap();
            let run = store
                .transition_task_run(&run.id, TaskRunPhase::Implementing, None)
                .await
                .unwrap();
            let hook = coordinator.lifecycle_hook(&session.id);
            Self {
                coordinator,
                store,
                hook,
                session_id: session.id,
                run_id: run.id,
                repository,
            }
        }

        fn request(
            &self,
            agent_id: &str,
            task_name: &str,
            role: &str,
            owned_paths: Vec<&str>,
        ) -> AgentSpawnLifecycleRequest {
            AgentSpawnLifecycleRequest {
                agent_id: agent_id.to_string(),
                agent_path: format!("/root/{task_name}"),
                owner_path: "/root".to_string(),
                session_id: self.session_id.clone(),
                task_name: task_name.to_string(),
                role: role.to_string(),
                owned_paths: owned_paths.into_iter().map(str::to_string).collect(),
                requested_by_call_id: format!("call-{agent_id}"),
            }
        }

        fn cleanup(self) {
            drop(self.hook);
            drop(self.coordinator);
            std::fs::remove_dir_all(self.repository).ok();
        }
    }

    fn test_run_spec(message: &str) -> AgentRunSpec {
        let mut provider = pl_model::ProviderInfo::openai(Some("http://example.invalid".into()));
        provider.default_model = "test-model".to_string();
        AgentRunSpec {
            provider: pl_model::create_provider(provider).unwrap(),
            reasoning_effort: None,
            config: None,
            mcp_runtime: None,
            lsp_runtime: None,
            workspace_instructions: None,
            instruction_snapshot: None,
            tool_registrar: None,
            workspace_root: PathBuf::from("."),
            options: TurnOptions::default(),
            event_tx: tokio::sync::broadcast::channel(8).0,
            call_id: "call-install".to_string(),
            message: message.to_string(),
            mode: CompileMode::Task,
            budget: TurnBudget::default(),
            initial_session: crate::CoreSession::new(),
        }
    }

    fn init_repository(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pure-task-spawn-{name}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        git(&path, &["init"]);
        git(&path, &["checkout", "-b", "main"]);
        git(&path, &["config", "user.email", "pure@example.invalid"]);
        git(&path, &["config", "user.name", "Pure Test"]);
        std::fs::write(path.join("README.md"), "initial\n").unwrap();
        git(&path, &["add", "README.md"]);
        git(&path, &["commit", "-m", "initial"]);
        path
    }

    fn git(repository: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output(repository: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
