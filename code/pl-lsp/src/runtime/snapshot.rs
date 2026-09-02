//! 只读 snapshot：read 路径只克隆 owner 已发布的状态。

use std::path::Path;

use crate::client::LspClientRuntimeStatus;

use super::LspServerSnapshot;

use super::request::diagnostic_counts;
use super::{LspRuntimeRegistry, canonical_workspace_root};

impl LspRuntimeRegistry {
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
                        .map(|server| (server.resolved.id.clone(), server.client.clone()))
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
                server.snapshot(*diagnostic_counts.get(&server.resolved.id).unwrap_or(&0))
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
}

fn apply_client_status(snapshot: &mut LspServerSnapshot, status: LspClientRuntimeStatus) {
    snapshot.activity_kind = status.activity_kind;
    snapshot.activity_title = status.activity_title;
    snapshot.activity_message = status.activity_message;
    snapshot.activity_percentage = status.activity_percentage;
    snapshot.last_error = status.last_error;
    snapshot.last_error_at = status.last_error_at;
}
