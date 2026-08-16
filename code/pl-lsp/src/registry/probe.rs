//! probe 与 repair：环境探测由 driver 完成，registry 只发布结果。

use std::path::Path;

use crate::driver::LspProbeOutcome;
use crate::types::{LspAvailabilityKind, LspResult, LspRuntimeError};

use super::lsp_query::unix_seconds;
use super::{LspRuntimeRegistry, canonical_workspace_root};

impl LspRuntimeRegistry {
    /// 显式探测一个 workspace 的全部 member server availability。
    ///
    /// Disabled（workspace 检测未命中）的 member 不探测；探测不启动 server 进程。
    pub async fn probe_lsp_server(&self, workspace_root: impl AsRef<Path>) {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        let _lifecycle_guard = self.lifecycle.read().await;
        let targets = {
            let state = self.state.lock().await;
            if state.closed {
                return;
            }
            state.workspaces.get(&workspace_root).map(|workspace| {
                workspace
                    .servers
                    .iter()
                    .filter(|(_, server)| server.availability_kind != LspAvailabilityKind::Disabled)
                    .map(|(server_id, server)| {
                        (
                            server_id.clone(),
                            server.resolved.clone(),
                            server.driver.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
        };
        let Some(targets) = targets else {
            return;
        };
        for (server_id, resolved, driver) in targets {
            let outcome = driver.probe(&resolved.command()).await;
            let (kind, message) = probe_availability(outcome);
            let checked_at = unix_seconds();
            let mut state = self.state.lock().await;
            if state.closed {
                return;
            }
            let Some(server) = state
                .workspaces
                .get_mut(&workspace_root)
                .and_then(|workspace| workspace.servers.get_mut(&server_id))
            else {
                continue;
            };
            if server.resolved.fingerprint() != resolved.fingerprint() {
                continue;
            }
            server.availability_kind = kind;
            server.availability_message = message;
            server.last_checked_at = Some(checked_at);
        }
        self.emit_update();
    }

    /// 仅在 typed missing-component 状态下执行 driver 修复并重新 probe。
    pub async fn repair_lsp_server(
        &self,
        workspace_root: impl AsRef<Path>,
        server_id: &str,
    ) -> LspResult<()> {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        let component = {
            let state = self.state.lock().await;
            if state.closed {
                return Err(LspRuntimeError::Unavailable(
                    "LSP runtime is stopped".to_string(),
                ));
            }
            let server = state
                .workspaces
                .get(&workspace_root)
                .and_then(|workspace| workspace.servers.get(server_id))
                .ok_or_else(|| {
                    LspRuntimeError::Unavailable(format!("LSP server not configured: {server_id}"))
                })?;
            let component = match &server.availability_kind {
                LspAvailabilityKind::MissingServerComponent(component) => Some(component.clone()),
                _ => None,
            };
            let Some(component) = component else {
                return Err(LspRuntimeError::Unavailable(
                    "LSP repair requires missingServerComponent state".to_string(),
                ));
            };
            (component, server.driver.clone())
        };
        let (component, driver) = component;
        let _lifecycle_guard = self.lifecycle.read().await;
        driver
            .repair(&component)
            .await
            .map_err(|error| LspRuntimeError::Unavailable(error.to_string()))?;
        self.probe_lsp_server(workspace_root).await;
        Ok(())
    }
}

/// 把 driver 探测结果映射为 runtime availability。
fn probe_availability(outcome: LspProbeOutcome) -> (LspAvailabilityKind, Option<String>) {
    match outcome {
        LspProbeOutcome::Ready { version } => (LspAvailabilityKind::Available, Some(version)),
        LspProbeOutcome::MissingCommand { message } => {
            (LspAvailabilityKind::MissingCommand, Some(message))
        }
        LspProbeOutcome::MissingComponent(component) => (
            LspAvailabilityKind::MissingServerComponent(component.clone()),
            Some(component.repair_hint),
        ),
        LspProbeOutcome::Failed { message } => (LspAvailabilityKind::Unavailable, Some(message)),
    }
}
