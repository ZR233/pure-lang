use serde::{Deserialize, Serialize};

use crate::turn::PermissionMode;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    #[serde(default, skip_serializing_if = "PermissionMode::is_default")]
    pub permission_mode: PermissionMode,
    #[serde(default, skip_serializing_if = "ToolCapabilityConfig::is_default")]
    pub tool_capabilities: ToolCapabilityConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_mcp_servers: Vec<String>,
}

impl RuntimeConfig {
    pub fn is_empty(&self) -> bool {
        self.permission_mode.is_default()
            && self.tool_capabilities.is_default()
            && self.active_skills.is_empty()
            && self.active_mcp_servers.is_empty()
    }
}

/// 共享 agent runtime 的工具能力开关。
///
/// 默认配置保持 pure-studio 既有本地能力：shell、workspace 文件、skills、MCP/LSP、
/// subagent 和用户输入工具开启；git、docker、container 等产品/环境相关能力关闭。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCapabilityConfig {
    #[serde(default = "default_true")]
    pub bash: bool,
    #[serde(default = "default_true")]
    pub workspace_files: bool,
    #[serde(default = "default_true")]
    pub skills: bool,
    #[serde(default = "default_true")]
    pub mcp: bool,
    #[serde(default = "default_true")]
    pub lsp: bool,
    #[serde(default = "default_true")]
    pub subagents: bool,
    #[serde(default = "default_true")]
    pub ask_user: bool,
    #[serde(default)]
    pub git: bool,
    #[serde(default)]
    pub docker: bool,
    #[serde(default)]
    pub container: bool,
}

impl Default for ToolCapabilityConfig {
    fn default() -> Self {
        Self {
            bash: true,
            workspace_files: true,
            skills: true,
            mcp: true,
            lsp: true,
            subagents: true,
            ask_user: true,
            git: false,
            docker: false,
            container: false,
        }
    }
}

impl ToolCapabilityConfig {
    /// Host 提供容器 workspace/backend 时使用的共享 agent 工具能力预设。
    ///
    /// 该预设关闭 pure-studio 本地 shell、skills、LSP 和 Docker 管理能力，
    /// 保留 workspace file、MCP、subagent、用户输入、git 与 container 工具。
    /// 宿主仍可在注册工具集时按实际 backend 可用性进一步关闭单项能力。
    pub fn hosted_container_workspace() -> Self {
        Self {
            bash: false,
            workspace_files: true,
            skills: false,
            mcp: true,
            lsp: false,
            subagents: true,
            ask_user: true,
            git: true,
            docker: false,
            container: true,
        }
    }

    /// 只注册 git workspace 工具时使用的共享能力预设。
    ///
    /// 产品层可用该预设通过 `ToolSetBuilder` 执行 git 工具，而不必在宿主项目里
    /// 复制一份共享工具能力矩阵。
    pub fn git_workspace() -> Self {
        Self {
            bash: false,
            workspace_files: false,
            skills: false,
            mcp: false,
            lsp: false,
            subagents: false,
            ask_user: false,
            git: true,
            docker: false,
            container: false,
        }
    }

    pub fn with_git(mut self, enabled: bool) -> Self {
        self.git = enabled;
        self
    }

    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub auto_learn: bool,
    #[serde(default)]
    pub system: SystemSkillsConfig,
    #[serde(default = "default_project_skills_dir")]
    pub project_dir: String,
    #[serde(default = "default_user_skills_dir")]
    pub user_dir: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_dirs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
    #[serde(default = "default_auto_learn_min_tool_calls")]
    pub auto_learn_min_tool_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemSkillsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_learn: true,
            system: SystemSkillsConfig::default(),
            project_dir: default_project_skills_dir(),
            user_dir: default_user_skills_dir(),
            external_dirs: Vec::new(),
            disabled: Vec::new(),
            auto_learn_min_tool_calls: default_auto_learn_min_tool_calls(),
        }
    }
}

impl Default for SystemSkillsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl SkillsConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

fn default_true() -> bool {
    true
}

fn default_project_skills_dir() -> String {
    "skills".to_string()
}

fn default_user_skills_dir() -> String {
    "~/.pure/skills".to_string()
}

fn default_auto_learn_min_tool_calls() -> u32 {
    5
}
