use serde::{Deserialize, Serialize};
/// 共享 agent runtime 的工具能力开关。
///
/// 默认配置保持 anywork 既有本地能力：命令执行、workspace 文件、skills、MCP/LSP
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
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

fn default_true() -> bool {
    true
}
