//! Workspace membership：catalog × 静态检测 → member server 集合。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::catalog::{LspCatalogError, LspCatalogServer, LspServerCatalog, LspUserServerConfig};
use crate::client::LspClient;
use crate::driver::LspServerDriver;
use crate::host::LspHostBackend;
use crate::resolved::ResolvedLspServer;
use crate::types::LspAvailabilityKind;

use super::{
    LspRuntimeRegistry, LspRuntimeServerState, LspWorkspaceState, canonical_workspace_root,
};

impl LspRuntimeRegistry {
    /// 只更新 workspace/server membership，不启动任何进程或执行 probe。
    pub async fn reconcile_workspace_membership(&self, workspace_root: impl AsRef<Path>) {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        if self.state.lock().await.closed {
            return;
        }
        let _lifecycle_guard = self.lifecycle.read().await;
        self.reconcile_membership_locked(&workspace_root, None)
            .await;
    }

    /// 使用宿主文件原语检测 membership，并把后续 LSP 文件/进程操作绑定到该宿主。
    pub async fn reconcile_workspace_membership_with_host(
        &self,
        workspace_root: impl AsRef<Path>,
        host: Arc<dyn LspHostBackend>,
    ) {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        if self.state.lock().await.closed {
            return;
        }
        let _lifecycle_guard = self.lifecycle.read().await;
        self.reconcile_membership_locked(&workspace_root, Some(host))
            .await;
    }

    /// 注册一条额外 server（宿主/测试扩展点）；重复 server id fail-loud。
    ///
    /// language_id 重叠不在注册时限制，由路由层以 typed 歧义错误拒绝。
    pub async fn register_server(&self, server: LspCatalogServer) -> Result<(), LspCatalogError> {
        let workspace_roots = {
            let mut state = self.state.lock().await;
            if state.closed {
                return Ok(());
            }
            state.catalog.insert(server)?;
            state.workspaces.keys().cloned().collect::<Vec<_>>()
        };
        self.reconcile_registered_workspaces(workspace_roots).await;
        Ok(())
    }

    /// 应用用户配置声明的自定义 server：catalog = 内置定义 + 用户声明。
    ///
    /// 重复 server id 或 language_id 冲突以 [`LspCatalogError`] fail-loud。
    pub async fn apply_user_servers(
        &self,
        user_servers: &BTreeMap<String, LspUserServerConfig>,
    ) -> Result<(), LspCatalogError> {
        let catalog = LspServerCatalog::with_user_servers(user_servers)?;
        let workspace_roots = {
            let mut state = self.state.lock().await;
            if state.closed {
                return Ok(());
            }
            state.catalog = catalog;
            state.workspaces.keys().cloned().collect::<Vec<_>>()
        };
        self.reconcile_registered_workspaces(workspace_roots).await;
        Ok(())
    }

    /// catalog × workspace 检测指纹；宿主用它判断是否需要重新激活 membership。
    pub async fn membership_fingerprint(&self, workspace_root: impl AsRef<Path>) -> String {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        let catalog = self.state.lock().await.catalog.clone();
        catalog.workspace_fingerprint(&workspace_root)
    }

    async fn reconcile_registered_workspaces(&self, workspace_roots: Vec<PathBuf>) {
        if workspace_roots.is_empty() {
            return;
        }
        let _lifecycle_guard = self.lifecycle.read().await;
        for workspace_root in workspace_roots {
            let host = self
                .state
                .lock()
                .await
                .workspaces
                .get(&workspace_root)
                .and_then(|workspace| workspace.host.clone());
            self.reconcile_membership_locked(&workspace_root, host)
                .await;
        }
    }

    /// 生命周期锁已持有时执行 membership 合并；只做静态检测，不运行命令。
    async fn reconcile_membership_locked(
        &self,
        workspace_root: &Path,
        host: Option<Arc<dyn LspHostBackend>>,
    ) {
        let catalog = {
            let state = self.state.lock().await;
            if state.closed {
                return;
            }
            state.catalog.clone()
        };
        let mut desired = Vec::new();
        for server in catalog.iter() {
            desired.push(desired_member(server, workspace_root, host.as_deref()).await);
        }
        let retired_clients = {
            let mut state = self.state.lock().await;
            if state.closed {
                return;
            }
            let mut retired_clients = retire_foreign_workspace_clients(&mut state, workspace_root);
            let workspace = state
                .workspaces
                .entry(workspace_root.to_path_buf())
                .or_default();
            let previous_identity = workspace.host.as_ref().map(|host| host.identity());
            let next_identity = host.as_ref().map(|host| host.identity());
            if previous_identity != next_identity {
                for server in workspace.servers.values_mut() {
                    if let Some(client) = server.client.take() {
                        retired_clients.push(client);
                    }
                    if server.availability_kind != LspAvailabilityKind::Disabled {
                        server.availability_kind = LspAvailabilityKind::Checking;
                        server.availability_message =
                            Some("LSP host changed; server must be probed again".to_string());
                        server.last_checked_at = None;
                    }
                }
            }
            workspace.host = host;
            retain_catalog_members(workspace, &desired, &mut retired_clients);
            for member in desired {
                merge_desired_member(workspace, member, &mut retired_clients);
            }
            retired_clients
        };
        for client in retired_clients {
            client.shutdown().await;
        }
        self.emit_update();
    }
}

