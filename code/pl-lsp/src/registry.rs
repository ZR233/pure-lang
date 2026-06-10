use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::process::Command;
use tokio::sync::{Mutex, broadcast};

use crate::client::{
    DiagnosticSink, LspClient, LspServerDefinition, with_content_modified_retries,
};
use crate::formatting::{format_diagnostics, format_lsp_result};
use crate::types::{
    LspAvailabilityKind, LspDiagnostic, LspQuery, LspQueryOperation, LspQueryResult, LspResult,
    LspRuntimeError, LspServerSnapshot,
};
use crate::uri::path_to_file_uri;

const RUST_ANALYZER_ID: &str = "rust-analyzer";
const RUST_ANALYZER_COMMAND: &str = "rust-analyzer";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

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

    pub async fn snapshots(&self) -> Vec<LspServerSnapshot> {
        let diagnostics = self.diagnostics.lock().await;
        let diagnostic_counts = diagnostic_counts(&diagnostics);
        drop(diagnostics);
        self.state
            .lock()
            .await
            .servers
            .values()
            .map(|server| {
                server.snapshot(*diagnostic_counts.get(&server.definition.id).unwrap_or(&0))
            })
            .collect()
    }

    pub async fn query(&self, query: LspQuery) -> LspResult<LspQueryResult> {
        if query.operation == LspQueryOperation::Diagnostics {
            return Ok(self.query_diagnostics(query).await);
        }
        let (server_id, definition, client) = self.client_for_query(&query).await?;
        if let Some(path) = query.file_path.as_deref() {
            client.open_or_change_file(path).await?;
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
        let Some(client) = self.open_client_for_path(path).await else {
            return;
        };
        if path.is_file() {
            if client.open_or_change_file(path).await.is_ok() {
                let _ = client.save_file(path).await;
            }
        } else {
            let _ = client.close_file(path).await;
        }
    }

    pub async fn notify_file_deleted(&self, path: impl AsRef<Path>) {
        if let Some(client) = self.open_client_for_path(path.as_ref()).await {
            let _ = client.close_file(path.as_ref()).await;
        }
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
            match probe_command(command).await {
                Ok(version) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::Available,
                    Some(version),
                    Some(unix_seconds()),
                ),
                Err(ProbeError::MissingCommand) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::MissingCommand,
                    Some("rust-analyzer command not found".to_string()),
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

    async fn client_for_query(
        &self,
        query: &LspQuery,
    ) -> LspResult<(String, LspServerDefinition, Arc<LspClient>)> {
        let server_id = self.server_id_for_query(query).await?;
        let mut state = self.state.lock().await;
        let server = state.servers.get_mut(&server_id).ok_or_else(|| {
            LspRuntimeError::Unavailable(format!("LSP server not configured: {server_id}"))
        })?;
        if server.availability_kind != LspAvailabilityKind::Available {
            return Err(LspRuntimeError::Unavailable(
                server
                    .availability_message
                    .clone()
                    .unwrap_or_else(|| format!("{server_id} is not available")),
            ));
        }
        let definition = server.definition.clone();
        let client = match &server.client {
            Some(client) => client.clone(),
            None => {
                let sink = DiagnosticSink::new(
                    definition.id.clone(),
                    definition.workspace_root.clone(),
                    self.diagnostics.clone(),
                    self.updates.clone(),
                );
                let client = Arc::new(LspClient::new(definition.clone(), sink));
                server.client = Some(client.clone());
                client
            }
        };
        Ok((server_id, definition, client))
    }

    async fn server_id_for_query(&self, query: &LspQuery) -> LspResult<String> {
        if query.operation.requires_file() {
            let path = query
                .file_path
                .as_deref()
                .ok_or_else(|| LspRuntimeError::InvalidQuery("filePath is required".to_string()))?;
            self.server_id_for_path(path).await.ok_or_else(|| {
                LspRuntimeError::Unavailable(format!(
                    "No LSP server available for {}",
                    path.display()
                ))
            })
        } else {
            self.active_server_names()
                .await
                .into_iter()
                .next()
                .ok_or_else(|| LspRuntimeError::Unavailable("No active LSP servers".to_string()))
        }
    }

    async fn server_id_for_path(&self, path: &Path) -> Option<String> {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        let extension = format!(".{extension}");
        self.state
            .lock()
            .await
            .servers
            .iter()
            .find(|(_, server)| {
                server
                    .definition
                    .extensions
                    .iter()
                    .any(|item| item == &extension)
            })
            .map(|(server_id, _)| server_id.clone())
    }

    async fn open_client_for_path(&self, path: &Path) -> Option<Arc<LspClient>> {
        let server_id = self.server_id_for_path(path).await?;
        self.state
            .lock()
            .await
            .servers
            .get(&server_id)
            .and_then(|server| server.client.clone())
    }

    async fn query_diagnostics(&self, query: LspQuery) -> LspQueryResult {
        let max_results = query.max_results.unwrap_or(100);
        let mut diagnostics = self.all_diagnostics().await;
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
        LspQueryResult {
            success: true,
            operation: LspQueryOperation::Diagnostics,
            server_id: None,
            result: formatted.text,
            result_count: formatted.result_count,
            file_count: formatted.file_count,
        }
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
        }
    }
}

