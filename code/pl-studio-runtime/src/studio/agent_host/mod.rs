mod continuation;
mod events;
mod lifecycle;
mod policy;
mod repository;
mod resources;
mod turn_factory;

use std::sync::Arc;

use pl_core::{AgentRuntimeHost, AgentRuntimeOptions};

use crate::McpRuntimeHandle;
use crate::config::ConfigStore;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{
    InteractionRuntime, StudioProductEventRuntime, StudioRuntimeState, StudioStore,
};

pub(super) use continuation::{StudioContinuationReason, StudioContinuationService};
use events::StudioAgentCommitObserver;
use lifecycle::StudioAgentLifecycle;
use repository::StudioAgentRepository;
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
        config_store: ConfigStore,
        mcp_runtime: McpRuntimeHandle,
        lsp_runtime: pl_lsp::LspRuntimeRegistry,
        interactions: InteractionRuntime,
        runtime_state: StudioRuntimeState,
        continuations: StudioContinuationService,
        coordinator: Arc<TaskCoordinator>,
        resources: StudioAgentResources,
        product_events: StudioProductEventRuntime,
    ) -> Self {
        Self {
            repository: StudioAgentRepository::new(store.clone()),
            turn_factory: StudioAgentTurnFactory::new(
                store.clone(),
                config_store,
                mcp_runtime,
                lsp_runtime,
                interactions.clone(),
                coordinator.clone(),
                resources.clone(),
            ),
            lifecycle: StudioAgentLifecycle::new(coordinator, resources.clone()),
            observer: StudioAgentCommitObserver::new(
                store,
                interactions,
                runtime_state,
                resources,
                continuations,
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
