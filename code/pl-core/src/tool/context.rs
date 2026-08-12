use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use pl_trace::AgentEventSender;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::turn::TurnOptions;

/// Agent workspace 是否允许宿主权限策略访问 root 之外的路径。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceBoundary {
    #[default]
    Confined,
    HostPermitted,
}

impl WorkspaceBoundary {
    fn allows_host_paths(self) -> bool {
        matches!(self, Self::HostPermitted)
    }
}

/// Agent workspace 的修改能力。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceMutability {
    ReadOnly,
    #[default]
    ReadWrite,
}

/// 单个 Agent 的 canonical workspace 边界。
///
/// 宿主负责根据 durable owner 构造该值；所有内置路径工具、命令 cwd、Git、LSP 与项目
/// skills 必须消费同一个 root。`Confined` 不会被 turn 的 `full-access` 权限放宽。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentWorkspace {
    root: PathBuf,
    boundary: WorkspaceBoundary,
    mutability: WorkspaceMutability,
}

impl AgentWorkspace {
    pub fn local(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            boundary: WorkspaceBoundary::HostPermitted,
            mutability: WorkspaceMutability::ReadWrite,
        }
    }

    pub fn confined(root: impl Into<PathBuf>, mutability: WorkspaceMutability) -> Self {
        Self {
            root: root.into(),
            boundary: WorkspaceBoundary::Confined,
            mutability,
        }
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn boundary(&self) -> WorkspaceBoundary {
        self.boundary
    }

    pub fn mutability(&self) -> WorkspaceMutability {
        self.mutability
    }
}

/// Runtime coordination policy for tools within one model response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRuntimeLockPolicy {
    Exclusive,
    Shared,
    None,
}

/// 单次工具执行上下文。
///
/// 由核心 turn 循环注入，工具通过它访问事件流、审批策略、
/// workspace 信息，以及当前 subagent 的父子关系。
#[derive(Clone)]
pub struct ToolContext {
    pub event_tx: AgentEventSender,
    pub options: TurnOptions,
    pub workspace_access: WorkspaceAccess,
    pub workspace: AgentWorkspace,
    pub workspace_instructions: Option<String>,
    pub instruction_snapshot: Option<crate::instruction::InstructionSnapshot>,
    pub provider_call_id: Option<String>,
    pub active_subagent: Option<SubagentContext>,
    pub lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
    pub parent_session: Arc<crate::session::AgentSession>,
    pub working_set: crate::TurnWorkingSetHandle,
    pub tool_cache: crate::tool::cache::TurnToolCacheHandle,
}

/// 单次工具调用的路径访问策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceAccess {
    #[default]
    WorkspaceOnly,
    ExternalAllowed,
}

impl WorkspaceAccess {
    pub fn allows_external(self) -> bool {
        matches!(self, Self::ExternalAllowed)
    }
}

/// 当前工具调用所在的 subagent 运行边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentContext {
    pub id: String,
    pub parent_id: Option<String>,
    pub agent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub depth: u32,
}

impl fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolContext")
            .field("workspace", &self.workspace)
            .field("permission_mode", &self.options.permission_mode)
            .field("workspace_access", &self.workspace_access)
            .field("provider_call_id", &self.provider_call_id)
            .field("active_subagent", &self.active_subagent)
            .field("lsp_runtime", &self.lsp_runtime.is_some())
            .field("working_set", &self.working_set)
            .field("workspace_epoch", &self.tool_cache.workspace_epoch())
            .finish_non_exhaustive()
    }
}

impl ToolContext {
    pub(crate) fn allows_workspace_escape(&self) -> bool {
        self.workspace.boundary.allows_host_paths()
            && (self.options.permission_mode.allows_workspace_escape()
                || self.workspace_access.allows_external())
    }

    pub(crate) fn ensure_workspace_writable(&self) -> crate::Result<()> {
        if self.workspace.mutability == WorkspaceMutability::ReadOnly {
            return Err(crate::PureError::ToolExecutionFailed {
                tool: "workspace".to_string(),
                error: "agent workspace is read-only".to_string(),
            });
        }
        Ok(())
    }

    pub(crate) async fn workspace_write_lock(&self) -> WorkspaceWriteGuard {
        workspace_write_locks()
            .lock_for(self.workspace.root())
            .await
    }
}

type WorkspaceWriteGuard = OwnedMutexGuard<()>;

#[derive(Default)]
struct WorkspaceWriteLocks {
    locks: std::sync::Mutex<std::collections::HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl WorkspaceWriteLocks {
    async fn lock_for(&self, workspace_root: &std::path::Path) -> WorkspaceWriteGuard {
        let key =
            std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
        let lock = {
            let mut locks = self.locks.lock().unwrap_or_else(|poisoned| {
                tracing::warn!("workspace write lock was poisoned, recovering");
                poisoned.into_inner()
            });
            locks
                .entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }
}

fn workspace_write_locks() -> &'static WorkspaceWriteLocks {
    static LOCKS: OnceLock<WorkspaceWriteLocks> = OnceLock::new();
    LOCKS.get_or_init(WorkspaceWriteLocks::default)
}