/// catalog 条目经 driver 解析后的期望 member 状态。
type DesiredMember = (
    ResolvedLspServer,
    Arc<dyn LspServerDriver>,
    LspAvailabilityKind,
    Option<String>,
);

async fn desired_member(
    server: &LspCatalogServer,
    workspace_root: &Path,
    host: Option<&dyn LspHostBackend>,
) -> DesiredMember {
    let resolved = resolve_member(server, workspace_root);
    let matches = match host {
        Some(host) => {
            workspace_matches_host(&server.definition.detection, workspace_root, host).await
        }
        None => server.definition.matches_workspace(workspace_root),
    };
    if matches {
        (
            resolved,
            server.driver.clone(),
            LspAvailabilityKind::Checking,
            Some("LSP server has not been probed".to_string()),
        )
    } else {
        let message = format!(
            "workspace does not match detection rules: {}",
            server.definition.detection.join(", ")
        );
        (
            resolved,
            server.driver.clone(),
            LspAvailabilityKind::Disabled,
            Some(message),
        )
    }
}

async fn workspace_matches_host(
    rules: &[String],
    workspace_root: &Path,
    host: &dyn LspHostBackend,
) -> bool {
    if rules.is_empty() {
        return true;
    }
    let entries = if rules.iter().any(|rule| rule.contains('*')) {
        host.list_directory(workspace_root).await.ok()
    } else {
        None
    };
    for rule in rules {
        if rule.contains('*') {
            if entries.as_ref().is_some_and(|entries| {
                entries
                    .iter()
                    .any(|name| crate::catalog::glob_match(rule, name))
            }) {
                return true;
            }
        } else if host
            .stat(&workspace_root.join(rule))
            .await
            .is_ok_and(|stat| stat.is_some())
        {
            return true;
        }
    }
    false
}

pub(super) fn resolve_member(
    server: &LspCatalogServer,
    workspace_root: &Path,
) -> ResolvedLspServer {
    let command = server
        .driver
        .resolve_command(&server.definition, workspace_root);
    ResolvedLspServer {
        id: server.definition.id.clone(),
        display_name: server.definition.display_name.clone(),
        program: command.program,
        args: command.args,
        extensions: server.definition.extensions.clone(),
        language_ids: server.definition.language_ids.clone(),
        operations: server.definition.operations.clone(),
        workspace_root: workspace_root.to_path_buf(),
    }
}

fn retire_foreign_workspace_clients(
    state: &mut super::LspRuntimeState,
    workspace_root: &Path,
) -> Vec<Arc<LspClient>> {
    state
        .workspaces
        .extract_if(.., |root, _| root != workspace_root)
        .flat_map(|(_, workspace)| {
            workspace
                .servers
                .into_values()
                .filter_map(|server| server.client)
        })
        .collect()
}

/// 移除已不在 catalog 中的 member，并回收其 client。
fn retain_catalog_members(
    workspace: &mut LspWorkspaceState,
    desired: &[DesiredMember],
    retired_clients: &mut Vec<Arc<LspClient>>,
) {
    let retired = workspace.servers.extract_if(.., |server_id, _| {
        desired
            .iter()
            .all(|(resolved, _, _, _)| resolved.id != *server_id)
    });
    for (_, mut server) in retired {
        if let Some(client) = server.client.take() {
            retired_clients.push(client);
        }
    }
}

/// 合并单个期望 member：定义未变化且非 Disabled 时保留探测结果与 client。
fn merge_desired_member(
    workspace: &mut LspWorkspaceState,
    member: DesiredMember,
    retired_clients: &mut Vec<Arc<LspClient>>,
) {
    let (resolved, driver, availability_kind, availability_message) = member;
    let server_id = resolved.id.clone();
    let Some(current) = workspace.servers.get(&server_id) else {
        workspace.servers.insert(
            server_id,
            LspRuntimeServerState::new(
                resolved,
                driver,
                availability_kind,
                availability_message,
                None,
            ),
        );
        return;
    };
    if current.resolved.fingerprint() == resolved.fingerprint()
        && current.preserves_across_reconcile()
    {
        let mut preserved = LspRuntimeServerState::new(
            resolved,
            driver,
            current.availability_kind.clone(),
            current.availability_message.clone(),
            current.last_checked_at,
        );
        preserved.client = current.client.clone();
        workspace.servers.insert(server_id, preserved);
        return;
    }
    if let Some(client) = workspace
        .servers
        .get_mut(&server_id)
        .and_then(|server| server.client.take())
    {
        retired_clients.push(client);
    }
    workspace.servers.insert(
        server_id,
        LspRuntimeServerState::new(
            resolved,
            driver,
            availability_kind,
            availability_message,
            None,
        ),
    );
}
