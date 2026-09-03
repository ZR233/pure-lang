use std::path::PathBuf;

use pl_model::runtime::ModelRuntime;
use pl_protocol::Result;

use crate::ResolvedModelRoute;
use crate::config::{ReasoningEffort, SkillsConfig, ToolCapabilityConfig};
use crate::context_compaction::ContextCompactionConfig;
use crate::execution_environment::ExecutionEnvironment;
use crate::instruction::InstructionProfile;
use crate::tool::AgentWorkspace;
use crate::turn::TurnOptions;

use super::TurnEngine;

/// 工具注册策略。
///
/// 该枚举只描述 `pl-core` 是否应该自动注册默认工具。具体工具执行环境
/// 仍由工具实现自身决定；宿主工具始终通过显式注册表发布。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolProfile {
    #[default]
    Minimal,
    LocalWorkspace,
}

/// `TurnEngine` 初始化 profile。
///
/// 不同宿主通过该结构选择提示词、workspace 与工具注册策略。pure-studio
/// 使用 `local_workspace`，服务端 smoke test 与显式注入工具的宿主使用默认
/// `minimal`。
#[derive(Debug, Clone, Default)]
pub struct CoreRuntimeProfile {
    pub instruction_profile: Option<InstructionProfile>,
    pub workspace: Option<AgentWorkspace>,
    pub workspace_instructions: Option<String>,
    pub tool_profile: ToolProfile,
    pub default_turn_options: TurnOptions,
    pub context_compaction: ContextCompactionConfig,
    pub attachment_runtime: Option<crate::AttachmentRuntime>,
    pub execution_environment: Option<ExecutionEnvironment>,
}

impl CoreRuntimeProfile {
    pub fn minimal() -> Self {
        Self::default()
    }

    pub fn local_workspace(root: impl Into<PathBuf>) -> Self {
        Self {
            workspace: Some(AgentWorkspace::local(root)),
            tool_profile: ToolProfile::LocalWorkspace,
            ..Self::default()
        }
    }

    pub fn local_agent_workspace(workspace: AgentWorkspace) -> Self {
        Self {
            workspace: Some(workspace),
            tool_profile: ToolProfile::LocalWorkspace,
            ..Self::default()
        }
    }

    /// 设置工具执行上下文使用的 workspace，但不自动注册本地工具。
    pub fn with_agent_workspace(mut self, workspace: AgentWorkspace) -> Self {
        self.workspace = Some(workspace);
        self
    }

    pub fn with_instruction_profile(mut self, profile: InstructionProfile) -> Self {
        self.instruction_profile = Some(profile);
        self
    }

    pub fn with_workspace_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.workspace_instructions = Some(instructions.into());
        self
    }

    pub fn with_tool_profile(mut self, tool_profile: ToolProfile) -> Self {
        self.tool_profile = tool_profile;
        self
    }

    pub fn with_default_turn_options(mut self, options: TurnOptions) -> Self {
        self.default_turn_options = options;
        self
    }

    pub fn with_context_compaction(mut self, config: ContextCompactionConfig) -> Self {
        self.context_compaction = config;
        self
    }

    /// 绑定当前线程的附件持久化与授权读取运行时。
    pub fn with_attachment_runtime(mut self, runtime: crate::AttachmentRuntime) -> Self {
        self.attachment_runtime = Some(runtime);
        self
    }

    pub fn with_execution_environment(mut self, environment: ExecutionEnvironment) -> Self {
        self.execution_environment = Some(environment);
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
    skill_catalog: Option<std::sync::Arc<crate::skill::FrozenSkillCatalog>>,
    lsp_runtime: Option<pl_lsp::runtime::LspRuntimeRegistry>,
    agent_tools: Option<crate::tool::AgentToolSet>,
    before_model_step: Option<crate::tool::BeforeModelStepHook>,
    runtime_profile: CoreRuntimeProfile,
}

impl TurnEngineBuilder {
    /// 从已校验的角色路由构造绑定单一模型的 Turn runtime。
    pub fn from_route(route: &ResolvedModelRoute) -> Result<Self> {
        let runtime = ModelRuntime::new_with_provider_id(
            route.provider_id.as_str(),
            route.endpoint.clone(),
            route.model.clone(),
        )?;
        Ok(Self {
            runtime,
            effort: route.effort.clone(),
            tool_capabilities: ToolCapabilityConfig::default(),
            skills: None,
            skill_catalog: None,
            lsp_runtime: None,
            agent_tools: None,
            before_model_step: None,
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
        catalog: std::sync::Arc<crate::skill::FrozenSkillCatalog>,
    ) -> Self {
        self.skills
            .get_or_insert_with(crate::config::SkillsConfig::default);
        self.skill_catalog = Some(catalog);
        self
    }

    pub fn with_lsp_runtime(mut self, registry: pl_lsp::runtime::LspRuntimeRegistry) -> Self {
        self.lsp_runtime = Some(registry);
        self
    }

    /// 绑定宿主持久拥有的 per-agent 工具集合。
    pub fn with_agent_tool_set(mut self, tools: crate::tool::AgentToolSet) -> Self {
        self.agent_tools = Some(tools);
        self
    }

    /// 安装每个模型 step 冻结工具 plan 前执行的刷新窗口。
    pub fn with_before_model_step(mut self, hook: crate::tool::BeforeModelStepHook) -> Self {
        self.before_model_step = Some(hook);
        self
    }

    pub fn with_runtime_profile(mut self, runtime_profile: CoreRuntimeProfile) -> Self {
        self.runtime_profile = runtime_profile;
        self
    }

    pub fn build(self) -> TurnEngine {
        let CoreRuntimeProfile {
            instruction_profile,
            workspace,
            workspace_instructions,
            tool_profile,
            default_turn_options,
            context_compaction,
            attachment_runtime,
            execution_environment,
        } = self.runtime_profile;
        let agent_tools = self.agent_tools.unwrap_or_else(|| {
            crate::tool::ToolManager::new()
                .agent_tool_set("standalone", crate::tool::GlobalToolInheritance::Isolated)
        });
        TurnEngine {
            runtime: self.runtime,
            effort: self.effort,
            skills: self.skills,
            skill_catalog: self.skill_catalog,
            lsp_runtime: self.lsp_runtime,
            agent_tools,
            before_model_step: self.before_model_step,
            workspace,
            workspace_instructions,
            instruction_profile,
            tool_profile,
            tool_capabilities: self.tool_capabilities,
            default_turn_options,
            context_compaction,
            attachment_runtime,
            execution_environment: execution_environment
                .unwrap_or_else(ExecutionEnvironment::detect_local),
            active_subagent: None,
            tool_session_runtime: crate::tool::ToolSessionRuntime::default(),
        }
    }
}
