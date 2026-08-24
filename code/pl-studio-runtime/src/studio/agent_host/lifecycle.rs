use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pl_core::{
    AgentLifecycleAdapter, CloseLifecycleRequest, SpawnLifecycleRequest, SpawnRollbackPhase,
    SpawnRollbackReason,
};

use crate::{PureError, Result, WorktreeError, WorktreeHandle, WorktreeManager};

use crate::studio::product_event_bus::ProductEventBus;
use crate::studio::records::ThreadRecord;
use crate::studio::store::directory::RegisteredChildThread;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::task_coordinator::{
    ExecutorCloseDisposition, OperationalTaskSpawnFailure, StudioSpawnIntent,
    StudioTaskSpawnPreparation, StudioTaskSpawnRequest, TaskSpawnCompensation,
    TaskSpawnCompensationState, TaskSpawnFailure, TaskSpawnFailureCode, TaskSpawnFailurePhase,
};
use crate::studio::{StudioStore, UnregisteredThreadFault};

use super::resources::{StudioAgentResource, StudioAgentResources};

#[derive(Clone)]
pub(in crate::studio) struct StudioAgentLifecycle {
    store: StudioStore,
    product_events: ProductEventBus,
    coordinator: Arc<TaskCoordinator>,
    resources: StudioAgentResources,
}

pub(in crate::studio) struct StudioSpawnLease {
    agent_id: pl_core::ThreadId,
    resource: StudioAgentResource,
}

pub(in crate::studio) struct StudioCloseLease {
    resource: Option<StudioAgentResource>,
    disposition: ExecutorCloseDisposition,
    durable_discard_committed: AtomicBool,
}

impl StudioAgentLifecycle {
    pub(super) fn new(
        store: StudioStore,
        product_events: ProductEventBus,
        coordinator: Arc<TaskCoordinator>,
        resources: StudioAgentResources,
    ) -> Self {
        Self {
            store,
            product_events,
            coordinator,
            resources,
        }
    }
}

impl AgentLifecycleAdapter for StudioAgentLifecycle {
    type Error = PureError;
    type SpawnLease = StudioSpawnLease;
    type CloseLease = StudioCloseLease;

