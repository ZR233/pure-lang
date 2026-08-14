use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock, broadcast};

use crate::client::LspClient;
use crate::diagnostics::DiagnosticSink;
use crate::formatting::{format_diagnostics, format_lsp_result};
use crate::server_definition::LspServerDefinition;
use crate::status::LspClientRuntimeStatus;
use crate::types::{
    LanguageToolInfo, LspActivityKind, LspAvailabilityKind, LspDiagnostic, LspQuery,
    LspQueryOperation, LspQueryResult, LspResult, LspScope, LspServerSnapshot,
};
use crate::uri::path_to_file_uri;

mod lsp_query;
mod rustup;
mod server_router;

use self::lsp_query::{
    diagnostic_counts, extensions_for_language, request_for_query, unix_seconds,
};
use self::rustup::{
    ProbeError, RUST_ANALYZER_COMMAND, install_rust_analyzer_component,
    missing_rust_analyzer_message, probe_rust_analyzer, rust_analyzer_definition,
    rustup_is_available,
};
use crate::server_definition::RUST_ANALYZER_ID;

#[derive(Clone)]
pub struct LspRuntimeRegistry {
    state: Arc<Mutex<LspRuntimeState>>,
    lifecycle: Arc<RwLock<()>>,
    updates: broadcast::Sender<()>,
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
    pub fn new() -> Self {
        let (updates, _) = broadcast::channel(64);
        Self {
            state: Arc::new(Mutex::new(LspRuntimeState::default())),
            lifecycle: Arc::new(RwLock::new(())),
            updates,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.updates.subscribe()
    }

    /// 只更新 workspace/server membership，不启动任何进程或执行 probe。
    pub async fn reconcile_workspace_membership(&self, workspace_root: impl AsRef<Path>) {
        self.reconcile_workspace_membership_with_command(
            workspace_root.as_ref(),
            RUST_ANALYZER_COMMAND,
        )
        .await;
    }

    /// 显式探测一个 workspace 的 rust-analyzer availability。
    pub async fn probe_lsp_server(&self, workspace_root: impl AsRef<Path>) {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        let _lifecycle_guard = self.lifecycle.read().await;
        let definition = {
            let state = self.state.lock().await;
            if state.closed {
                return;
            }
            state
                .workspaces
                .get(&workspace_root)
                .and_then(|workspace| workspace.servers.get(RUST_ANALYZER_ID))
                .map(|server| server.definition.clone())
        };
        let Some(definition) = definition else {
            return;
        };
        let outcome = probe_rust_analyzer(&definition.command).await;
        let checked_at = unix_seconds();
        let (kind, message) = match outcome {
            Ok(version) => (LspAvailabilityKind::Available, Some(version)),
            Err(ProbeError::MissingCommand) => (
                LspAvailabilityKind::MissingCommand,
                Some(missing_rust_analyzer_message()),
            ),
            Err(ProbeError::MissingRustupComponent) => (
                LspAvailabilityKind::MissingRustupComponent,
                Some("rust-analyzer rustup component is missing".to_string()),
            ),
            Err(ProbeError::Failed(message)) => (LspAvailabilityKind::Unavailable, Some(message)),
        };
        let mut state = self.state.lock().await;
        if state.closed {
            return;
        }
        let Some(server) = state
            .workspaces
            .get_mut(&workspace_root)
            .and_then(|workspace| workspace.servers.get_mut(RUST_ANALYZER_ID))
        else {
            return;
        };
        if server.definition.command != definition.command {
            return;
        }
        server.availability_kind = kind;
        server.availability_message = message;
        server.last_checked_at = Some(checked_at);
        drop(state);
        self.emit_update();
    }

    /// 仅在 typed missing-component 状态下安装 rust-analyzer 并重新 probe。
    pub async fn repair_lsp_server(
        &self,
        workspace_root: impl AsRef<Path>,
        server_id: &str,
    ) -> LspResult<()> {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        {
            let state = self.state.lock().await;
            if state.closed {
                return Err(crate::types::LspRuntimeError::Unavailable(
                    "LSP runtime is stopped".to_string(),
                ));
            }
            let server = state
                .workspaces
                .get(&workspace_root)
                .and_then(|workspace| workspace.servers.get(server_id))
                .ok_or_else(|| {
                    crate::types::LspRuntimeError::Unavailable(format!(
                        "LSP server not configured: {server_id}"
                    ))
                })?;
            if server.availability_kind != LspAvailabilityKind::MissingRustupComponent {
                return Err(crate::types::LspRuntimeError::Unavailable(
                    "LSP repair requires missingRustupComponent state".to_string(),
                ));
            }
        }
        let _lifecycle_guard = self.lifecycle.read().await;
        if !rustup_is_available().await {
            return Err(crate::types::LspRuntimeError::Unavailable(
                "rustup was not found on PATH".to_string(),
            ));
        }
        install_rust_analyzer_component()
            .await
            .map_err(|error| crate::types::LspRuntimeError::Unavailable(format!("{error:?}")))?;
        drop(_lifecycle_guard);
        self.probe_lsp_server(workspace_root).await;
        Ok(())
    }

    /// 重置目标 client；registry 保持可用，shutdown 才进入终止态。
    pub async fn reset_lsp(&self, scope: LspScope) -> LspResult<()> {
        let _lifecycle_guard = self.lifecycle.write().await;
        let targets = {
            let mut state = self.state.lock().await;
            if state.closed {
                return Err(crate::types::LspRuntimeError::Unavailable(
                    "LSP runtime is stopped".to_string(),
                ));
            }
            let mut targets = Vec::new();
            for (workspace_root, workspace) in &mut state.workspaces {
                let workspace_matches = match &scope {
                    LspScope::All => true,
                    LspScope::Workspace {
                        workspace_root: target,
                    }
                    | LspScope::Server {
                        workspace_root: target,
                        ..
                    } => canonical_workspace_root(target) == *workspace_root,
                };
                if !workspace_matches {
                    continue;
                }
                for (server_id, server) in &mut workspace.servers {
                    if matches!(
                        &scope,
                        LspScope::Server {
                            server_id: target,
                            ..
                        } if target != server_id
                    ) {
                        continue;
                    }
                    targets.push((
                        workspace_root.clone(),
                        server_id.clone(),
                        server.definition.clone(),
                        workspace.diagnostics.clone(),
                        server.client.take(),
                    ));
                }
                workspace.diagnostics.lock().await.clear();
            }
            targets
        };
        for (workspace_root, server_id, definition, diagnostics, previous) in targets {
            let restart = previous.is_some();
            if let Some(client) = previous {
                client.shutdown().await;
            }
            if !restart {
                continue;
            }
            let sink = DiagnosticSink::new(
                definition.id.clone(),
                definition.workspace_root.clone(),
                diagnostics,
                self.updates.clone(),
            );
            let client = Arc::new(LspClient::new(definition, sink));
            let start_result = client.start().await;
            let mut state = self.state.lock().await;
            let Some(server) = state
                .workspaces
                .get_mut(&workspace_root)
                .and_then(|workspace| workspace.servers.get_mut(&server_id))
            else {
                client.shutdown().await;
                continue;
            };
            match start_result {
                Ok(()) => server.client = Some(client),
                Err(error) => {
                    server.availability_kind = LspAvailabilityKind::Unavailable;
                    server.availability_message = Some(error.to_string());
                }
            }
        }
        self.emit_update();
        Ok(())
    }

    pub async fn active_server_names(&self) -> Vec<String> {
        let state = self.state.lock().await;
        let mut names = state
            .workspaces
            .values()
            .flat_map(|workspace| workspace.servers.iter())
            .filter(|(_, server)| server.availability_kind == LspAvailabilityKind::Available)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    }

    pub async fn active_server_names_for_workspace(
        &self,
        workspace_root: impl AsRef<Path>,
    ) -> Vec<String> {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        self.state
            .lock()
            .await
            .workspaces
            .get(&workspace_root)
            .into_iter()
            .flat_map(|workspace| workspace.servers.iter())
            .filter(|(_, server)| server.availability_kind == LspAvailabilityKind::Available)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 返回当前 Available 状态的所有语言工具信息。
    ///
    /// 每种已注册且可用的 LSP 服务器会按其 `language_ids` 逐一展开，
    /// 便于 `pl-core` 为每个语言生成独立的查询工具。
    pub async fn available_languages(&self) -> Vec<LanguageToolInfo> {
        let state = self.state.lock().await;
        let mut result = Vec::new();
        for workspace in state.workspaces.values() {
            append_available_languages(&mut result, workspace);
        }
        result.sort_by(|left, right| {
            left.language_id
                .cmp(&right.language_id)
                .then(left.server_id.cmp(&right.server_id))
        });
        result.dedup_by(|left, right| {
            left.language_id == right.language_id && left.server_id == right.server_id
        });
        result
    }

    pub async fn available_languages_for_workspace(
        &self,
        workspace_root: impl AsRef<Path>,
    ) -> Vec<LanguageToolInfo> {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        let state = self.state.lock().await;
        let mut result = Vec::new();
        if let Some(workspace) = state.workspaces.get(&workspace_root) {
            append_available_languages(&mut result, workspace);
        }
        result
    }

    pub async fn snapshots_for_workspace(
        &self,
        workspace_root: impl AsRef<Path>,
    ) -> Vec<LspServerSnapshot> {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        let workspace = {
            let state = self.state.lock().await;
            state.workspaces.get(&workspace_root).map(|workspace| {
                (
                    workspace.diagnostics.clone(),
                    workspace
                        .servers
                        .values()
                        .map(|server| (server.definition.id.clone(), server.client.clone()))
                        .collect::<Vec<_>>(),
                )
            })
        };
        let Some((diagnostics, clients)) = workspace else {
            return Vec::new();
        };
        let diagnostics = diagnostics.lock().await;
        let diagnostic_counts = diagnostic_counts(&diagnostics);
        drop(diagnostics);
        let state = self.state.lock().await;
        let Some(workspace) = state.workspaces.get(&workspace_root) else {
            return Vec::new();
        };
        let mut snapshots = workspace
            .servers
            .values()
            .map(|server| {
                server.snapshot(*diagnostic_counts.get(&server.definition.id).unwrap_or(&0))
            })
            .collect::<Vec<_>>();
        drop(state);
        for snapshot in &mut snapshots {
            let client = clients
                .iter()
                .find(|(server_id, _)| server_id == &snapshot.id)
                .and_then(|(_, client)| client.clone());
            if let Some(client) = client {
                apply_client_status(snapshot, client.runtime_status().await);
            }
        }
        snapshots
    }

    pub async fn snapshots(&self) -> Vec<LspServerSnapshot> {
        let workspace_roots = self
            .state
            .lock()
            .await
            .workspaces
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut output = Vec::new();
        for workspace_root in workspace_roots {
            output.extend(self.snapshots_for_workspace(workspace_root).await);
        }
        output
    }

    pub async fn query(&self, query: LspQuery) -> LspResult<LspQueryResult> {
        let workspace_root = self.workspace_root_for_query(&query).await?;
        self.query_in_workspace(workspace_root, query).await
    }

    pub async fn query_in_workspace(
        &self,
        workspace_root: impl AsRef<Path>,
        query: LspQuery,
    ) -> LspResult<LspQueryResult> {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        if query.operation == LspQueryOperation::Diagnostics {
            return self
                .query_diagnostics_in_workspace(&workspace_root, query)
                .await;
        }
        let (server_id, definition, client) = self
            .client_for_query_in_workspace(&workspace_root, &query)
            .await?;
        if let Some(path) = query.file_path.as_deref() {
            let uri = path_to_file_uri(path);
            let _ = client.open_document(path, &uri).await;
        }
        let value = match request_for_query(&client, &query).await {
            Ok(value) => value,
            Err(error) => {
                self.mark_client_failure(&workspace_root, &server_id, error.to_string())
                    .await;
                return Err(error);
            }
        };
        let formatted = format_lsp_result(query.operation, &value, &definition.workspace_root);
        Ok(LspQueryResult {
            success: true,
            operation: query.operation,
            server_id: Some(server_id),
            result: formatted.text,
            result_count: formatted.result_count,
            file_count: formatted.file_count,
        })
    }

    pub async fn notify_file_changed(&self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        for client in self.open_clients_for_path(path).await {
            let _ = client.file_changed(path).await;
        }
        let Some(client) = self.open_client_for_path(path).await else {
            return;
        };
        if path.is_file() {
            let _ = client.change_document(path).await;
        } else {
            let _ = client.close_document(path).await;
        }
    }

    pub async fn notify_file_deleted(&self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        for client in self.open_clients_for_path(path).await {
            let _ = client.file_deleted(path).await;
        }
        if let Some(client) = self.open_client_for_path(path).await {
            let _ = client.close_document(path).await;
        }
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

    async fn reconcile_workspace_membership_with_command(
        &self,
        workspace_root: &Path,
        command: &str,
    ) {
        let workspace_root = canonical_workspace_root(workspace_root);
        if self.state.lock().await.closed {
            return;
        }
        let _lifecycle_guard = self.lifecycle.read().await;
        if self.state.lock().await.closed {
            return;
        }

        let definition = rust_analyzer_definition(&workspace_root, command);
        let mut next = if !workspace_root.join("Cargo.toml").exists() {
            LspRuntimeServerState::new(
                definition,
                LspAvailabilityKind::Disabled,
                Some("No Cargo.toml found in workspace root".to_string()),
                None,
            )
        } else {
            LspRuntimeServerState::new(
                definition,
                LspAvailabilityKind::Checking,
                Some("LSP server has not been probed".to_string()),
                None,
            )
        };
        let retired_clients = {
            let mut state = self.state.lock().await;
            if state.closed {
                return;
            }
            let mut retired_clients = state
                .workspaces
                .extract_if(.., |root, _| root != &workspace_root)
                .flat_map(|(_, workspace)| {
                    workspace
                        .servers
                        .into_values()
                        .filter_map(|server| server.client)
                })
                .collect::<Vec<_>>();
            let workspace = state.workspaces.entry(workspace_root).or_default();
            if let Some(current) = workspace.servers.get(RUST_ANALYZER_ID)
                && current.definition.command == next.definition.command
                && current.availability_kind != LspAvailabilityKind::Disabled
            {
                next.availability_kind = current.availability_kind;
                next.availability_message = current.availability_message.clone();
                next.last_checked_at = current.last_checked_at;
                next.client = current.client.clone();
            } else if let Some(client) = workspace
                .servers
                .get_mut(RUST_ANALYZER_ID)
                .and_then(|server| server.client.take())
            {
                retired_clients.push(client);
            }
            workspace.servers.insert(RUST_ANALYZER_ID.to_string(), next);
            retired_clients
        };
        for client in retired_clients {
            client.shutdown().await;
        }
        self.emit_update();
    }

    async fn mark_client_failure(&self, workspace_root: &Path, server_id: &str, message: String) {
        let client = {
            let mut state = self.state.lock().await;
            let Some(server) = state
                .workspaces
                .get_mut(workspace_root)
                .and_then(|workspace| workspace.servers.get_mut(server_id))
            else {
                return;
            };
            server.availability_kind = LspAvailabilityKind::Unavailable;
            server.availability_message = Some(message);
            server.client.take()
        };
        if let Some(client) = client {
            client.shutdown().await;
        }
        self.emit_update();
    }

    async fn query_diagnostics_in_workspace(
        &self,
        workspace_root: &Path,
        query: LspQuery,
    ) -> LspResult<LspQueryResult> {
        let max_results = query.max_results.unwrap_or(100);
        let mut diagnostics = self.all_diagnostics_for_workspace(workspace_root).await;
        let server_id = if query.language_id.is_some() {
            Some(
                self.server_id_for_query_in_workspace(workspace_root, &query)
                    .await?,
            )
        } else {
            None
        };
        if let Some(server_id) = server_id.as_deref() {
            diagnostics.retain(|diagnostic| diagnostic.server_id == server_id);
        }
        if let Some(path) = query.file_path.as_deref() {
            let display = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            diagnostics.retain(|diagnostic| {
                diagnostic.path == path.to_string_lossy() || diagnostic.path.ends_with(&display)
            });
        }
        let formatted = format_diagnostics(&diagnostics, max_results);
        Ok(LspQueryResult {
            success: true,
            operation: LspQueryOperation::Diagnostics,
            server_id,
            result: formatted.text,
            result_count: formatted.result_count,
            file_count: formatted.file_count,
        })
    }

    async fn all_diagnostics_for_workspace(&self, workspace_root: &Path) -> Vec<LspDiagnostic> {
        let diagnostics = self
            .state
            .lock()
            .await
            .workspaces
            .get(workspace_root)
            .map(|workspace| workspace.diagnostics.clone());
        let Some(diagnostics) = diagnostics else {
            return Vec::new();
        };
        let mut diagnostics = diagnostics
            .lock()
            .await
            .values()
            .flat_map(|items| items.iter().cloned())
            .collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.range.start.line.cmp(&right.range.start.line))
                .then(left.range.start.character.cmp(&right.range.start.character))
        });
        diagnostics
    }

