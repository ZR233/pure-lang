//! Product configuration and shared service leases used to construct core Threads.
mod child_resources;
mod errors;
mod restore_child;
mod root_resources;
mod thread_seed;
mod thread_tools;
mod tool_bindings;

use crate::studio::{ProductEventBus, StudioStore};
use std::sync::Arc;

/// Existing service owners, shared by lease; no mutable Thread/session state lives here.
#[derive(Clone)]
pub(in crate::studio) struct StudioThreadServices {
    pub store: StudioStore,
    pub worktrees: crate::studio::agent_host::worktree_lease::WorktreeLeaseOwner,
    pub model_performance: crate::studio::runtime::ModelPerformanceOwner,
    pub product_events: ProductEventBus,
    pub config_runtime: crate::config::ConfigRuntime,
    pub mcp_runtime: pl_tool::mcp::McpRuntimeHandle,
    pub lsp_runtime: pl_lsp::runtime::LspRuntimeRegistry,
    pub skills: crate::studio::runtime::SkillCatalogRuntime,
    pub thread_modes: crate::mode::ThreadModeManager,
    pub ssh_manager: Arc<pl_tool::remote::SshManager>,
}

/// Assembles fresh model and tool instances from product inputs, without the old Turn engine.
#[derive(Clone)]
pub(in crate::studio) struct StudioThreadFactory {
    services: StudioThreadServices,
    bindings: Arc<std::sync::Mutex<std::collections::BTreeMap<String, tool_bindings::ToolBinding>>>,
}
impl StudioThreadFactory {
    pub(in crate::studio) fn new(services: StudioThreadServices) -> Self {
        Self {
            services,
            bindings: Default::default(),
        }
    }
}
impl std::fmt::Debug for StudioThreadFactory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StudioThreadFactory")
            .finish_non_exhaustive()
    }
}

impl crate::thread_assembler::StudioActivationFactory for StudioThreadFactory {
    async fn prepare(
        &self,
        request: crate::thread_assembler::ThreadPreparation,
    ) -> Result<
        crate::thread_assembler::StudioThreadSpec,
        crate::thread_assembler::ThreadAssemblyError,
    > {
        if request.identity.parent_id.is_some() {
            self.prepare_restored_child(request).await
        } else {
            self.prepare_root_thread(&request.identity.id, request.cancellation)
                .await
        }
    }
}

impl StudioThreadFactory {
    async fn thread_record(
        &self,
        id: &str,
    ) -> Result<crate::studio::ThreadRecord, crate::thread_assembler::ThreadAssemblyError> {
        if let Some(record) = self.services.product_events.thread_snapshot(id) {
            return Ok(crate::studio::ThreadRecord::from_directory_thread(record));
        }
        self.services
            .store
            .read_thread(id)
            .await
            .map_err(|error| errors::resource_error("read Thread association", error))?
            .ok_or_else(|| crate::thread_assembler::ThreadAssemblyError::Identity(id.into()))
    }
    async fn project_record(
        &self,
        id: &str,
    ) -> Result<crate::studio::ProjectRecord, crate::thread_assembler::ThreadAssemblyError> {
        if let Some(record) = self
            .services
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|record| record.id == id)
        {
            return Ok(record);
        }
        self.services
            .store
            .read_project(id)
            .await
            .map_err(|error| errors::resource_error("read project association", error))?
            .ok_or_else(|| crate::thread_assembler::ThreadAssemblyError::Identity(id.into()))
    }
}
