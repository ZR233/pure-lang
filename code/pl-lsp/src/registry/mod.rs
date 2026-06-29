use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{Mutex, broadcast};

use crate::client::LspClient;
use crate::formatting::{format_diagnostics, format_lsp_result};
use crate::server_definition::LspServerDefinition;
use crate::types::{
    LanguageToolInfo, LspActivityKind, LspAvailabilityKind, LspDiagnostic, LspQuery,
    LspQueryOperation, LspQueryResult, LspResult, LspServerSnapshot,
};
use crate::uri::path_to_file_uri;

mod lsp_query;
mod rustup;
mod server_router;

use self::lsp_query::{
    diagnostic_counts, extensions_for_language, request_for_query, unix_seconds,
};
use self::rustup::{
    ProbeError, RUST_ANALYZER_COMMAND, missing_rust_analyzer_message, probe_rust_analyzer,
    rust_analyzer_definition,
};
use crate::server_definition::RUST_ANALYZER_ID;

#[derive(Clone)]
pub struct LspRuntimeRegistry {
    state: Arc<Mutex<LspRuntimeState>>,
    diagnostics: Arc<Mutex<HashMap<String, Vec<LspDiagnostic>>>>,
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
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
            updates,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.updates.subscribe()
    }

    pub async fn reconcile_workspace(&self, workspace_root: impl AsRef<Path>) {
        self.reconcile_workspace_with_command(workspace_root.as_ref(), RUST_ANALYZER_COMMAND)
            .await;
    }

    pub async fn active_server_names(&self) -> Vec<String> {
        self.state
            .lock()
            .await
            .servers
            .iter()
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
        for server in state.servers.values() {
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
        result
    }

    pub async fn snapshots(&self) -> Vec<LspServerSnapshot> {
        let diagnostics = self.diagnostics.lock().await;
        let diagnostic_counts = diagnostic_counts(&diagnostics);
        drop(diagnostics);
        let snapshots = self
            .state
            .lock()
            .await
            .servers
            .values()
            .map(|server| {
                (
                    server.snapshot(*diagnostic_counts.get(&server.definition.id).unwrap_or(&0)),
                    server.client.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut output = Vec::with_capacity(snapshots.len());
        for (mut snapshot, client) in snapshots {
            if let Some(client) = client {
                let status = client.runtime_status().await;
                snapshot.activity_kind = status.activity_kind;
                snapshot.activity_title = status.activity_title;
                snapshot.activity_message = status.activity_message;
                snapshot.activity_percentage = status.activity_percentage;
                snapshot.last_error = status.last_error;
                snapshot.last_error_at = status.last_error_at;
            }
            output.push(snapshot);
        }
        output
    }

    pub async fn query(&self, query: LspQuery) -> LspResult<LspQueryResult> {
        if query.operation == LspQueryOperation::Diagnostics {
            return self.query_diagnostics(query).await;
        }
        let (server_id, definition, client) = self.client_for_query(&query).await?;
        if let Some(path) = query.file_path.as_deref() {
            let uri = path_to_file_uri(path);
            let _ = client.open_document(path, &uri).await;
        }
        let value = request_for_query(&client, &query).await?;
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
        for client in self.open_clients().await {
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
        for client in self.open_clients().await {
            let _ = client.file_deleted(path).await;
        }
        if let Some(client) = self.open_client_for_path(path).await {
            let _ = client.close_document(path).await;
        }
    }

    pub async fn shutdown(&self) {
        let clients = {
            let mut state = self.state.lock().await;
            state.workspace_root = None;
            let clients = state
                .servers
                .values_mut()
                .filter_map(|server| server.client.take())
                .collect::<Vec<_>>();
            state.servers.clear();
            clients
        };
        self.diagnostics.lock().await.clear();
        for client in clients {
            client.shutdown().await;
        }
        self.emit_update();
    }

    async fn reconcile_workspace_with_command(&self, workspace_root: &Path, command: &str) {
        let workspace_root =
            std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
        let old_clients = {
            let mut state = self.state.lock().await;
            let workspace_changed = state.workspace_root.as_ref() != Some(&workspace_root);
            if workspace_changed {
                let clients = state
                    .servers
                    .values()
                    .filter_map(|server| server.client.clone())
                    .collect::<Vec<_>>();
                state.servers.clear();
                self.diagnostics.lock().await.clear();
                state.workspace_root = Some(workspace_root.clone());
                clients
            } else {
                Vec::new()
            }
        };
        for client in old_clients {
            client.shutdown().await;
        }

        let definition = rust_analyzer_definition(&workspace_root, command);
        let next = if !workspace_root.join("Cargo.toml").exists() {
            LspRuntimeServerState::new(
                definition,
                LspAvailabilityKind::Disabled,
                Some("No Cargo.toml found in workspace root".to_string()),
                None,
            )
        } else {
            match probe_rust_analyzer(command).await {
                Ok(version) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::Available,
                    Some(version),
                    Some(unix_seconds()),
                ),
                Err(ProbeError::MissingCommand) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::MissingCommand,
                    Some(missing_rust_analyzer_message()),
                    Some(unix_seconds()),
                ),
                Err(ProbeError::MissingRustupComponent) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::MissingCommand,
                    Some(missing_rust_analyzer_message()),
                    Some(unix_seconds()),
                ),
                Err(ProbeError::Failed(message)) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::Unavailable,
                    Some(message),
                    Some(unix_seconds()),
                ),
            }
        };
        {
            let mut state = self.state.lock().await;
            state.servers.insert(RUST_ANALYZER_ID.to_string(), next);
        }
        self.emit_update();
    }

    async fn query_diagnostics(&self, query: LspQuery) -> LspResult<LspQueryResult> {
        let max_results = query.max_results.unwrap_or(100);
        let mut diagnostics = self.all_diagnostics().await;
        let server_id = if query.language_id.is_some() {
            Some(self.server_id_for_query(&query).await?)
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

    async fn all_diagnostics(&self) -> Vec<LspDiagnostic> {
        let mut diagnostics = self
            .diagnostics
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

#[derive(Default)]
struct LspRuntimeState {
    workspace_root: Option<PathBuf>,
    servers: BTreeMap<String, LspRuntimeServerState>,
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

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pure-lsp-{name}-{stamp}"))
    }

    async fn register_available_rust(registry: &LspRuntimeRegistry, workspace_root: &Path) {
        let definition = rust_analyzer_definition(workspace_root, RUST_ANALYZER_COMMAND);
        let mut state = registry.state.lock().await;
        state.workspace_root = Some(workspace_root.to_path_buf());
        state.servers.insert(
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
            .reconcile_workspace_with_command(&dir, "definitely-not-rust-analyzer-pure-test")
            .await;
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
    async fn live_rust_analyzer_document_symbol_query() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let registry = LspRuntimeRegistry::new();

        registry.reconcile_workspace(&workspace_root).await;
        let snapshots = registry.snapshots().await;

        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].availability_kind,
            LspAvailabilityKind::Available,
            "{}",
            snapshots[0].availability_message.as_deref().unwrap_or("")
        );

        let result = registry
            .query(LspQuery {
                operation: LspQueryOperation::DocumentSymbol,
                file_path: Some(workspace_root.join("code/pl-lsp/src/registry.rs")),
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: None,
            })
            .await
            .expect("document symbol query");

        assert!(result.success);
        assert_eq!(result.server_id.as_deref(), Some(RUST_ANALYZER_ID));
        assert!(
            result.result.contains("LspRuntimeRegistry"),
            "{}",
            result.result
        );
        registry.shutdown().await;
    }

    #[tokio::test]
    async fn available_languages_returns_empty_when_no_servers() {
        let registry = LspRuntimeRegistry::new();
        let languages = registry.available_languages().await;
        assert!(languages.is_empty());
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
            .reconcile_workspace_with_command(&dir, "definitely-not-rust-analyzer-pure-test")
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
}
