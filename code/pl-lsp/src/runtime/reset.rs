//! LSP reset：目标 client 重启或回到未启动状态。

use crate::client::{DiagnosticSink, LspClient};

use super::{LspAvailabilityKind, LspResult, LspRuntimeError, LspRuntimeRegistry, LspScope};

impl LspRuntimeRegistry {
    /// 重置目标 client；registry 保持可用，shutdown 才进入终止态。
    pub async fn reset_lsp(&self, scope: LspScope) -> LspResult<()> {
        let _lifecycle_guard = self.lifecycle.write().await;
        let targets = {
            let mut state = self.state.lock().await;
            if state.closed {
                return Err(LspRuntimeError::Unavailable(
                    "LSP runtime is stopped".to_string(),
                ));
            }
            let mut targets = Vec::new();
            for (workspace_root, workspace) in &mut state.workspaces {
                if !scope_matches_workspace(&scope, workspace_root) {
                    continue;
                }
                for (server_id, server) in &mut workspace.servers {
                    if let LspScope::Server {
                        server_id: target, ..
                    } = &scope
                        && target != server_id
                    {
                        continue;
                    }
                    targets.push((
                        workspace_root.clone(),
                        server_id.clone(),
                        server.resolved.clone(),
                        server.driver.clone(),
                        workspace.diagnostics.clone(),
                        workspace.host.clone(),
                        server.client.take(),
                    ));
                }
                workspace.diagnostics.lock().await.clear();
            }
            targets
        };
        for (workspace_root, server_id, resolved, driver, diagnostics, host, previous) in targets {
            let restart = previous.is_some();
            if let Some(client) = previous {
                client.shutdown().await;
            }
            if !restart {
                continue;
            }
            let sink = DiagnosticSink::new(
                resolved.id.clone(),
                resolved.workspace_root.clone(),
                diagnostics,
                self.updates.clone(),
            );
            let client = LspClient::new(resolved, driver, sink, host);
            let client = std::sync::Arc::new(client);
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
}

fn scope_matches_workspace(scope: &LspScope, workspace_root: &std::path::Path) -> bool {
    match scope {
        LspScope::All => true,
        LspScope::Workspace {
            workspace_root: target,
        }
        | LspScope::Server {
            workspace_root: target,
            ..
        } => super::canonical_workspace_root(target) == workspace_root,
    }
}
