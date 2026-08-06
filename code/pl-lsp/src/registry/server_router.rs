use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::client::LspClient;
use crate::diagnostics::DiagnosticSink;
use crate::server_definition::LspServerDefinition;
use crate::types::{LspAvailabilityKind, LspQuery, LspResult, LspRuntimeError};

use super::{LspRuntimeRegistry, canonical_workspace_root};

impl LspRuntimeRegistry {
    pub(crate) async fn client_for_query_in_workspace(
        &self,
        workspace_root: &Path,
        query: &LspQuery,
    ) -> LspResult<(String, LspServerDefinition, Arc<LspClient>)> {
        let server_id = self
            .server_id_for_query_in_workspace(workspace_root, query)
            .await?;
        let mut state = self.state.lock().await;
        let workspace = state.workspaces.get_mut(workspace_root).ok_or_else(|| {
            LspRuntimeError::Unavailable(format!(
                "LSP workspace is not configured: {}",
                workspace_root.display()
            ))
        })?;
        let diagnostics = workspace.diagnostics.clone();
        let server = workspace.servers.get_mut(&server_id).ok_or_else(|| {
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
                    diagnostics,
                    self.updates.clone(),
                );
                let client = Arc::new(LspClient::new(definition.clone(), sink));
                server.client = Some(client.clone());
                client
            }
        };
        Ok((server_id, definition, client))
    }

    pub(crate) async fn workspace_root_for_query(&self, query: &LspQuery) -> LspResult<PathBuf> {
        let state = self.state.lock().await;
        if let Some(path) = query.file_path.as_deref() {
            let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            return state
                .workspaces
                .keys()
                .filter(|workspace_root| path.starts_with(workspace_root))
                .max_by_key(|workspace_root| workspace_root.components().count())
                .cloned()
                .ok_or_else(|| {
                    LspRuntimeError::Unavailable(format!(
                        "no LSP workspace owns file: {}",
                        path.display()
                    ))
                });
        }
        if state.workspaces.len() == 1 {
            return Ok(state
                .workspaces
                .keys()
                .next()
                .expect("single workspace exists")
                .clone());
        }
        Err(LspRuntimeError::InvalidQuery(
            "file_path is required when multiple LSP workspaces are active".to_string(),
        ))
    }

    #[cfg(test)]
    pub(crate) async fn server_id_for_query(&self, query: &LspQuery) -> LspResult<String> {
        let workspace_root = self.workspace_root_for_query(query).await?;
        self.server_id_for_query_in_workspace(&workspace_root, query)
            .await
    }

    pub(crate) async fn server_id_for_query_in_workspace(
        &self,
        workspace_root: &Path,
        query: &LspQuery,
    ) -> LspResult<String> {
        if let Some(language_id) = &query.language_id {
            self.server_id_for_language_in_workspace(workspace_root, language_id)
                .await
                .ok_or_else(|| {
                    LspRuntimeError::Unavailable(format!(
                        "no LSP server found for language: {language_id}"
                    ))
                })
        } else if let Some(path) = &query.file_path {
            self.server_id_for_path_in_workspace(workspace_root, path)
                .await
                .ok_or_else(|| {
                    LspRuntimeError::Unavailable(format!(
                        "no LSP server found for file: {}",
                        path.display()
                    ))
                })
        } else {
            Err(LspRuntimeError::InvalidQuery(
                "language_id or file_path is required".to_string(),
            ))
        }
    }

    pub(crate) async fn server_id_for_language_in_workspace(
        &self,
        workspace_root: &Path,
        language_id: &str,
    ) -> Option<String> {
        let workspace_root = canonical_workspace_root(workspace_root);
        self.state
            .lock()
            .await
            .workspaces
            .get(&workspace_root)?
            .servers
            .iter()
            .find(|(_, server)| {
                server
                    .definition
                    .language_ids
                    .iter()
                    .any(|item| item == language_id)
            })
            .map(|(server_id, _)| server_id.clone())
    }

    pub(crate) async fn server_id_for_path_in_workspace(
        &self,
        workspace_root: &Path,
        path: &Path,
    ) -> Option<String> {
        let extension = extension_for_path(path);
        let workspace_root = canonical_workspace_root(workspace_root);
        self.state
            .lock()
            .await
            .workspaces
            .get(&workspace_root)?
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

    pub(crate) async fn open_client_for_path(&self, path: &Path) -> Option<Arc<LspClient>> {
        let workspace_root = self.workspace_root_for_path(path).await?;
        let server_id = self
            .server_id_for_path_in_workspace(&workspace_root, path)
            .await?;
        self.state
            .lock()
            .await
            .workspaces
            .get(&workspace_root)?
            .servers
            .get(&server_id)
            .and_then(|server| server.client.clone())
    }

    pub(crate) async fn open_clients_for_path(&self, path: &Path) -> Vec<Arc<LspClient>> {
        let Some(workspace_root) = self.workspace_root_for_path(path).await else {
            return Vec::new();
        };
        self.state
            .lock()
            .await
            .workspaces
            .get(&workspace_root)
            .into_iter()
            .flat_map(|workspace| workspace.servers.values())
            .filter_map(|server| server.client.clone())
            .collect()
    }

    async fn workspace_root_for_path(&self, path: &Path) -> Option<PathBuf> {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.state
            .lock()
            .await
            .workspaces
            .keys()
            .filter(|workspace_root| path.starts_with(workspace_root))
            .max_by_key(|workspace_root| workspace_root.components().count())
            .cloned()
    }
}

fn extension_for_path(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    format!(".{extension}")
}
