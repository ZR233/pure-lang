use std::path::PathBuf;

use crate::config::ToolCapabilityConfig;
use crate::tool::{
    ApplyPatchTool, AskUserTool, CloseAgentTool, CopyPathTool, CreateDirectoryTool, DeletePathTool,
    FollowupTaskTool, ListAgentsTool, ListFilesTool, MovePathTool, PlanExitTool, ReadFileTool,
    SearchFilesTool, SendMessageTool, SpawnAgentTool, StatPathTool, WaitAgentTool, WriteFileTool,
    command_tool_pair,
};

use super::PureCore;

/// 按能力开关组装 pl-core 的共享工具集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSetBuilder {
    capabilities: ToolCapabilityConfig,
}

impl Default for ToolSetBuilder {
    fn default() -> Self {
        Self {
            capabilities: ToolCapabilityConfig::default(),
        }
    }
}

impl ToolSetBuilder {
    pub fn from_capabilities(capabilities: ToolCapabilityConfig) -> Self {
        Self { capabilities }
    }

    pub fn capabilities(&self) -> &ToolCapabilityConfig {
        &self.capabilities
    }

    pub async fn register(
        &self,
        core: &mut PureCore,
        workspace_root: impl Into<PathBuf>,
        workspace_instructions: Option<String>,
    ) {
        let workspace_root = workspace_root.into();
        core.workspace_root = Some(workspace_root.clone());
        core.workspace_instructions = workspace_instructions.clone();

        if self.capabilities.skills {
            core.register_skill_tools_for_workspace(
                workspace_root.clone(),
                workspace_instructions.clone(),
            );
        }
        if self.capabilities.bash {
            let (bash_tool, write_stdin_tool) = command_tool_pair(workspace_root.clone());
            core.register_tool(bash_tool);
            core.register_tool(write_stdin_tool);
        }
        if self.capabilities.workspace_files {
            register_file_tools(core);
        }
        if self.capabilities.lsp {
            if let Some(registry) = core.lsp_runtime.clone() {
                core.tools.register_lsp_languages(&registry).await;
            }
        }
        if self.capabilities.subagents {
            register_subagent_tools(core, workspace_instructions.clone());
        }
        if self.capabilities.ask_user {
            core.register_tool(AskUserTool);
        }
        core.register_tool(PlanExitTool);
    }
}

fn register_file_tools(core: &mut PureCore) {
    core.register_tool(ReadFileTool::new());
    core.register_tool(WriteFileTool);
    core.register_tool(ListFilesTool);
    core.register_tool(SearchFilesTool);
    core.register_tool(StatPathTool);
    core.register_tool(CreateDirectoryTool);
    core.register_tool(DeletePathTool);
    core.register_tool(CopyPathTool);
    core.register_tool(MovePathTool);
    core.register_tool(ApplyPatchTool);
}

fn register_subagent_tools(core: &mut PureCore, workspace_instructions: Option<String>) {
    core.register_tool(SpawnAgentTool::new(
        core.provider.clone(),
        core.reasoning_effort.clone(),
        core.config.clone(),
        core.mcp_runtime.clone(),
        core.lsp_runtime.clone(),
        workspace_instructions.clone(),
    ));
    core.register_tool(WaitAgentTool);
    core.register_tool(ListAgentsTool);
    core.register_tool(SendMessageTool);
    core.register_tool(FollowupTaskTool::new(
        core.provider.clone(),
        core.reasoning_effort.clone(),
        core.config.clone(),
        core.mcp_runtime.clone(),
        core.lsp_runtime.clone(),
        workspace_instructions,
    ));
    core.register_tool(CloseAgentTool);
}
