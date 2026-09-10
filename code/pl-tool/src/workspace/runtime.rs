//! Workspace tool resources and shared file mutation leases.
use crate::workspace::{AgentWorkspace, WorkspaceMutability};
use pl_core::tool::opaque::CallContext;
use std::{
    fmt,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::sync::{Mutex, OwnedMutexGuard};

/// 由 workspace 类工具在注册时捕获的稳定运行时依赖。
///
/// 该对象统一持有 agent workspace、LSP 通知出口与写入串行化边界；调用级批准仍从
/// [`CallContext`] 读取，因此旧 plan 不会看到后续 workspace replacement。
#[derive(Clone)]
pub struct ToolWorkspace {
    workspace: AgentWorkspace,
    lsp_runtime: Option<pl_lsp::runtime::LspRuntimeRegistry>,
}

impl ToolWorkspace {
    pub fn new(workspace: AgentWorkspace) -> Self {
        Self {
            workspace,
            lsp_runtime: None,
        }
    }

    pub fn with_lsp_runtime(
        mut self,
        runtime: Option<pl_lsp::runtime::LspRuntimeRegistry>,
    ) -> Self {
        self.lsp_runtime = runtime;
        self
    }

    pub fn workspace(&self) -> &AgentWorkspace {
        &self.workspace
    }

    /// Non-secret execution-policy identity; reconnecting services does not change it.
    pub fn authorization(&self) -> pl_core::tool::opaque::ToolAuthorization {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        let mut part = |bytes: &[u8]| {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        };
        part(b"pl-tool/workspace-authorization/v1");
        part(self.workspace.root().as_os_str().as_encoded_bytes());
        part(self.workspace.project_root().as_os_str().as_encoded_bytes());
        part(match self.workspace.boundary() {
            super::WorkspaceBoundary::Confined => b"confined",
            super::WorkspaceBoundary::HostPermitted => b"host-permitted",
        });
        part(match self.workspace.mutability() {
            WorkspaceMutability::ReadOnly => b"read-only",
            WorkspaceMutability::ReadWrite => b"read-write",
        });
        match self.workspace.project_writable_paths() {
            None => part(b"all-project-paths"),
            Some(paths) => {
                part(b"selected-project-paths");
                let mut paths = paths.iter().collect::<Vec<_>>();
                paths.sort();
                paths.dedup();
                for path in paths {
                    part(path.as_os_str().as_encoded_bytes());
                }
            }
        }
        pl_core::tool::opaque::ToolAuthorization::new(format!("pl.workspace:{:x}", hash.finalize()))
    }

    pub fn root(&self) -> &std::path::Path {
        self.workspace.root()
    }

    pub fn allows_workspace_escape(&self, context: &CallContext) -> bool {
        self.workspace.boundary().allows_host_paths()
            && context
                .grant
                .contains(crate::approval::HOST_WORKSPACE_ACCESS)
    }

    pub fn ensure_workspace_writable(&self) -> pl_protocol::Result<()> {
        if self.workspace.mutability() == WorkspaceMutability::ReadOnly {
            return Err(pl_protocol::PureError::ToolExecutionFailed {
                tool: "workspace".to_string(),
                error: "agent workspace is read-only".to_string(),
            });
        }
        Ok(())
    }

    /// 校验一次 Pure 内置文件 mutation 的最终解析路径。
    pub fn ensure_path_writable(&self, path: &std::path::Path) -> pl_protocol::Result<()> {
        self.workspace.ensure_path_writable(path)
    }

    /// 校验 confined backend 中一次 workspace-relative 文件 mutation。
    pub fn ensure_relative_path_writable(
        &self,
        cwd: Option<&str>,
        path: &str,
    ) -> pl_protocol::Result<()> {
        self.workspace.ensure_relative_path_writable(cwd, path)
    }

    /// Acquires the workspace mutation lease shared by file backends for this root.
    pub async fn write_lock(&self) -> WorkspaceWriteGuard {
        workspace_write_locks()
            .lock_for(self.workspace.root())
            .await
    }

    /// Notifies the bound language service after a successful file mutation.
    pub async fn notify_changed(&self, path: &std::path::Path) {
        if let Some(runtime) = &self.lsp_runtime {
            runtime.notify_file_changed(path).await;
        }
    }

    /// Notifies the bound language service after a successful removal.
    pub async fn notify_deleted(&self, path: &std::path::Path) {
        if let Some(runtime) = &self.lsp_runtime {
            runtime.notify_file_deleted(path).await;
        }
    }

    /// Returns the explicitly bound language-service capability for a file backend.
    pub fn lsp_runtime(&self) -> Option<pl_lsp::runtime::LspRuntimeRegistry> {
        self.lsp_runtime.clone()
    }
}

impl fmt::Debug for ToolWorkspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolWorkspace")
            .field("workspace", &self.workspace)
            .field("lsp_runtime", &self.lsp_runtime.is_some())
            .finish()
    }
}

/// Owned cooperative file-mutation lease; dropping it admits the next writer.
pub type WorkspaceWriteGuard = OwnedMutexGuard<()>;

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

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn workspace_write_lock_is_shared_for_same_workspace() {
        let root = std::env::temp_dir().join(format!(
            "pure-lang-write-lock-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let workspace = ToolWorkspace::new(AgentWorkspace::local(root.clone()));
        let first_guard = workspace.write_lock().await;
        let second_workspace = workspace.clone();
        let second = tokio::spawn(async move { second_workspace.write_lock().await });
        tokio::task::yield_now().await;

        assert!(!second.is_finished());
        drop(first_guard);
        let second_guard = second.await.unwrap();
        drop(second_guard);
        let _ = tokio::fs::remove_dir_all(root).await;
    }
    #[test]
    fn workspace_authorization_changes_with_policy_but_not_path_order_or_service_clones() {
        let first = ToolWorkspace::new(AgentWorkspace::directory(
            "project",
            Some(vec!["src".into(), "docs".into()]),
        ));
        let same = ToolWorkspace::new(AgentWorkspace::directory(
            "project",
            Some(vec!["docs".into(), "src".into(), "src".into()]),
        ));
        assert_eq!(first.authorization(), same.authorization());
        assert_eq!(first.authorization(), first.clone().authorization());
        let revoked = ToolWorkspace::new(AgentWorkspace::directory("project", Some(Vec::new())));
        assert_ne!(first.authorization(), revoked.authorization());
        let unrestricted = ToolWorkspace::new(AgentWorkspace::directory("project", None));
        assert_ne!(revoked.authorization(), unrestricted.authorization());
    }
}
