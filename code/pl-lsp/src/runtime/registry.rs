//! LSP runtime registry：进程、连接、handler、diagnostics、activity 和 snapshot 的唯一 owner。
//!
//! 其他 runtime 模块围绕这里的 owner 状态实现单一职责的编排步骤。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock, broadcast};

use crate::catalog::LspServerCatalog;
use crate::client::LspClient;
use crate::driver::LspServerDriver;
use crate::host::LspHostBackend;
use crate::query::{LspDiagnostic, LspQuery, LspQueryResult};

use super::server::ResolvedLspServer;
use super::{LspActivityKind, LspAvailabilityKind, LspResult, LspRuntimeError, LspServerSnapshot};

/// 进程内 LSP runtime 的唯一 owner；clone 共享同一份状态。
/// 工作区路径必须由宿主预先解析；registry 不使用本地文件系统重解释远程身份。
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

    /// 关闭全部 workspace 的 language-server client 并确认整树回收。
    ///
    /// 独立 service 的关闭并发发出，任一 hang 不会阻止其它 service 收到 cancel；只有
    /// 确认回收的 client owner 才从 registry 移除。未确认的 owner 保留在 registry 中，
    /// 其进程由 `LspHostProcess` 自有 future / supervisor 租约在任务结束时兜底回收。
    /// 返回 typed `Result`，让 runtime 报告真实失败而不是当作 clean。
    pub async fn shutdown(&self) -> LspResult<()> {
        {
            let mut state = self.state.lock().await;
            state.closed = true;
        }
        let _lifecycle_guard = self.lifecycle.write().await;
        // Snapshot clients before closing; workspaces stay intact so unconfirmed owners
        // are not dropped before the close attempt.
        let clients: Vec<Arc<LspClient>> = {
            let state = self.state.lock().await;
            let mut seen = BTreeSet::new();
            let mut clients = Vec::new();
            for workspace in state.workspaces.values() {
                for server in workspace.servers.values() {
                    if let Some(client) = &server.client
                        && seen.insert(Arc::as_ptr(client) as usize)
                    {
                        clients.push(client.clone());
                    }
                }
            }
            clients
        };
        let outcomes = futures::future::join_all(
            clients
                .iter()
                .cloned()
                .map(|client| async move { client.shutdown().await }),
        )
        .await;
        let mut reclaimed = BTreeSet::new();
        let mut failures = Vec::new();
        for (client, outcome) in clients.iter().zip(outcomes) {
            match outcome {
                Ok(()) => {
                    reclaimed.insert(Arc::as_ptr(client) as usize);
                }
                Err(error) => failures.push(error.to_string()),
            }
        }
        {
            let mut state = self.state.lock().await;
            for workspace in state.workspaces.values_mut() {
                for server in workspace.servers.values_mut() {
                    let reclaimable = server
                        .client
                        .as_ref()
                        .is_some_and(|client| reclaimed.contains(&(Arc::as_ptr(client) as usize)));
                    if reclaimable {
                        server.client = None;
                    }
                }
            }
        }
        self.emit_update();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(LspRuntimeError::Unavailable(format!(
                "{} LSP client(s) did not confirm process-tree reclamation: {}",
                failures.len(),
                failures.join("; ")
            )))
        }
    }

    pub(crate) fn emit_update(&self) {
        let _ = self.updates.send(());
    }
}

pub(super) fn workspace_key(workspace_root: &std::path::Path) -> PathBuf {
    workspace_root.to_path_buf()
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
