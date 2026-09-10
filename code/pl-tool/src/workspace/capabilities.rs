use serde::{Deserialize, Serialize};
/// 共享 agent runtime 的工具能力开关。
///
/// 默认配置保持 pure-studio 既有本地能力：命令执行、workspace 文件、skills、MCP/LSP
/// 和用户输入工具开启；git 等产品能力关闭。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCapabilityConfig {
    #[serde(default = "default_true")]
    pub exec: bool,
    #[serde(default = "default_true")]
    pub workspace_files: bool,
    #[serde(default = "default_true")]
    pub skills: bool,
    #[serde(default = "default_true")]
    pub mcp: bool,
    #[serde(default = "default_true")]
    pub lsp: bool,
    #[serde(default = "default_true")]
    pub ask_user: bool,
    #[serde(default)]
    pub git: bool,
}

impl Default for ToolCapabilityConfig {
    fn default() -> Self {
        Self {
            exec: true,
            workspace_files: true,
            skills: true,
            mcp: true,
            lsp: true,
            ask_user: true,
            git: false,
        }
    }
}

impl ToolCapabilityConfig {
    /// 产品显式提供 workspace/backend 时使用的通用工具能力预设。
    ///
    /// 该预设关闭 skills 和 LSP，保留 exec、workspace file、
    /// MCP、用户输入与 git 工具。agent 协作工具
    /// 由 Studio Thread 装配器按执行策略注册，不属于工作区能力配置。
    pub fn hosted_workspace() -> Self {
        Self {
            exec: true,
            workspace_files: true,
            skills: false,
            mcp: true,
            lsp: false,
            ask_user: true,
            git: true,
        }
    }

    /// 只注册 git workspace 工具时使用的共享能力预设。
    ///
    /// 产品层可用该预设通过 `pl_tool::workspace::WorkspaceTools` 执行 git 工具，而不必在宿主项目里
    /// 复制一份共享工具能力矩阵。
    pub fn git_workspace() -> Self {
        Self {
            exec: false,
            workspace_files: false,
            skills: false,
            mcp: false,
            lsp: false,
            ask_user: false,
            git: true,
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

fn default_true() -> bool {
    true
}
