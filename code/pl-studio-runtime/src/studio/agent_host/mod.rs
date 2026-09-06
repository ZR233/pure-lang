mod events;
mod lifecycle;
mod policy;
mod repository;
mod resources;
mod turn_factory;
mod workspace_resolver;
pub(in crate::studio) mod worktree_lease;

use std::sync::Arc;

use pl_core::{AgentRuntimeHost, AgentRuntimeOptions};

use crate::McpRuntimeHandle;
use crate::config::ConfigRuntime;
use crate::studio::runtime::SkillCatalogRuntime;
use crate::studio::{InteractionService, ProductEventBus, StudioStore};

use events::StudioAgentCommitObserver;
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
        worktrees: worktree_lease::WorktreeLeaseOwner,
        store: StudioStore,
        config_runtime: ConfigRuntime,
        mcp_runtime: McpRuntimeHandle,
        tool_manager: pl_core::ToolManager,
        lsp_runtime: pl_lsp::runtime::LspRuntimeRegistry,
        interactions: InteractionService,
        resources: StudioAgentResources,
        product_events: ProductEventBus,
        skills: SkillCatalogRuntime,
        thread_modes: pl_core::ThreadModeManager,
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
                resources.clone(),
                skills,
                thread_modes,
                ssh_manager.clone(),
            ),
            lifecycle: StudioAgentLifecycle::new(
                worktrees,
                product_events.clone(),
                resources.clone(),
                ssh_manager,
            ),
            observer: StudioAgentCommitObserver::new(resources, product_events),
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
