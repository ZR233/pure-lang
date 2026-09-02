//! LSP 查询编排：路由 → client → 请求 → 格式化；以及文件变更通知。

use std::path::Path;

use crate::client::uri::path_to_file_uri;
use crate::query::{LspDiagnostic, LspQuery, LspQueryOperation, LspQueryResult};

use super::formatting::{format_diagnostics, format_lsp_result};
use super::request::request_for_query;
use super::{LspAvailabilityKind, LspResult, LspRuntimeRegistry};

impl LspRuntimeRegistry {
    pub async fn query_in_workspace(
        &self,
        workspace_root: impl AsRef<Path>,
        query: LspQuery,
    ) -> LspResult<LspQueryResult> {
        let workspace_root = super::canonical_workspace_root(workspace_root.as_ref());
        if query.operation == LspQueryOperation::Diagnostics {
            return self
                .query_diagnostics_in_workspace(&workspace_root, query)
                .await;
        }
        let (server_id, resolved, client) = self
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
        let formatted = format_lsp_result(query.operation, &value, &resolved.workspace_root);
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
        let _ = client.refresh_document(path).await;
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
        let server_id = if let Some(language_id) = query.language_id.as_deref() {
            let server_id = self
                .server_id_for_language_in_workspace(workspace_root, language_id)
                .await?;
            self.ensure_operation_supported_in_workspace(
                workspace_root,
                &server_id,
                query.operation,
            )
            .await?;
            Some(server_id)
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
}
