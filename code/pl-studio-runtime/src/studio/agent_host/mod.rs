mod events;
mod lifecycle;
mod plan_confirmation;
mod policy;
mod repository;
mod resources;
mod turn_factory;
mod workspace_resolver;

use std::sync::Arc;

use pl_core::{AgentRuntimeHost, AgentRuntimeOptions};

use crate::McpRuntimeHandle;
use crate::config::ConfigRuntime;
use crate::studio::runtime::SkillCatalogRuntime;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{InteractionService, ProductEventBus, StudioStore};

use events::StudioAgentCommitObserver;
pub(in crate::studio) use events::{
    materialize_pending_task_planner_wakes, materialize_task_planner_wake,
};
use lifecycle::StudioAgentLifecycle;
use plan_confirmation::StudioPlanConfirmationProjector;
pub(in crate::studio) use repository::StudioAgentRepository;
pub(super) use resources::{StudioAgentResources, root_agent_id};
use turn_factory::StudioAgentTurnFactory;

/// Studio 对 framework repository、turn factory、lifecycle 和 event sink 的聚合。
#[derive(Clone)]
pub(super) struct StudioAgentHost {
    repository: StudioAgentRepository,
    turn_factory: StudioAgentTurnFactory,
    lifecycle: StudioAgentLifecycle,
    observer: StudioAgentCommitObserver,
}

impl StudioAgentHost {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        store: StudioStore,
        config_runtime: ConfigRuntime,
        mcp_runtime: McpRuntimeHandle,
        mcp_shared_tools: std::sync::Arc<pl_core::ToolRegistry>,
        lsp_runtime: pl_lsp::LspRuntimeRegistry,
        interactions: InteractionService,
        coordinator: Arc<TaskCoordinator>,
        resources: StudioAgentResources,
        product_events: ProductEventBus,
        skills: SkillCatalogRuntime,
    ) -> Self {
        Self {
            repository: StudioAgentRepository::new(store.clone()),
            turn_factory: StudioAgentTurnFactory::new(
                store.clone(),
                config_runtime,
                mcp_runtime,
                mcp_shared_tools,
                lsp_runtime,
                interactions.clone(),
                coordinator.clone(),
                resources.clone(),
                skills,
            ),
            lifecycle: StudioAgentLifecycle::new(
                store.clone(),
                coordinator.clone(),
                resources.clone(),
            ),
            observer: StudioAgentCommitObserver::new(
                store,
                interactions,
                coordinator,
                resources,
                product_events,
            ),
        }
    }

    pub(super) async fn attach_runtime(&self, runtime: pl_core::AgentRuntimeHandle) {
        self.observer.attach_runtime(runtime).await;
    }

    pub(super) async fn detach_runtime(&self) {
        self.observer.detach_runtime().await;
    }

    /// 暴露内部 repository（含 write-behind writer）句柄，供关机排空使用。
    pub(super) fn persistence(&self) -> StudioAgentRepository {
        self.repository.clone()
    }
}

async fn wait_for_runtime(
    mut runtime: tokio::sync::watch::Receiver<Option<pl_core::AgentRuntimeHandle>>,
) -> anyhow::Result<pl_core::AgentRuntimeHandle> {
    loop {
        if let Some(runtime) = runtime.borrow_and_update().clone() {
            return Ok(runtime);
        }
        runtime
            .changed()
            .await
            .map_err(|_| anyhow::anyhow!("Studio agent runtime attachment channel closed"))?;
    }
}

impl AgentRuntimeHost for StudioAgentHost {
    type Error = crate::PureError;
    type Repository = StudioAgentRepository;
    type TurnFactory = StudioAgentTurnFactory;
    type Lifecycle = StudioAgentLifecycle;
    type Observer = StudioAgentCommitObserver;

    fn repository(&self) -> &Self::Repository {
        &self.repository
    }

    fn turn_factory(&self) -> &Self::TurnFactory {
        &self.turn_factory
    }

    fn lifecycle(&self) -> &Self::Lifecycle {
        &self.lifecycle
    }

    fn observer(&self) -> &Self::Observer {
        &self.observer
    }
}

pub(super) type StudioAgentRuntime = pl_core::AgentRuntime<StudioAgentHost>;

pub(super) fn runtime_options() -> AgentRuntimeOptions {
    AgentRuntimeOptions {
        restored_inputs: pl_core::RestoredInputPolicy::Hold,
        ..AgentRuntimeOptions::default()
    }
}
