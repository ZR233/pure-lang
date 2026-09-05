//! `StudioAgentTurnFactory` 的定义与构造。

use std::sync::Arc;

use crate::McpRuntimeHandle;
use crate::config::ConfigRuntime;
use crate::studio::product_event_bus::ProductEventBus;
use crate::studio::runtime::SkillCatalogRuntime;
use crate::studio::{InteractionService, StudioStore};

use super::super::resources::StudioAgentResources;

/// 使用 Studio 配置、project/session 和产品工具准备一次 framework turn。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentTurnFactory {
    pub(super) store: StudioStore,
    pub(super) product_events: ProductEventBus,
    pub(super) config_runtime: ConfigRuntime,
    pub(super) mcp_runtime: McpRuntimeHandle,
    pub(super) tool_manager: pl_core::ToolManager,
    pub(super) lsp_runtime: pl_lsp::runtime::LspRuntimeRegistry,
    pub(super) interactions: InteractionService,
    pub(super) resources: StudioAgentResources,
    pub(super) skills: SkillCatalogRuntime,
    pub(super) thread_modes: pl_core::ThreadModeManager,
    pub(super) ssh_manager: Arc<pl_core::remote::SshManager>,
}

impl StudioAgentTurnFactory {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::studio) fn new(
        store: StudioStore,
        product_events: ProductEventBus,
        config_runtime: ConfigRuntime,
        mcp_runtime: McpRuntimeHandle,
        tool_manager: pl_core::ToolManager,
        lsp_runtime: pl_lsp::runtime::LspRuntimeRegistry,
        interactions: InteractionService,
        resources: StudioAgentResources,
        skills: SkillCatalogRuntime,
        thread_modes: pl_core::ThreadModeManager,
        ssh_manager: Arc<pl_core::remote::SshManager>,
    ) -> Self {
        Self {
            store,
            product_events,
            config_runtime,
            mcp_runtime,
            tool_manager,
            lsp_runtime,
            interactions,
            resources,
            skills,
            thread_modes,
            ssh_manager,
        }
    }
}
