use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use super::builtin::builtin_rust_analyzer;
use super::{LspServerDefinition, LspUserServerConfig};
use crate::driver::LspServerDriver;

/// catalog 中一条 server：纯数据定义加生命周期 driver。
#[derive(Clone)]
pub struct LspCatalogServer {
    pub definition: LspServerDefinition,
    pub driver: Arc<dyn LspServerDriver>,
}

impl std::fmt::Debug for LspCatalogServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LspCatalogServer")
            .field("definition", &self.definition)
            .finish_non_exhaustive()
    }
}

/// catalog 合并/注册的 typed 冲突。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LspCatalogError {
    #[error("duplicate LSP server id `{server_id}`")]
    DuplicateServerId { server_id: String },
    #[error("language `{language_id}` is declared by multiple LSP servers: {servers:?}")]
    ConflictingLanguage {
        language_id: String,
        servers: Vec<String>,
    },
}

/// 内置定义与用户配置合并后的 server catalog。
#[derive(Clone, Default)]
pub struct LspServerCatalog {
    servers: BTreeMap<String, LspCatalogServer>,
}

impl std::fmt::Debug for LspServerCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_set()
            .entries(self.servers.values().map(|server| &server.definition))
            .finish()
    }
}

impl LspServerCatalog {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn builtin() -> Self {
        let mut catalog = Self::empty();
        let _ = catalog.insert(builtin_rust_analyzer());
        catalog
    }

    pub fn with_user_servers(
        user_servers: &BTreeMap<String, LspUserServerConfig>,
    ) -> Result<Self, LspCatalogError> {
        let mut catalog = Self::builtin();
        for (server_id, config) in user_servers {
            catalog.insert_checked(config.to_catalog_server(server_id))?;
        }
        Ok(catalog)
    }

    pub fn insert(&mut self, server: LspCatalogServer) -> Result<(), LspCatalogError> {
        if self.servers.contains_key(&server.definition.id) {
            return Err(LspCatalogError::DuplicateServerId {
                server_id: server.definition.id,
            });
        }
        self.servers.insert(server.definition.id.clone(), server);
        Ok(())
    }

    fn insert_checked(&mut self, server: LspCatalogServer) -> Result<(), LspCatalogError> {
        let id = server.definition.id.clone();
        for language_id in &server.definition.language_ids {
            let mut conflicting = self
                .servers
                .values()
                .filter(|existing| existing.definition.language_ids.contains(language_id))
                .map(|existing| existing.definition.id.clone())
                .collect::<Vec<_>>();
            if !conflicting.is_empty() {
                conflicting.push(id);
                conflicting.sort();
                return Err(LspCatalogError::ConflictingLanguage {
                    language_id: language_id.clone(),
                    servers: conflicting,
                });
            }
        }
        self.insert(server)
    }

    pub fn get(&self, server_id: &str) -> Option<&LspCatalogServer> {
        self.servers.get(server_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &LspCatalogServer> {
        self.servers.values()
    }

    pub fn fingerprint(&self) -> String {
        self.servers
            .values()
            .map(|server| server.definition.fingerprint())
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn workspace_fingerprint(&self, workspace_root: &Path) -> String {
        let detection = self
            .servers
            .values()
            .map(|server| {
                format!(
                    "{}={}",
                    server.definition.id,
                    server.definition.matches_workspace(workspace_root)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{}#{detection}", self.fingerprint())
    }
}