    fn emit_update(&self) {
        let _ = self.updates.send(());
    }
}

fn append_available_languages(result: &mut Vec<LanguageToolInfo>, workspace: &LspWorkspaceState) {
    for server in workspace.servers.values() {
        if server.availability_kind != LspAvailabilityKind::Available {
            continue;
        }
        for language_id in &server.definition.language_ids {
            result.push(LanguageToolInfo {
                language_id: language_id.clone(),
                server_id: server.definition.id.clone(),
                display_name: server.definition.display_name.clone(),
                extensions: extensions_for_language(&server.definition, language_id),
            });
        }
    }
}

fn apply_client_status(snapshot: &mut LspServerSnapshot, status: LspClientRuntimeStatus) {
    snapshot.activity_kind = status.activity_kind;
    snapshot.activity_title = status.activity_title;
    snapshot.activity_message = status.activity_message;
    snapshot.activity_percentage = status.activity_percentage;
    snapshot.last_error = status.last_error;
    snapshot.last_error_at = status.last_error_at;
}

fn canonical_workspace_root(workspace_root: &Path) -> PathBuf {
    std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf())
}

#[derive(Default)]
struct LspRuntimeState {
    workspaces: BTreeMap<PathBuf, LspWorkspaceState>,
    closed: bool,
}

