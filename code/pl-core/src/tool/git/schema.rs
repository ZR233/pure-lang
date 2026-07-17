use pl_model::ToolSchema;
use serde::Deserialize;
use serde_json::{Value, json};

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
            Self::Status | Self::WorkspaceInfo => object_schema(vec![]),
            Self::Diff => object_schema(vec![
                ("staged", json!({ "type": "boolean" }), false),
                ("path", json!({ "type": "string" }), false),
            ]),
            Self::Branch => object_schema(vec![
                (
                    "action",
                    json!({ "type": "string", "enum": ["list", "switch", "create"] }),
                    false,
                ),
                ("name", json!({ "type": "string" }), false),
                ("startPoint", json!({ "type": "string" }), false),
            ]),
            Self::Fetch => object_schema(vec![
                ("remote", json!({ "type": "string" }), false),
                ("refspec", json!({ "type": "string" }), false),
                ("prune", json!({ "type": "boolean" }), false),
            ]),
            Self::Commit => object_schema(vec![
                ("message", json!({ "type": "string" }), true),
                ("all", json!({ "type": "boolean" }), false),
            ]),
            Self::Push => object_schema(vec![
                ("remote", json!({ "type": "string" }), false),
                ("branch", json!({ "type": "string" }), false),
                ("setUpstream", json!({ "type": "boolean" }), false),
            ]),
            Self::SyncDefaultBranch => object_schema(vec![
                (
                    "force",
                    json!({
                        "type": "boolean",
                        "description": "Discard uncommitted workspace changes while syncing."
                    }),
                    false,
                ),
                (
                    "preserveChanges",
                    json!({
                        "type": "boolean",
                        "description": "Stash uncommitted workspace changes before syncing and restore them afterwards."
                    }),
                    false,
                ),
            ]),
        }
    }

    pub fn to_schema(self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitDiffInput {
    #[serde(default)]
    pub(super) staged: bool,
    pub(super) path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitBranchInput {
    pub(super) action: Option<String>,
    pub(super) name: Option<String>,
    pub(super) start_point: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitFetchInput {
    pub(super) remote: Option<String>,
    pub(super) refspec: Option<String>,
    #[serde(default = "default_true")]
    pub(super) prune: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitCommitInput {
    pub(super) message: String,
    #[serde(default)]
    pub(super) all: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitPushInput {
    pub(super) remote: Option<String>,
    pub(super) branch: Option<String>,
    #[serde(default)]
    pub(super) set_upstream: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GitSyncDefaultBranchInput {
    #[serde(default)]
    pub(super) force: bool,
    #[serde(default)]
    pub(super) preserve_changes: bool,
}

fn object_schema(properties: Vec<(&'static str, Value, bool)>) -> Value {
    let mut props = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, schema, is_required) in properties {
        props.insert(name.to_string(), schema);
        if is_required {
            required.push(Value::String(name.to_string()));
        }
    }
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn default_true() -> bool {
    true
}
