//! LSP 工具组的按需构建。

pub(super) fn lsp_tool_group(
    available: bool,
    registry: pl_lsp::runtime::LspRuntimeRegistry,
    workspace: pl_core::ToolWorkspace,
) -> Vec<pl_core::DynTool> {
    if available {
        pl_core::lsp_tools(registry, workspace)
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{ToolGroupId, ToolInstallGroup};

    #[test]
    fn lsp_availability_replaces_the_agent_group_without_stale_tools() {
        let manager = pl_core::ToolManager::new();
        let tools = manager.agent_tool_set("lsp-switch", pl_core::GlobalToolInheritance::Isolated);
        let registry = pl_lsp::runtime::LspRuntimeRegistry::new();
        let workspace =
            pl_core::ToolWorkspace::new(pl_core::AgentWorkspace::local(std::env::temp_dir()));

        tools
            .install(ToolInstallGroup::direct(
                ToolGroupId::new("lsp"),
                lsp_tool_group(true, registry.clone(), workspace.clone()),
            ))
            .expect("publish available LSP tools");
        assert_eq!(
            tools.tool_names(),
            vec!["lsp_capabilities".to_string(), "lsp_query".to_string()]
        );

        tools
            .install(ToolInstallGroup::direct(
                ToolGroupId::new("lsp"),
                lsp_tool_group(false, registry, workspace),
            ))
            .expect("publish unavailable LSP generation");
        assert!(tools.tool_names().is_empty());
    }
}