    async fn prepare_spawn(&self, request: SpawnLifecycleRequest) -> Result<Self::SpawnLease> {
        let intent = StudioSpawnIntent::parse(request.metadata.clone())
            .map_err(|error| lifecycle_error(error.to_string()))?;
        let role = request.child.identity.role.as_str();
        intent
            .validate_role(role)
            .map_err(|error| lifecycle_error(error.to_string()))?;
        let parent_thread_id = self
            .resources
            .thread_id(&request.parent.identity.id)
            .await
            .or_else(|| intent.studio_thread_id.clone())
            .ok_or_else(|| lifecycle_error("spawn has no Studio Thread boundary"))?;
        let parent_thread = match self.product_events.thread_snapshot(&parent_thread_id) {
            Some(thread) => ThreadRecord::from_directory_thread(thread),
            None => self
                .store
                .read_thread(&parent_thread_id)
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?
                .ok_or_else(|| lifecycle_error("spawn parent Studio Thread does not exist"))?,
        };
        let root_thread_id = parent_thread.root_thread_id.clone();
        if intent.spawn_kind.is_some()
            && intent.studio_thread_id.as_deref() != Some(root_thread_id.as_str())
        {
            return Err(lifecycle_error(
                "Task spawn intent does not match the parent Studio Thread",
            ));
        }
        let child_thread_id = request.child_thread_id.to_string();
        let task_name = intent.task_name(role);
        let studio_request = StudioTaskSpawnRequest {
            agent_id: request.child.identity.id.to_string(),
            root_thread_id: root_thread_id.clone(),
            task_name: task_name.clone(),
            role: role.to_string(),
            scope_hints: intent.scope_hints.clone(),
            requested_by_call_id: intent.requesting_tool_call_id(),
            review_round_id: intent.review_round_id.clone(),
            blueprint: intent.blueprint.clone(),
        };
        let mut preparation = self
            .coordinator
            .prepare_agent_spawn(&studio_request)
            .await?;
        let worktree = match create_worktree(&preparation, &studio_request).await {
            Ok(worktree) => worktree,
            Err(error) => {
                let failure = worktree_spawn_failure(&studio_request, &preparation, &error);
                return Err(record_prepared_spawn_failure(
                    &self.coordinator,
                    &studio_request,
                    &preparation,
                    failure,
                )
                .await);
            }
        };
        if let Some((manager, handle, actual_base_commit)) = &worktree
            && let Err(error) = self
                .coordinator
                .finalize_executor_worktree(&studio_request, &mut preparation, actual_base_commit)
                .await
        {
            let operation = WorktreeError::Io(format!(
                "failed to persist executor worktree base and handoff: {error}"
            ));
            let failure_error = compensate_created_worktree(manager, handle, operation).await;
            let failure = worktree_spawn_failure(&studio_request, &preparation, &failure_error);
            return Err(record_prepared_spawn_failure(
                &self.coordinator,
                &studio_request,
                &preparation,
                failure,
            )
            .await);
        }
        // child Thread 注册走目录通道：热集合先行，durable delta FIFO 先于该
        // child 的首个 state commit 落库；SQLite 失败只影响持久化健康状态。
        if let Err(error) = self
            .product_events
            .register_child_thread(RegisteredChildThread {
                id: child_thread_id.clone(),
                parent_thread_id: parent_thread_id.clone(),
                agent_path: request.child.identity.id.to_string(),
                project_id: parent_thread.project_id.clone(),
                root_thread_id: root_thread_id.clone(),
                mode: parent_thread.mode.into(),
                role: request.child.identity.role.to_string(),
                title: task_name.clone(),
            })
            .await
        {
            let failure = operational_spawn_failure(
                &studio_request,
                &preparation,
                TaskSpawnFailureCode::ChildThreadCreate,
                TaskSpawnFailurePhase::ChildThreadCreate,
                format!("failed to create Studio child Thread: {error}"),
                worktree.is_some(),
                TaskSpawnCompensationState::NotCreated,
            );
            return Err(rollback_prepared_spawn(
                &self.coordinator,
                &studio_request,
                &preparation,
                worktree
                    .as_ref()
                    .map(|(manager, handle, _)| (manager, handle)),
                failure,
            )
            .await);
        }
        Ok(StudioSpawnLease {
            agent_id: request.child.identity.id,
            resource: StudioAgentResource {
                thread_id: child_thread_id,
                task_name,
                request: studio_request,
                preparation,
                worktree: worktree.map(|(manager, handle, _)| (manager, handle)),
            },
        })
    }

    fn initial_context(
        &self,
        lease: &Self::SpawnLease,
    ) -> Result<Vec<pl_core::PinnedContextSection>> {
        Ok(lease.resource.preparation.initial_context().to_vec())
    }

    async fn activate_spawn(&self, lease: &Self::SpawnLease) -> Result<()> {
        self.coordinator
            .activate_agent_spawn(&lease.resource.request, &lease.resource.preparation)
            .await?;
        self.resources
            .insert(lease.agent_id.clone(), lease.resource.clone())
            .await;
        Ok(())
    }

