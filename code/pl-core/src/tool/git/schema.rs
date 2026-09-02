use pl_model::ToolSpec;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::tool::cache::ToolCachePolicy;
use crate::tool::typed_tool_input_schema;
use crate::turn::ToolEffect;

pub const TOOL_GIT_STATUS: &str = "git_status";
pub const TOOL_GIT_DIFF: &str = "git_diff";
pub const TOOL_GIT_BRANCH: &str = "git_branch";
pub const TOOL_GIT_FETCH: &str = "git_fetch";
pub const TOOL_GIT_COMMIT: &str = "git_commit";
pub const TOOL_GIT_PUSH: &str = "git_push";
pub const TOOL_GIT_WORKSPACE_INFO: &str = "git_workspace_info";
pub const TOOL_GIT_SYNC_DEFAULT_BRANCH: &str = "git_sync_default_branch";

/// pl-core 提供的通用 git 工具类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitToolKind {
    Status,
    Diff,
    Branch,
    Fetch,
    Commit,
    Push,
    WorkspaceInfo,
    SyncDefaultBranch,
}

impl GitToolKind {
    pub fn all() -> &'static [Self] {
        &[
            Self::Status,
            Self::Diff,
            Self::Branch,
            Self::Fetch,
            Self::Commit,
            Self::Push,
            Self::WorkspaceInfo,
            Self::SyncDefaultBranch,
        ]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            TOOL_GIT_STATUS => Some(Self::Status),
            TOOL_GIT_DIFF => Some(Self::Diff),
            TOOL_GIT_BRANCH => Some(Self::Branch),
            TOOL_GIT_FETCH => Some(Self::Fetch),
            TOOL_GIT_COMMIT => Some(Self::Commit),
            TOOL_GIT_PUSH => Some(Self::Push),
            TOOL_GIT_WORKSPACE_INFO => Some(Self::WorkspaceInfo),
            TOOL_GIT_SYNC_DEFAULT_BRANCH => Some(Self::SyncDefaultBranch),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Status => TOOL_GIT_STATUS,
            Self::Diff => TOOL_GIT_DIFF,
            Self::Branch => TOOL_GIT_BRANCH,
            Self::Fetch => TOOL_GIT_FETCH,
            Self::Commit => TOOL_GIT_COMMIT,
            Self::Push => TOOL_GIT_PUSH,
            Self::WorkspaceInfo => TOOL_GIT_WORKSPACE_INFO,
            Self::SyncDefaultBranch => TOOL_GIT_SYNC_DEFAULT_BRANCH,
        }
    }

    /// 该类别 git 工具的副作用声明。
    pub fn effect(self) -> ToolEffect {
        match self {
            Self::Status | Self::Diff | Self::WorkspaceInfo => ToolEffect::Read,
            Self::Branch | Self::Fetch | Self::Commit | Self::Push | Self::SyncDefaultBranch => {
                ToolEffect::BranchControl
            }
        }
    }

    /// 只读 git 查询结果在 workspace mutation epoch 内可复用。
    pub fn cache_policy(self) -> ToolCachePolicy {
        match self {
            Self::Status | Self::Diff | Self::WorkspaceInfo => {
                ToolCachePolicy::UntilWorkspaceMutation
            }
            Self::Branch | Self::Fetch | Self::Commit | Self::Push | Self::SyncDefaultBranch => {
                ToolCachePolicy::Never
            }
        }
    }

    pub(super) fn description(self) -> &'static str {
        match self {
            Self::Status => "Show git working tree status for this workspace.",
            Self::Diff => "Show git diff for this workspace.",
            Self::Branch => "List branches or create/switch the current branch.",
            Self::Fetch => "Fetch from the repository remote using host-injected credentials.",
            Self::Commit => "Create a git commit in this workspace.",
            Self::Push => "Push the current branch using host-injected credentials.",
            Self::WorkspaceInfo => "Show information about this git workspace.",
            Self::SyncDefaultBranch => {
                "Synchronize this workspace branch with the configured default branch."
            }
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::Status | Self::WorkspaceInfo => typed_tool_input_schema::<GitEmptyInput>(),
            Self::Diff => typed_tool_input_schema::<GitDiffInput>(),
            Self::Branch => typed_tool_input_schema::<GitBranchInput>(),
            Self::Fetch => typed_tool_input_schema::<GitFetchInput>(),
            Self::Commit => typed_tool_input_schema::<GitCommitInput>(),
            Self::Push => typed_tool_input_schema::<GitPushInput>(),
            Self::SyncDefaultBranch => typed_tool_input_schema::<GitSyncDefaultBranchInput>(),
        }
    }

    pub fn to_spec(self) -> ToolSpec {
        ToolSpec::function(self.name(), self.description(), self.input_schema())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GitEmptyInput {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitDiffInput {
    /// Show staged changes instead of unstaged changes.
    #[serde(default)]
    pub(super) staged: bool,
    /// Optional repository-relative path to limit the diff.
    pub(super) path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitBranchInput {
    /// Branch action; defaults to list.
    pub(super) action: Option<GitBranchAction>,
    /// Branch name used by switch and create.
    pub(super) name: Option<String>,
    /// Optional starting revision for create.
    pub(super) start_point: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) enum GitBranchAction {
    List,
    Switch,
    Create,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GitRemoteInput {
    /// Remote name; defaults to origin.
    remote: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GitFetchInput {
    #[serde(flatten)]
    remote: GitRemoteInput,
    /// Optional fetch refspec.
    pub(super) refspec: Option<String>,
    /// Whether to prune stale remote-tracking refs.
    #[serde(default = "default_true")]
    pub(super) prune: bool,
}

impl GitFetchInput {
    pub(super) fn remote(&self) -> Option<String> {
        self.remote.remote.clone()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitCommitInput {
    /// Commit message.
    pub(super) message: String,
    /// Commit all tracked modified and deleted files.
    #[serde(default)]
    pub(super) all: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GitPushInput {
    #[serde(flatten)]
    remote: GitRemoteInput,
    /// Destination branch; defaults to the configured push branch.
    pub(super) branch: Option<String>,
    /// Set the upstream branch while pushing.
    #[serde(default)]
    pub(super) set_upstream: bool,
}

impl GitPushInput {
    pub(super) fn remote(&self) -> Option<String> {
        self.remote.remote.clone()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitSyncDefaultBranchInput {
    /// Discard uncommitted workspace changes while syncing.
    #[serde(default)]
    pub(super) force: bool,
    /// Stash uncommitted changes before syncing and restore them afterwards.
    #[serde(default)]
    pub(super) preserve_changes: bool,
}

fn default_true() -> bool {
    true
}
