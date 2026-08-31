use serde::{Deserialize, Serialize};

/// 子 Agent 使用的工作区执行模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentWorkspaceMode {
    /// 不增加 Profile 级路径限制；宿主权限模式仍然生效。
    Unrestricted,
    /// 与父 Agent 共享项目目录，并可由 spawn 参数限制内置文件工具的写入目录。
    #[default]
    Directory,
    /// 在独立 Git worktree 中执行。
    Worktree,
}

/// 关闭 worktree Agent 时对物理 workspace 的显式处置。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentWorkspaceDisposition {
    #[default]
    Preserve,
    Cleanup,
}

impl AgentWorkspaceMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unrestricted => "unrestricted",
            Self::Directory => "directory",
            Self::Worktree => "worktree",
        }
    }
}

/// Worktree 模式在 spawn 时冻结的 Git 资源收据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentWorktreeSnapshot {
    pub repository_root: String,
    pub path: String,
    pub branch: String,
    pub base_commit: String,
}

/// 子 Agent 在创建时冻结的有效工作区边界。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentWorkspaceAssignmentSnapshot {
    pub mode: AgentWorkspaceMode,
    pub project_root: String,
    pub root: String,
    /// `None` 表示整个项目可写，空数组表示项目内只读。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writable_paths: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<AgentWorktreeSnapshot>,
}

/// Agent Profile 在子会话创建时冻结的可执行快照。
///
/// `profile_id` 同时作为运行时角色标识；provider/model/effort 与系统指令不再
/// 随磁盘配置变化，保证已启动 Agent 的行为可复现。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileSnapshot {
    pub profile_id: String,
    pub display_name: String,
    pub description: String,
    pub when_to_use: String,
    pub system_instructions: String,
    pub provider_id: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub source: String,
    pub revision: String,
    pub content_hash: String,
    #[serde(default)]
    pub system: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub workspace_mode: AgentWorkspaceMode,
}

const fn default_true() -> bool {
    true
}