async fn request_for_query(client: &Arc<LspClient>, query: &LspQuery) -> LspResult<Value> {
    let (method, params) = method_and_params(query)?;
    let mut value = with_content_modified_retries(|| {
        let client = client.clone();
        let method = method.clone();
        let params = params.clone();
        async move { client.request(&method, params).await }
    })
    .await?;
    if matches!(
        query.operation,
        LspQueryOperation::IncomingCalls | LspQueryOperation::OutgoingCalls
    ) {
        let Some(items) = value.as_array() else {
            return Ok(Value::Array(Vec::new()));
        };
        let Some(item) = items.first() else {
            return Ok(Value::Array(Vec::new()));
        };
        let method = match query.operation {
            LspQueryOperation::IncomingCalls => "callHierarchy/incomingCalls",
            LspQueryOperation::OutgoingCalls => "callHierarchy/outgoingCalls",
            _ => unreachable!(),
        };
        value = with_content_modified_retries(|| {
            let client = client.clone();
            let item = item.clone();
            async move {
                client
                    .request(method, serde_json::json!({ "item": item }))
                    .await
            }
        })
        .await?;
    }
    Ok(value)
}

fn method_and_params(query: &LspQuery) -> LspResult<(String, Value)> {
    let uri = query
        .file_path
        .as_deref()
        .map(path_to_file_uri)
        .unwrap_or_default();
    let position = if query.operation.requires_position() {
        let line = query
            .line
            .ok_or_else(|| LspRuntimeError::InvalidQuery("line is required".to_string()))?;
        let character = query
            .character
            .ok_or_else(|| LspRuntimeError::InvalidQuery("character is required".to_string()))?;
        if line == 0 || character == 0 {
            return Err(LspRuntimeError::InvalidQuery(
                "line and character are 1-based and must be positive".to_string(),
            ));
        }
        Some(serde_json::json!({
            "line": line - 1,
            "character": character - 1,
        }))
    } else {
        None
    };
    let text_document = serde_json::json!({ "uri": uri });
    let output = match query.operation {
        LspQueryOperation::GoToDefinition => (
            "textDocument/definition",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::FindReferences => (
            "textDocument/references",
            serde_json::json!({
                "textDocument": text_document,
                "position": position,
                "context": { "includeDeclaration": true },
            }),
        ),
        LspQueryOperation::Hover => (
            "textDocument/hover",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::DocumentSymbol => (
            "textDocument/documentSymbol",
            serde_json::json!({ "textDocument": text_document }),
        ),
        LspQueryOperation::WorkspaceSymbol => (
            "workspace/symbol",
            serde_json::json!({ "query": query.query.clone().unwrap_or_default() }),
        ),
        LspQueryOperation::GoToImplementation => (
            "textDocument/implementation",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::PrepareCallHierarchy
        | LspQueryOperation::IncomingCalls
        | LspQueryOperation::OutgoingCalls => (
            "textDocument/prepareCallHierarchy",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::Diagnostics => unreachable!("diagnostics does not call LSP server"),
    };
    Ok((output.0.to_string(), output.1))
}

fn rust_analyzer_definition(workspace_root: &Path, command: &str) -> LspServerDefinition {
    LspServerDefinition {
        id: RUST_ANALYZER_ID.to_string(),
        display_name: "rust-analyzer".to_string(),
        command: command.to_string(),
        args: Vec::new(),
        extensions: vec![".rs".to_string()],
        language_ids: vec!["rust".to_string()],
        workspace_root: workspace_root.to_path_buf(),
    }
}

#[derive(Debug, Clone)]
enum ProbeError {
    MissingCommand,
    Failed(String),
}

async fn probe_command(command: &str) -> Result<String, ProbeError> {
    let child = Command::new(command)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProbeError::MissingCommand
            } else {
                ProbeError::Failed(error.to_string())
            }
        })?;
    let output = tokio::time::timeout(PROBE_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| ProbeError::Failed("rust-analyzer --version timed out".to_string()))?
        .map_err(|error| ProbeError::Failed(error.to_string()))?;
    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if version.is_empty() {
            "rust-analyzer is available".to_string()
        } else {
            version
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(ProbeError::Failed(if stderr.is_empty() {
            format!("rust-analyzer exited with {}", output.status)
        } else {
            stderr
        }))
    }
}

fn diagnostic_counts(diagnostics: &HashMap<String, Vec<LspDiagnostic>>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for values in diagnostics.values() {
        for diagnostic in values {
            *counts.entry(diagnostic.server_id.clone()).or_insert(0) += 1;
        }
    }
    counts
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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

    #[test]
    fn position_query_uses_one_based_input() {
        let query = LspQuery {
            operation: LspQueryOperation::Hover,
            file_path: Some(PathBuf::from("src/lib.rs")),
            line: Some(7),
            character: Some(3),
            query: None,
            max_results: None,
        };

        let (_, params) = method_and_params(&query).unwrap();

        assert_eq!(params["position"]["line"], serde_json::json!(6));
        assert_eq!(params["position"]["character"], serde_json::json!(2));
    }
}
