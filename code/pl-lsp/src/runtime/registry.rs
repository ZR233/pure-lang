//! LSP runtime registry：进程、连接、handler、diagnostics、activity 和 snapshot 的唯一 owner。
//!
//! 其他 runtime 模块围绕这里的 owner 状态实现单一职责的编排步骤。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock, broadcast};

use crate::catalog::LspServerCatalog;
use crate::client::LspClient;
use crate::driver::LspServerDriver;
use crate::host::LspHostBackend;
use crate::query::{LspDiagnostic, LspQuery, LspQueryResult};

use super::server::ResolvedLspServer;
use super::{LspActivityKind, LspAvailabilityKind, LspResult, LspServerSnapshot};

/// 进程内 LSP runtime 的唯一 owner；clone 共享同一份状态。
#[derive(Clone)]
pub struct LspRuntimeRegistry {
    pub(super) state: Arc<Mutex<LspRuntimeState>>,
    pub(super) lifecycle: Arc<RwLock<()>>,
    pub(super) updates: broadcast::Sender<()>,
}

impl std::fmt::Debug for LspRuntimeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspRuntimeRegistry").finish_non_exhaustive()
    }
}

impl Default for LspRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LspRuntimeRegistry {
    /// 使用内置 catalog 构造 registry。
    pub fn new() -> Self {
        Self::with_catalog(LspServerCatalog::builtin())
    }

    /// 使用宿主提供的 catalog 构造 registry（内置 catalog 可被替换或裁剪）。
    pub fn with_catalog(catalog: LspServerCatalog) -> Self {
        let (updates, _) = broadcast::channel(64);
        Self {
            state: Arc::new(Mutex::new(LspRuntimeState {
                workspaces: BTreeMap::new(),
                catalog,
                closed: false,
            })),
            lifecycle: Arc::new(RwLock::new(())),
            updates,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.updates.subscribe()
    }

    pub async fn query(&self, query: LspQuery) -> LspResult<LspQueryResult> {
        let workspace_root = self.workspace_root_for_query(&query).await?;
        self.query_in_workspace(workspace_root, query).await
    }

    pub async fn shutdown(&self) {
        self.state.lock().await.closed = true;
        let _lifecycle_guard = self.lifecycle.write().await;
        let clients = {
            let mut state = self.state.lock().await;
            let clients = state
                .workspaces
                .values_mut()
                .flat_map(|workspace| workspace.servers.values_mut())
                .filter_map(|server| server.client.take())
                .collect::<Vec<_>>();
            state.workspaces.clear();
            clients
        };
        for client in clients {
            client.shutdown().await;
        }
        self.emit_update();
    }

    pub(crate) fn emit_update(&self) {
        let _ = self.updates.send(());
    }
}

pub(super) fn canonical_workspace_root(workspace_root: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf())
}

pub(super) struct LspRuntimeState {
    pub(super) workspaces: BTreeMap<PathBuf, LspWorkspaceState>,
    pub(super) catalog: LspServerCatalog,
    pub(super) closed: bool,
}

#[derive(Default)]
pub(super) struct LspWorkspaceState {
    pub(super) servers: BTreeMap<String, LspRuntimeServerState>,
    pub(super) diagnostics: Arc<Mutex<HashMap<String, Vec<LspDiagnostic>>>>,
    pub(super) host: Option<Arc<dyn LspHostBackend>>,
}

/// 单个 workspace member 的运行态：解析后的定义、driver 与探测/连接状态。
pub(super) struct LspRuntimeServerState {
    pub(super) resolved: ResolvedLspServer,
    pub(super) driver: Arc<dyn LspServerDriver>,
    pub(super) availability_kind: LspAvailabilityKind,
    pub(super) availability_message: Option<String>,
    pub(super) last_checked_at: Option<i64>,
    pub(super) client: Option<Arc<LspClient>>,
}

impl LspRuntimeServerState {
    pub(super) fn new(
        resolved: ResolvedLspServer,
        driver: Arc<dyn LspServerDriver>,
        availability_kind: LspAvailabilityKind,
        availability_message: Option<String>,
        last_checked_at: Option<i64>,
    ) -> Self {
        Self {
            resolved,
            driver,
            availability_kind,
            availability_message,
            last_checked_at,
            client: None,
        }
    }

    /// membership 合并时是否保留既有探测结果与 client。
    pub(super) fn preserves_across_reconcile(&self) -> bool {
        self.availability_kind != LspAvailabilityKind::Disabled
    }

    pub(super) fn snapshot(&self, diagnostic_count: usize) -> LspServerSnapshot {
        LspServerSnapshot {
            id: self.resolved.id.clone(),
            display_name: self.resolved.display_name.clone(),
            extensions: self.resolved.extensions.clone(),
            language_ids: self.resolved.language_ids.clone(),
            availability_kind: self.availability_kind.clone(),
            availability_message: self.availability_message.clone(),
            last_checked_at: self.last_checked_at,
            diagnostic_count,
            activity_kind: LspActivityKind::Idle,
            activity_title: None,
            activity_message: None,
            activity_percentage: None,
            last_error: None,
            last_error_at: None,
        }
    }
}
