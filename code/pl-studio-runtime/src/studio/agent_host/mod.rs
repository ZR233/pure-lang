mod events;
mod lifecycle;
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
pub(in crate::studio) use repository::{StudioAgentRepository, ThreadWriteBehindWriter};
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
        repository: StudioAgentRepository,
        store: StudioStore,
        config_runtime: ConfigRuntime,
        mcp_runtime: McpRuntimeHandle,
        tool_manager: pl_core::ToolManager,
        lsp_runtime: pl_lsp::LspRuntimeRegistry,
        interactions: InteractionService,
        coordinator: Arc<TaskCoordinator>,
        resources: StudioAgentResources,
        product_events: ProductEventBus,
        skills: SkillCatalogRuntime,
        ssh_manager: Arc<pl_core::remote::SshManager>,
    ) -> Self {
        Self {
            repository,
            turn_factory: StudioAgentTurnFactory::new(
                store.clone(),
                product_events.clone(),
                config_runtime,
                mcp_runtime,
                tool_manager,
                lsp_runtime,
                interactions.clone(),
                coordinator.clone(),
                resources.clone(),
                skills,
                ssh_manager.clone(),
            ),
            lifecycle: StudioAgentLifecycle::new(
                store.clone(),
                product_events.clone(),
                coordinator.clone(),
                resources.clone(),
                ssh_manager,
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