#[derive(Default)]
struct LspWorkspaceState {
    servers: BTreeMap<String, LspRuntimeServerState>,
    diagnostics: Arc<Mutex<HashMap<String, Vec<LspDiagnostic>>>>,
}

struct LspRuntimeServerState {
    definition: LspServerDefinition,
    availability_kind: LspAvailabilityKind,
    availability_message: Option<String>,
    last_checked_at: Option<i64>,
    client: Option<Arc<LspClient>>,
}

impl LspRuntimeServerState {
    fn new(
        definition: LspServerDefinition,
        availability_kind: LspAvailabilityKind,
        availability_message: Option<String>,
        last_checked_at: Option<i64>,
    ) -> Self {
        Self {
            definition,
            availability_kind,
            availability_message,
            last_checked_at,
            client: None,
        }
    }

    fn snapshot(&self, diagnostic_count: usize) -> LspServerSnapshot {
        LspServerSnapshot {
            id: self.definition.id.clone(),
            display_name: self.definition.display_name.clone(),
            extensions: self.definition.extensions.clone(),
            language_ids: self.definition.language_ids.clone(),
            availability_kind: self.availability_kind,
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::types::LspRuntimeError;

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pure-lsp-{name}-{stamp}"))
    }

    async fn register_available_rust(registry: &LspRuntimeRegistry, workspace_root: &Path) {
        let workspace_root = canonical_workspace_root(workspace_root);
        let definition = rust_analyzer_definition(&workspace_root, RUST_ANALYZER_COMMAND);
        let mut state = registry.state.lock().await;
        state
            .workspaces
            .entry(workspace_root)
            .or_default()
            .servers
            .insert(
                RUST_ANALYZER_ID.to_string(),
                LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::Available,
                    Some("rust-analyzer test".to_string()),
                    Some(1),
                ),
            );
    }

