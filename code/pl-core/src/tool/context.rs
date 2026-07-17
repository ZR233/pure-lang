use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use pl_trace::AgentEventSender;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::turn::TurnOptions;

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
    pub workspace_root: PathBuf,
    pub workspace_instructions: Option<String>,
    pub instruction_snapshot: Option<crate::instruction::InstructionSnapshot>,
    pub provider_call_id: Option<String>,
    pub active_subagent: Option<SubagentContext>,
    pub lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
    pub parent_session: Arc<crate::session::AgentSession>,
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
            .field("workspace_root", &self.workspace_root)
            .field("permission_mode", &self.options.permission_mode)
            .field("workspace_access", &self.workspace_access)
            .field("provider_call_id", &self.provider_call_id)
            .field("active_subagent", &self.active_subagent)
            .field("lsp_runtime", &self.lsp_runtime.is_some())
            .finish_non_exhaustive()
    }
}

impl ToolContext {
    pub(crate) fn allows_workspace_escape(&self) -> bool {
        self.options.permission_mode.allows_workspace_escape()
            || self.workspace_access.allows_external()
    }

    pub(crate) async fn workspace_write_lock(&self) -> WorkspaceWriteGuard {
        workspace_write_locks().lock_for(&self.workspace_root).await
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
