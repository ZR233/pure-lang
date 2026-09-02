//! capabilities 与语言投影：catalog × workspace 检测 × 运行态。

use std::path::Path;

use super::{
    LanguageToolInfo, LspAvailabilityKind, LspRuntimeRegistry, LspWorkspaceCapabilities,
    LspWorkspaceServerCapabilities, LspWorkspaceState, canonical_workspace_root,
};

use super::request::extensions_for_language;

impl LspRuntimeRegistry {
    pub async fn active_server_names(&self) -> Vec<String> {
        let state = self.state.lock().await;
        let mut names = state
            .workspaces
            .values()
            .flat_map(active_server_names_for_state)
            .collect::<Vec<_>>();
        normalize_server_names(&mut names);
        names
    }

    pub async fn active_server_names_for_workspace(
        &self,
        workspace_root: impl AsRef<Path>,
    ) -> Vec<String> {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        let mut names = self
            .state
            .lock()
            .await
            .workspaces
            .get(&workspace_root)
            .map(active_server_names_for_state)
            .unwrap_or_default();
        normalize_server_names(&mut names);
        names
    }

    /// 返回当前 Available 状态的所有语言工具信息。
    ///
    /// 每种已注册且可用的 LSP 服务器会按其 `language_ids` 逐一展开。
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

    /// 返回一个 workspace 的能力投影（`lsp_capabilities` 工具输入）。
    ///
    /// server 列表由 catalog × workspace 检测动态产出；availability 与 ready
    /// 反映运行态探测结果，未 reconcile 的 workspace 按 checking/disabled 推导。
    pub async fn capabilities_for_workspace(
        &self,
        workspace_root: impl AsRef<Path>,
    ) -> LspWorkspaceCapabilities {
        let workspace_root = canonical_workspace_root(workspace_root.as_ref());
        let (catalog, observed) = {
            let state = self.state.lock().await;
            let observed = state
                .workspaces
                .get(&workspace_root)
                .map(|workspace| {
                    workspace
                        .servers
                        .iter()
                        .map(|(server_id, server)| {
                            (server_id.clone(), server.availability_kind.clone())
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (state.catalog.clone(), observed)
        };
        let servers = catalog
            .iter()
            .map(|server| project_server_capabilities(server, &workspace_root, &observed))
            .collect::<Vec<_>>();
        LspWorkspaceCapabilities { servers }
    }
}

fn active_server_names_for_state(workspace: &LspWorkspaceState) -> Vec<String> {
    workspace
        .servers
        .iter()
        .filter(|(_, server)| server.availability_kind == LspAvailabilityKind::Available)
        .map(|(id, _)| id.clone())
        .collect()
}

fn normalize_server_names(names: &mut Vec<String>) {
    names.sort();
    names.dedup();
}

fn project_server_capabilities(
    server: &crate::catalog::LspCatalogServer,
    workspace_root: &std::path::Path,
    observed: &[(String, LspAvailabilityKind)],
) -> LspWorkspaceServerCapabilities {
    let matched = server.definition.matches_workspace(workspace_root);
    let availability_kind = observed
        .iter()
        .find(|(server_id, _)| server_id == &server.definition.id)
        .map(|(_, kind)| kind.clone())
        .unwrap_or(if matched {
            LspAvailabilityKind::Checking
        } else {
            LspAvailabilityKind::Disabled
        });
    LspWorkspaceServerCapabilities {
        id: server.definition.id.clone(),
        display_name: server.definition.display_name.clone(),
        language_ids: server.definition.language_ids.clone(),
        operations: server
            .definition
            .operations
            .iter()
            .map(|operation| operation.as_str().to_string())
            .collect(),
        availability: availability_kind.as_str().to_string(),
        ready: availability_kind == LspAvailabilityKind::Available,
    }
}

fn append_available_languages(result: &mut Vec<LanguageToolInfo>, workspace: &LspWorkspaceState) {
    for server in workspace.servers.values() {
        if server.availability_kind != LspAvailabilityKind::Available {
            continue;
        }
        for language_id in &server.resolved.language_ids {
            result.push(LanguageToolInfo {
                language_id: language_id.clone(),
                server_id: server.resolved.id.clone(),
                display_name: server.resolved.display_name.clone(),
                extensions: extensions_for_language(&server.resolved, language_id),
            });
        }
    }
}
