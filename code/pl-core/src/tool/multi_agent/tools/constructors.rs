use crate::tool::ToolContext;
use crate::turn::TurnBudget;

use super::super::types::{AgentToolRuntime, SendInputTool, SpawnAgentTool};

impl SpawnAgentTool {
    pub fn new(
        provider: pl_model::SharedModelProvider,
        reasoning_effort: Option<crate::config::ReasoningEffort>,
        config: Option<crate::config::PureConfig>,
        mcp_runtime: Option<crate::mcp::McpRuntimeRegistry>,
        lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
        workspace_instructions: Option<String>,
    ) -> Self {
        Self {
            runtime: AgentToolRuntime::new(
                provider,
                reasoning_effort,
                config,
                mcp_runtime,
                lsp_runtime,
                workspace_instructions,
            ),
        }
    }
}

impl SendInputTool {
    pub fn new(
        provider: pl_model::SharedModelProvider,
        reasoning_effort: Option<crate::config::ReasoningEffort>,
        config: Option<crate::config::PureConfig>,
        mcp_runtime: Option<crate::mcp::McpRuntimeRegistry>,
        lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
        workspace_instructions: Option<String>,
    ) -> Self {
        Self {
            runtime: AgentToolRuntime::new(
                provider,
                reasoning_effort,
                config,
                mcp_runtime,
                lsp_runtime,
                workspace_instructions,
            ),
        }
    }
}

impl AgentToolRuntime {
    pub(in crate::tool::multi_agent) fn run_config(
        &self,
        context: &ToolContext,
        options: crate::TurnOptions,
        call_id: String,
        message: String,
        initial_session: crate::CoreSession,
    ) -> crate::AgentRunSpec {
        crate::AgentRunSpec {
            provider: self.provider.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            config: self.config.clone(),
            mcp_runtime: self.mcp_runtime.clone(),
            lsp_runtime: self.lsp_runtime.clone(),
            workspace_instructions: context
                .workspace_instructions
                .clone()
                .or_else(|| self.workspace_instructions.clone()),
            instruction_snapshot: context.instruction_snapshot.clone(),
            tool_registrar: context.agent_tool_registrar.clone(),
            workspace_root: context.workspace_root.clone(),
            options,
            event_tx: context.event_tx.clone(),
            call_id,
            message,
            mode: context.mode,
            budget: TurnBudget::child_default(),
            initial_session,
        }
    }
}
