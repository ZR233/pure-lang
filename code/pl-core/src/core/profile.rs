use std::path::PathBuf;

use pl_model::{ProviderInfo, SharedModelProvider, create_provider, create_provider_with_models};
use pl_protocol::Result;

use crate::config::{PureConfig, ReasoningEffort};
use crate::instruction::InstructionProfile;
use crate::turn::TurnOptions;

use super::PureCore;

/// 工具注册策略。
///
/// 该枚举只描述 `pl-core` 是否应该自动注册默认工具。具体工具执行环境
/// 仍由工具实现自身决定，宿主可以在 `HostProvided` 模式下注册自己的工具。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolProfile {
    #[default]
    Minimal,
    LocalWorkspace,
    HostProvided,
}

/// workspace 运行边界配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceProfile {
    pub root: Option<PathBuf>,
    pub instructions: Option<String>,
}

impl WorkspaceProfile {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
            instructions: None,
        }
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }
}

/// Agent 运行后端。
///
/// 默认使用 `pl-core` 内存 supervisor。宿主需要复用自己维护的 agent 树时，
/// 可以传入已有 `AgentSupervisor`，避免在 core 外再套一层 agent 状态机。
#[derive(Debug, Clone)]
pub struct AgentBackendProfile {
    supervisor: crate::AgentSupervisor,
}

impl AgentBackendProfile {
    pub fn in_memory() -> Self {
        Self {
            supervisor: crate::AgentSupervisor::default(),
        }
    }

    pub fn from_supervisor(supervisor: crate::AgentSupervisor) -> Self {
        Self { supervisor }
    }

    pub(crate) fn into_supervisor(self) -> crate::AgentSupervisor {
        self.supervisor
    }
}

impl Default for AgentBackendProfile {
    fn default() -> Self {
        Self::in_memory()
    }
}

/// Core 运行选项。
///
/// 这些选项作为初始化级默认值使用；调用 `run_turn_with_options` 时仍可用
/// turn 级 options 完整覆盖。
#[derive(Debug, Clone, Default)]
pub struct CoreRuntimeOptions {
    pub default_turn_options: TurnOptions,
}

impl CoreRuntimeOptions {
    pub fn with_turn_options(mut self, options: TurnOptions) -> Self {
        self.default_turn_options = options;
        self
    }
}

/// `PureCore` 初始化 profile。
///
/// 不同宿主通过该结构选择提示词、workspace 与工具注册策略。pure-studio
/// 使用 `local_workspace`，服务端 smoke test 可使用默认 `minimal`，外部宿主
/// 可使用 `host_provided` 后自行注册工具。
#[derive(Debug, Clone, Default)]
pub struct CoreRuntimeProfile {
    pub instruction_profile: Option<InstructionProfile>,
    pub workspace_profile: WorkspaceProfile,
    pub tool_profile: ToolProfile,
    pub agent_backend: AgentBackendProfile,
    pub runtime_options: CoreRuntimeOptions,
}

impl CoreRuntimeProfile {
    pub fn minimal() -> Self {
        Self::default()
    }

    pub fn local_workspace(root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_profile: WorkspaceProfile::new(root),
            tool_profile: ToolProfile::LocalWorkspace,
            instruction_profile: None,
            agent_backend: AgentBackendProfile::default(),
            runtime_options: CoreRuntimeOptions::default(),
        }
    }

    pub fn host_provided(root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_profile: WorkspaceProfile::new(root),
            tool_profile: ToolProfile::HostProvided,
            instruction_profile: None,
            agent_backend: AgentBackendProfile::default(),
            runtime_options: CoreRuntimeOptions::default(),
        }
    }

    pub fn with_instruction_profile(mut self, profile: InstructionProfile) -> Self {
        self.instruction_profile = Some(profile);
        self
    }

    pub fn with_workspace_instructions(mut self, instructions: impl Into<String>) -> Self {
        let instructions = instructions.into();
        self.workspace_profile.instructions = Some(instructions.clone());
        self.instruction_profile = Some(
            self.instruction_profile
                .take()
                .unwrap_or_default()
                .with_workspace_instructions(instructions),
        );
        self
    }

    pub fn with_tool_profile(mut self, tool_profile: ToolProfile) -> Self {
        self.tool_profile = tool_profile;
        self
    }

    pub fn with_agent_backend(mut self, agent_backend: AgentBackendProfile) -> Self {
        self.agent_backend = agent_backend;
        self
    }

    pub fn with_agent_supervisor(self, supervisor: crate::AgentSupervisor) -> Self {
        self.with_agent_backend(AgentBackendProfile::from_supervisor(supervisor))
    }

    pub fn with_runtime_options(mut self, runtime_options: CoreRuntimeOptions) -> Self {
        self.runtime_options = runtime_options;
        self
    }
}

/// `PureCore` 构造器。
#[derive(Debug, Clone)]
pub struct PureCoreBuilder {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
    config: Option<PureConfig>,
    mcp_runtime: Option<crate::mcp::McpRuntimeRegistry>,
    lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
    runtime_profile: CoreRuntimeProfile,
    agent_supervisor: Option<crate::AgentSupervisor>,
}

impl PureCoreBuilder {
    pub fn new(provider: SharedModelProvider) -> Self {
        Self {
            provider,
            reasoning_effort: None,
            config: None,
            mcp_runtime: None,
            lsp_runtime: None,
            runtime_profile: CoreRuntimeProfile::minimal(),
            agent_supervisor: None,
        }
    }

    pub fn from_provider_info(info: ProviderInfo) -> Result<Self> {
        Ok(Self::new(create_provider(info)?))
    }

    pub fn from_provider_info_with_models(
        info: ProviderInfo,
        models: Vec<pl_model::ModelInfo>,
    ) -> Result<Self> {
        Ok(Self::new(create_provider_with_models(info, models)?))
    }

    pub fn with_reasoning_effort(mut self, reasoning_effort: ReasoningEffort) -> Self {
        self.reasoning_effort = Some(reasoning_effort);
        self
    }

    pub fn with_config(mut self, config: PureConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn with_mcp_runtime(mut self, registry: crate::mcp::McpRuntimeRegistry) -> Self {
        self.mcp_runtime = Some(registry);
        self
    }

    pub fn with_lsp_runtime(mut self, registry: pl_lsp::LspRuntimeRegistry) -> Self {
        self.lsp_runtime = Some(registry);
        self
    }

    pub fn with_runtime_profile(mut self, runtime_profile: CoreRuntimeProfile) -> Self {
        self.runtime_profile = runtime_profile;
        self
    }

    pub fn with_agent_supervisor(mut self, agent_supervisor: crate::AgentSupervisor) -> Self {
        self.agent_supervisor = Some(agent_supervisor);
        self
    }

    pub fn build(self) -> PureCore {
        let CoreRuntimeProfile {
            instruction_profile,
            workspace_profile,
            tool_profile,
            agent_backend,
            runtime_options,
        } = self.runtime_profile;
        let agent_supervisor = self
            .agent_supervisor
            .unwrap_or_else(|| agent_backend.into_supervisor());
        PureCore {
            provider: self.provider,
            reasoning_effort: self.reasoning_effort,
            config: self.config,
            mcp_runtime: self.mcp_runtime,
            lsp_runtime: self.lsp_runtime,
            workspace_root: workspace_profile.root,
            workspace_instructions: workspace_profile.instructions,
            instruction_profile,
            tool_profile,
            runtime_options,
            active_subagent: None,
            agent_supervisor,
            tools: crate::tool::ToolRegistry::new(),
        }
    }
}
