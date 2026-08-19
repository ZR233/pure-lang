use std::path::PathBuf;

use pl_model::ModelRuntime;
use pl_protocol::Result;

use crate::ResolvedModelRoute;
use crate::config::{ReasoningEffort, SkillsConfig, ToolCapabilityConfig};
use crate::context_compaction::ContextCompactionConfig;
use crate::instruction::InstructionProfile;
use crate::tool::AgentWorkspace;
use crate::turn::TurnOptions;

use super::TurnEngine;

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
    pub workspace: Option<AgentWorkspace>,
    pub instructions: Option<String>,
}

impl WorkspaceProfile {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            workspace: Some(AgentWorkspace::local(root)),
            instructions: None,
        }
    }

    pub fn from_agent_workspace(workspace: AgentWorkspace) -> Self {
        Self {
            workspace: Some(workspace),
            instructions: None,
        }
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
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

/// `TurnEngine` 初始化 profile。
///
/// 不同宿主通过该结构选择提示词、workspace 与工具注册策略。pure-studio
/// 使用 `local_workspace`，服务端 smoke test 可使用默认 `minimal`，外部宿主
/// 可使用 `host_provided` 后自行注册工具。
#[derive(Debug, Clone, Default)]
pub struct CoreRuntimeProfile {
    pub instruction_profile: Option<InstructionProfile>,
    pub workspace_profile: WorkspaceProfile,
    pub tool_profile: ToolProfile,
    pub runtime_options: CoreRuntimeOptions,
    pub context_compaction: ContextCompactionConfig,
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
            runtime_options: CoreRuntimeOptions::default(),
            context_compaction: ContextCompactionConfig::default(),
        }
    }

    pub fn local_agent_workspace(workspace: AgentWorkspace) -> Self {
        Self {
            workspace_profile: WorkspaceProfile::from_agent_workspace(workspace),
            tool_profile: ToolProfile::LocalWorkspace,
            instruction_profile: None,
            runtime_options: CoreRuntimeOptions::default(),
            context_compaction: ContextCompactionConfig::default(),
        }
    }

    pub fn host_provided(root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_profile: WorkspaceProfile::new(root),
            tool_profile: ToolProfile::HostProvided,
            instruction_profile: None,
            runtime_options: CoreRuntimeOptions::default(),
            context_compaction: ContextCompactionConfig::default(),
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

    pub fn with_runtime_options(mut self, runtime_options: CoreRuntimeOptions) -> Self {
        self.runtime_options = runtime_options;
        self
    }

    pub fn with_context_compaction(mut self, config: ContextCompactionConfig) -> Self {
        self.context_compaction = config;
        self
    }
}

/// `TurnEngine` 构造器。
#[derive(Debug, Clone)]
pub struct TurnEngineBuilder {
    runtime: ModelRuntime,
    effort: Option<ReasoningEffort>,
    tool_capabilities: ToolCapabilityConfig,
    skills: Option<SkillsConfig>,
    skill_catalog: Option<std::sync::Arc<crate::skill::SkillCatalog>>,
    lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
    shared_tool_registry: Option<std::sync::Arc<crate::tool::ToolRegistry>>,
    runtime_profile: CoreRuntimeProfile,
}

impl TurnEngineBuilder {
    /// 从已校验的角色路由构造绑定单一模型的 Turn runtime。
    pub fn from_route(route: &ResolvedModelRoute) -> Result<Self> {
        let runtime = ModelRuntime::new(route.endpoint.clone(), route.model.clone())?;
        Ok(Self {
            runtime,
            effort: route.effort.clone(),
            tool_capabilities: ToolCapabilityConfig::default(),
            skills: None,
            skill_catalog: None,
            lsp_runtime: None,
            shared_tool_registry: None,
            runtime_profile: CoreRuntimeProfile::minimal(),
        })
    }

    pub fn with_tool_capabilities(mut self, capabilities: ToolCapabilityConfig) -> Self {
        self.tool_capabilities = capabilities;
        self
    }

    pub fn with_skills_config(mut self, skills: SkillsConfig) -> Self {
        self.skills = Some(skills);
        self
    }

    pub fn with_skill_catalog(
        mut self,
        catalog: std::sync::Arc<crate::skill::SkillCatalog>,
    ) -> Self {
        self.skill_catalog = Some(catalog);
        self
    }

    pub fn with_lsp_runtime(mut self, registry: pl_lsp::LspRuntimeRegistry) -> Self {
        self.lsp_runtime = Some(registry);
        self
    }

    /// 挂接共享工具注册表（如 MCP worker 的代际发布）。
    pub fn with_shared_tool_registry(
        mut self,
        registry: std::sync::Arc<crate::tool::ToolRegistry>,
    ) -> Self {
        self.shared_tool_registry = Some(registry);
        self
    }

    pub fn with_runtime_profile(mut self, runtime_profile: CoreRuntimeProfile) -> Self {
        self.runtime_profile = runtime_profile;
        self
    }

    pub fn build(self) -> TurnEngine {
        let CoreRuntimeProfile {
            instruction_profile,
            workspace_profile,
            tool_profile,
            runtime_options,
            context_compaction,
        } = self.runtime_profile;
        TurnEngine {
            runtime: self.runtime,
            effort: self.effort,
            skills: self.skills,
            skill_catalog: self.skill_catalog,
            lsp_runtime: self.lsp_runtime,
            shared_tools: self.shared_tool_registry,
            workspace: workspace_profile.workspace,
            workspace_instructions: workspace_profile.instructions,
            instruction_profile,
            tool_profile,
            tool_capabilities: self.tool_capabilities,
            runtime_options,
            context_compaction,
            active_subagent: None,
            tools: crate::tool::ToolRegistry::new(),
            local_sources: Default::default(),
            tool_guards: Default::default(),
        }
    }
}