    #[tokio::test]
    async fn missing_rust_analyzer_command_records_snapshot() {
        let dir = temp_dir("missing-command");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();
        let registry = LspRuntimeRegistry::new();

        registry
            .reconcile_workspace_membership_with_command(
                &dir,
                "definitely-not-rust-analyzer-pure-test",
            )
            .await;
        registry.probe_lsp_server(&dir).await;
        let snapshots = registry.snapshots().await;

        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].availability_kind,
            LspAvailabilityKind::MissingCommand
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    #[ignore = "requires rust-analyzer component and starts the language server"]
    async fn live_rust_analyzer_queries_unique_cargo_demo() {
        let workspace = tempfile::tempdir().expect("temporary Cargo demo");
        let workspace_root = workspace.path().to_path_buf();
        let source_root = workspace_root.join("src");
        fs::create_dir_all(&source_root).expect("create demo source directory");
        fs::write(
            workspace_root.join("Cargo.toml"),
            "[package]\nname='pure_lsp_live_demo'\nversion='0.1.0'\nedition='2024'\n",
        )
        .expect("write demo manifest");
        let library = source_root.join("lib.rs");
        fs::write(&library, "pub fn answer() -> i32 {\n    42\n}\n").expect("write demo library");
        let binary = source_root.join("main.rs");
        fs::write(
            &binary,
            "use pure_lsp_live_demo::answer;\n\nfn main() {\n    println!(\"{}\", answer());\n}\n",
        )
        .expect("write demo binary");
        let registry = LspRuntimeRegistry::new();

        registry
            .reconcile_workspace_membership(&workspace_root)
            .await;
        let snapshots = registry.snapshots().await;
        let outcome = async {
            let document_symbols = registry
                .query(live_query(
                    LspQueryOperation::DocumentSymbol,
                    &library,
                    None,
                ))
                .await?;
            let hover = registry
                .query(live_query(LspQueryOperation::Hover, &library, Some((1, 8))))
                .await?;
            let definition = registry
                .query(live_query(
                    LspQueryOperation::GoToDefinition,
                    &binary,
                    Some((4, 20)),
                ))
                .await?;
            let references = registry
                .query(live_query(
                    LspQueryOperation::FindReferences,
                    &binary,
                    Some((4, 20)),
                ))
                .await?;
            Ok::<_, LspRuntimeError>((document_symbols, hover, definition, references))
        }
        .await;
        let process_id = registry
            .client_for_query_in_workspace(
                &canonical_workspace_root(&workspace_root),
                &live_query(LspQueryOperation::DocumentSymbol, &library, None),
            )
            .await
            .expect("live rust-analyzer client")
            .2
            .child_id_for_test()
            .await
            .expect("live rust-analyzer process id");
        registry.shutdown().await;

        #[cfg(windows)]
        assert!(
            !windows_process_is_running(process_id),
            "rust-analyzer process {process_id} survived registry shutdown"
        );
        #[cfg(not(windows))]
        let _ = process_id;

        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].availability_kind,
            LspAvailabilityKind::Available,
            "{}",
            snapshots[0].availability_message.as_deref().unwrap_or("")
        );
        let (document_symbols, hover, definition, references) =
            outcome.expect("live rust-analyzer queries");
        eprintln!("document symbols:\n{}", document_symbols.result);
        eprintln!("hover:\n{}", hover.result);
        eprintln!("definition:\n{}", definition.result);
        eprintln!("references:\n{}", references.result);
        for result in [&document_symbols, &hover, &definition, &references] {
            assert!(result.success);
            assert_eq!(result.server_id.as_deref(), Some(RUST_ANALYZER_ID));
        }
        assert!(document_symbols.result.contains("answer"));
        assert!(hover.result.contains("answer"), "{}", hover.result);
        assert!(
            definition.result.replace('\\', "/").contains("src/lib.rs"),
            "{}",
            definition.result
        );
        assert!(references.result_count.is_some_and(|count| count >= 2));
        let reference_paths = references.result.replace('\\', "/");
        assert!(reference_paths.contains("src/lib.rs"), "{reference_paths}");
        assert!(reference_paths.contains("src/main.rs"), "{reference_paths}");
    }

    fn live_query(
        operation: LspQueryOperation,
        file_path: &Path,
        position: Option<(u32, u32)>,
    ) -> LspQuery {
        LspQuery {
            operation,
            file_path: Some(file_path.to_path_buf()),
            line: position.map(|(line, _)| line),
            character: position.map(|(_, character)| character),
            query: None,
            max_results: None,
            language_id: Some("rust".to_string()),
        }
    }

    #[cfg(windows)]
    fn windows_process_is_running(process_id: u32) -> bool {
        use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        let Ok(process) =
            (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
        else {
            return false;
        };
        let mut exit_code = 0;
        let running = unsafe { GetExitCodeProcess(process, &mut exit_code) }.is_ok()
            && exit_code == STILL_ACTIVE.0 as u32;
        let _ = unsafe { CloseHandle(process) };
        running
    }

    #[tokio::test]
    async fn available_languages_returns_empty_when_no_servers() {
        let registry = LspRuntimeRegistry::new();
        let languages = registry.available_languages().await;
        assert!(languages.is_empty());
    }

    #[tokio::test]
    async fn shutdown_is_terminal_and_reconcile_cannot_publish_a_workspace() {
        let workspace = tempfile::tempdir().expect("temporary workspace");
        fs::write(
            workspace.path().join("Cargo.toml"),
            "[package]\nname='closed_registry'\nversion='0.1.0'\n",
        )
        .expect("write manifest");
        let registry = LspRuntimeRegistry::new();

        registry.shutdown().await;
        registry
            .reconcile_workspace_membership_with_command(
                workspace.path(),
                "command-must-not-run-after-lsp-shutdown",
            )
            .await;

        let state = registry.state.lock().await;
        assert!(state.closed);
        assert!(state.workspaces.is_empty());
    }

    #[tokio::test]
    async fn shutdown_waits_for_in_flight_reconcile_section() {
        let registry = LspRuntimeRegistry::new();
        let reconcile_guard = registry.lifecycle.read().await;
        let shutting_down = registry.clone();
        let shutdown = tokio::spawn(async move { shutting_down.shutdown().await });
        loop {
            if registry.state.lock().await.closed {
                break;
            }
            tokio::task::yield_now().await;
        }

        assert!(!shutdown.is_finished());
        drop(reconcile_guard);
        tokio::time::timeout(std::time::Duration::from_secs(5), shutdown)
            .await
            .expect("shutdown must finish after reconcile releases its lease")
            .expect("shutdown task must not panic");
    }

    #[tokio::test]
    async fn available_languages_returns_empty_when_server_not_available() {
        let dir = temp_dir("available-languages");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();
        let registry = LspRuntimeRegistry::new();
        registry
            .reconcile_workspace_membership_with_command(
                &dir,
                "definitely-not-rust-analyzer-pure-test",
            )
            .await;

        let languages = registry.available_languages().await;
        assert!(languages.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn available_languages_returns_available_server_languages() {
        let dir = temp_dir("available-languages-rust");
        fs::create_dir_all(&dir).unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &dir).await;

        let languages = registry.available_languages().await;

        assert_eq!(
            languages,
            vec![LanguageToolInfo {
                language_id: "rust".to_string(),
                server_id: RUST_ANALYZER_ID.to_string(),
                display_name: "rust-analyzer".to_string(),
                extensions: vec![".rs".to_string()],
            }]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn multiple_workspaces_keep_independent_language_servers_and_route_by_file() {
        let first = temp_dir("workspace-pool-first");
        let second = temp_dir("workspace-pool-second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let first_file = first.join("src/lib.rs");
        let second_file = second.join("src/lib.rs");
        fs::create_dir_all(first_file.parent().unwrap()).unwrap();
        fs::create_dir_all(second_file.parent().unwrap()).unwrap();
        fs::write(&first_file, "pub fn first() {}\n").unwrap();
        fs::write(&second_file, "pub fn second() {}\n").unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &first).await;
        register_available_rust(&registry, &second).await;

        assert_eq!(registry.state.lock().await.workspaces.len(), 2);
        assert_eq!(
            registry
                .available_languages_for_workspace(&first)
                .await
                .len(),
            1
        );
        assert_eq!(
            registry
                .available_languages_for_workspace(&second)
                .await
                .len(),
            1
        );
        let first_root = registry
            .workspace_root_for_query(&LspQuery {
                operation: LspQueryOperation::DocumentSymbol,
                file_path: Some(first_file),
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: Some("rust".to_string()),
            })
            .await
            .unwrap();
        let second_root = registry
            .workspace_root_for_query(&LspQuery {
                operation: LspQueryOperation::DocumentSymbol,
                file_path: Some(second_file),
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: Some("rust".to_string()),
            })
            .await
            .unwrap();

        assert_eq!(first_root, canonical_workspace_root(&first));
        assert_eq!(second_root, canonical_workspace_root(&second));
        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }

    #[tokio::test]
    async fn server_id_for_query_prefers_language_id() {
        let dir = temp_dir("language-route");
        fs::create_dir_all(&dir).unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &dir).await;
        let query = LspQuery {
            operation: LspQueryOperation::WorkspaceSymbol,
            file_path: None,
            line: None,
            character: None,
            query: Some("LspRuntimeRegistry".to_string()),
            max_results: None,
            language_id: Some("rust".to_string()),
        };

        let server_id = registry.server_id_for_query(&query).await.unwrap();

        assert_eq!(server_id, RUST_ANALYZER_ID);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn diagnostics_query_with_language_id_reports_target_server() {
        let dir = temp_dir("diagnostics-language-route");
        fs::create_dir_all(&dir).unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &dir).await;

        let result = registry
            .query(LspQuery {
                operation: LspQueryOperation::Diagnostics,
                file_path: None,
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: Some("rust".to_string()),
            })
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.server_id.as_deref(), Some(RUST_ANALYZER_ID));
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn multiple_workspaces_require_a_file_path_and_do_not_switch_roots() {
        let first = temp_dir("multi-root-required-first");
        let second = temp_dir("multi-root-required-second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &first).await;
        register_available_rust(&registry, &second).await;

        let error = registry
            .workspace_root_for_query(&LspQuery {
                operation: LspQueryOperation::WorkspaceSymbol,
                file_path: None,
                line: None,
                character: None,
                query: Some("symbol".to_string()),
                max_results: None,
                language_id: Some("rust".to_string()),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, LspRuntimeError::InvalidQuery(_)));
        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }

    #[tokio::test]
    async fn nested_workspaces_route_to_the_longest_canonical_root() {
        let outer = temp_dir("nested-root-outer");
        let inner = outer.join("nested");
        fs::create_dir_all(&inner).unwrap();
        let file = inner.join("src/lib.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "fn nested() {}\n").unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &outer).await;
        register_available_rust(&registry, &inner).await;

        let root = registry
            .workspace_root_for_query(&LspQuery {
                operation: LspQueryOperation::DocumentSymbol,
                file_path: Some(file),
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: Some("rust".to_string()),
            })
            .await
            .unwrap();

        assert_eq!(root, canonical_workspace_root(&inner));
        fs::remove_dir_all(outer).unwrap();
    }

    #[tokio::test]
    async fn file_outside_registered_workspaces_is_unavailable() {
        let workspace = temp_dir("outside-route-workspace");
        let outside = temp_dir("outside-route-file").join("lib.rs");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(outside.parent().unwrap()).unwrap();
        fs::write(&outside, "fn outside() {}\n").unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &workspace).await;

        let error = registry
            .workspace_root_for_query(&LspQuery {
                operation: LspQueryOperation::DocumentSymbol,
                file_path: Some(outside.clone()),
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: Some("rust".to_string()),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, LspRuntimeError::Unavailable(_)));
        fs::remove_dir_all(workspace).unwrap();
        fs::remove_dir_all(outside.parent().unwrap()).unwrap();
    }
}
