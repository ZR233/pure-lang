use std::path::Path;
use std::sync::Arc;

use crate::client::LspClient;
use crate::diagnostics::DiagnosticSink;
use crate::server_definition::LspServerDefinition;
use crate::types::{LspAvailabilityKind, LspQuery, LspResult, LspRuntimeError};

use super::LspRuntimeRegistry;

impl LspRuntimeRegistry {
    pub(crate) async fn client_for_query(
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

    pub(crate) async fn server_id_for_query(&self, query: &LspQuery) -> LspResult<String> {
        if let Some(language_id) = &query.language_id {
            self.server_id_for_language(language_id)
                .await
                .ok_or_else(|| {
                    LspRuntimeError::Unavailable(format!(
                        "no LSP server found for language: {language_id}"
                    ))
                })
        } else if let Some(path) = &query.file_path {
            self.server_id_for_path(path).await.ok_or_else(|| {
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

    pub(crate) async fn server_id_for_language(&self, language_id: &str) -> Option<String> {
        self.state
            .lock()
            .await
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

    pub(crate) async fn server_id_for_path(&self, path: &Path) -> Option<String> {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
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

    pub(crate) async fn open_client_for_path(&self, path: &Path) -> Option<Arc<LspClient>> {
        let server_id = self.server_id_for_path(path).await?;
        self.state
            .lock()
            .await
            .servers
            .get(&server_id)
            .and_then(|server| server.client.clone())
    }

    pub(crate) async fn open_clients(&self) -> Vec<Arc<LspClient>> {
        self.state
            .lock()
            .await
            .servers
            .values()
            .filter_map(|server| server.client.clone())
            .collect()
    }
}