    async fn rollback_spawn(
        &self,
        lease: Self::SpawnLease,
        reason: SpawnRollbackReason,
    ) -> Result<()> {
        self.resources.remove(&lease.agent_id).await;
        let mut failures = Vec::new();
        let (code, phase) = match reason.phase {
            SpawnRollbackPhase::InitialContext | SpawnRollbackPhase::AgentRegistration => (
                TaskSpawnFailureCode::AgentRegistration,
                TaskSpawnFailurePhase::AgentRegistration,
            ),
            SpawnRollbackPhase::Activation => (
                TaskSpawnFailureCode::Activation,
                TaskSpawnFailurePhase::Activation,
            ),
        };
        let mut failure = operational_spawn_failure(
            &lease.resource.request,
            &lease.resource.preparation,
            code,
            phase,
            reason.message.clone(),
            lease.resource.worktree.is_some(),
            TaskSpawnCompensationState::Unknown,
        );
        if let Some((manager, handle)) = &lease.resource.worktree {
            match manager.discard(handle).await {
                Ok(()) => {
                    failure.compensation.worktree = TaskSpawnCompensationState::Removed;
                }
                Err(error) => {
                    failure.compensation.worktree = TaskSpawnCompensationState::CleanupFailed;
                    failure.cause = error.cause();
                    failures.push(format!("worktree cleanup failed: {error}"));
                }
            }
        }
        match self
            .store
            .fault_unregistered_child_thread(&lease.resource.thread_id, &reason.message)
            .await
        {
            Ok(UnregisteredThreadFault::Faulted) => {
                failure.compensation.child_thread = TaskSpawnCompensationState::MarkedFailed;
            }
            Ok(UnregisteredThreadFault::RuntimeOwned) => {
                // repository commit 已建立 runtime revision；core 会在 rollback 返回后
                // 通过同一个 AgentState 状态机提交 Closed/Faulted compensation。
            }
            Err(error) => {
                failure.compensation.child_thread = TaskSpawnCompensationState::Faulted;
                failures.push(format!("child Thread fault compensation failed: {error}"));
            }
        }
        if let Err(error) = self
            .coordinator
            .rollback_agent_spawn(
                &lease.resource.request,
                &lease.resource.preparation,
                failure,
            )
            .await
        {
            failures.push(format!("spawn failure persistence failed: {error}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(lifecycle_error(failures.join("; ")))
        }
    }

    async fn prepare_close(&self, request: CloseLifecycleRequest) -> Result<Self::CloseLease> {
        let resource = self.resources.get(&request.agent.identity.id).await;
        let disposition = match resource.as_ref() {
            Some(resource) => {
                self.coordinator
                    .prepare_agent_close(&resource.request, &resource.preparation)
                    .await?
            }
            None => ExecutorCloseDisposition::Discard,
        };
        Ok(StudioCloseLease {
            resource,
            disposition,
            durable_discard_committed: AtomicBool::new(false),
        })
    }

    async fn commit_close(&self, lease: &Self::CloseLease) -> Result<()> {
        let Some(resource) = &lease.resource else {
            return Ok(());
        };
        let disposition = self
            .coordinator
            .commit_agent_close(&resource.request, &resource.preparation)
            .await?;
        if disposition != lease.disposition {
            return Err(lifecycle_error(
                "Studio agent close disposition changed after preflight",
            ));
        }
        if disposition == ExecutorCloseDisposition::Discard && resource.request.role == "executor" {
            lease
                .durable_discard_committed
                .store(true, Ordering::Release);
        }
        if disposition == ExecutorCloseDisposition::Discard
            && let Some((manager, handle)) = &resource.worktree
        {
            manager
                .discard(handle)
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?;
        }
        Ok(())
    }

    async fn rollback_close(&self, lease: Self::CloseLease) -> Result<()> {
        if lease.durable_discard_committed.load(Ordering::Acquire) {
            return Err(lifecycle_error(
                "Studio agent close cannot restore a committed executor discard",
            ));
        }
        Ok(())
    }
}

async fn rollback_prepared_spawn(
    coordinator: &TaskCoordinator,
    request: &StudioTaskSpawnRequest,
    preparation: &StudioTaskSpawnPreparation,
    worktree: Option<(&WorktreeManager, &WorktreeHandle)>,
    mut failure: TaskSpawnFailure,
) -> PureError {
    let mut failures = vec![failure.message.clone()];
    if let Some((manager, handle)) = worktree {
        match manager.discard(handle).await {
            Ok(()) => failure.compensation.worktree = TaskSpawnCompensationState::Removed,
            Err(error) => {
                failure.compensation.worktree = TaskSpawnCompensationState::CleanupFailed;
                failure.cause = error.cause();
                failures.push(format!("worktree cleanup failed: {error}"));
            }
        }
    }
    if let Err(error) = coordinator
        .rollback_agent_spawn(request, preparation, failure)
        .await
    {
        failures.push(format!("spawn failure persistence failed: {error}"));
    }
    lifecycle_error(failures.join("; "))
}

async fn record_prepared_spawn_failure(
    coordinator: &TaskCoordinator,
    request: &StudioTaskSpawnRequest,
    preparation: &StudioTaskSpawnPreparation,
    failure: TaskSpawnFailure,
) -> PureError {
    let message = failure.message.clone();
    match coordinator
        .rollback_agent_spawn(request, preparation, failure)
        .await
    {
        Ok(()) => lifecycle_error(message),
        Err(error) => lifecycle_error(format!(
            "{message}; spawn failure persistence failed: {error}"
        )),
    }
}

async fn create_worktree(
    preparation: &StudioTaskSpawnPreparation,
    _request: &StudioTaskSpawnRequest,
) -> std::result::Result<Option<(WorktreeManager, WorktreeHandle, String)>, WorktreeError> {
    let Some(spec) = preparation.worktree_spec().cloned() else {
        return Ok(None);
    };
    let manager = WorktreeManager::local(spec.repo_root.clone());
    let handle = manager.create_from_spec(spec).await?;
    let actual_base_commit = match manager.resolve_head(&handle).await {
        Ok(commit) => commit,
        Err(operation) => {
            return Err(compensate_created_worktree(&manager, &handle, operation).await);
        }
    };
    Ok(Some((manager, handle, actual_base_commit)))
}

async fn compensate_created_worktree(
    manager: &WorktreeManager,
    handle: &WorktreeHandle,
    operation: WorktreeError,
) -> WorktreeError {
    match manager.discard(handle).await {
        Ok(()) => WorktreeError::OperationFailedAfterCleanup {
            operation: Box::new(operation),
        },
        Err(cleanup) => WorktreeError::OperationFailedWithCleanup {
            operation: Box::new(operation),
            cleanup: Box::new(cleanup),
        },
    }
}

fn worktree_spawn_failure(
    request: &StudioTaskSpawnRequest,
    preparation: &StudioTaskSpawnPreparation,
    error: &WorktreeError,
) -> TaskSpawnFailure {
    TaskSpawnFailure::worktree(
        preparation.task_run_id().unwrap_or_default().to_string(),
        preparation
            .lifecycle_token()
            .unwrap_or_default()
            .to_string(),
        request.agent_id.clone(),
        preparation.spawn_resource().unwrap_or_else(|| {
            crate::studio::task_coordinator::TaskSpawnResource {
                repo_root: String::new(),
                path: String::new(),
                branch: String::new(),
                base_ref: "HEAD".to_string(),
            }
        }),
        error,
    )
}

fn operational_spawn_failure(
    request: &StudioTaskSpawnRequest,
    preparation: &StudioTaskSpawnPreparation,
    code: TaskSpawnFailureCode,
    phase: TaskSpawnFailurePhase,
    message: String,
    has_worktree: bool,
    child_thread: TaskSpawnCompensationState,
) -> TaskSpawnFailure {
    TaskSpawnFailure::operational(OperationalTaskSpawnFailure {
        code,
        phase,
        message,
        task_run_id: preparation.task_run_id().map(str::to_string),
        work_unit_id: preparation.lifecycle_token().map(str::to_string),
        agent_id: request.agent_id.clone(),
        resource: preparation.spawn_resource(),
        compensation: TaskSpawnCompensation {
            allocation: TaskSpawnCompensationState::MarkedFailed,
            worktree: if has_worktree {
                TaskSpawnCompensationState::Unknown
            } else {
                TaskSpawnCompensationState::NotCreated
            },
            child_thread,
        },
    })
}

fn lifecycle_error(error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "studio_agent_lifecycle".to_string(),
        error: error.into(),
    }
}
