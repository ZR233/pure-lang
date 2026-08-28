//! LSP server 数据 catalog。
//!
//! [`LspServerDefinition`] 是纯数据：server id、展示名、language ids、workspace
//! 检测规则、command 解析与能力集。catalog 由内置定义与用户配置声明合并而成；
//! 合并冲突以 [`LspCatalogError`] fail-loud，不按注册顺序静默覆盖。

mod matching;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::driver::LspResolvedCommand;
use crate::driver::LspServerDriver;
use crate::driver::command::CommandDriver;
use crate::driver::rust_analyzer::RustAnalyzerDriver;
use crate::types::LspQueryOperation;

pub(crate) use self::matching::glob_match;
use self::matching::workspace_matches;

/// 内置 rust-analyzer server id。
pub const RUST_ANALYZER_ID: &str = "rust-analyzer";

const WORKSPACE_ROOT_PLACEHOLDER: &str = "{workspaceRoot}";

/// catalog 中一条 server 的纯数据定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerDefinition {
    pub id: String,
    pub display_name: String,
    /// 该 server 声明支持的 language id 列表；与 `extensions` 按位对应（可较短）。
    pub language_ids: Vec<String>,
    /// language id 对应的文件扩展名（如 `.rs`）。
    pub extensions: Vec<String>,
    /// workspace 检测规则：相对 workspace root 的文件名或单段 glob；空表示总是匹配。
    pub detection: Vec<String>,
    pub command: LspCommandSpec,
    /// 该 server 支持的 `lsp_query` 操作子集，用于 capabilities 报告与路由校验。
    pub operations: Vec<LspQueryOperation>,
}

impl LspServerDefinition {
    /// 静态检测当前 workspace 是否适用该 server；只做文件系统检查，不运行命令。
    pub fn matches_workspace(&self, workspace_root: &Path) -> bool {
        workspace_matches(&self.detection, workspace_root)
    }

    /// 与 workspace 无关的定义指纹；用于探测/合并结果的新鲜度判断。
    pub fn fingerprint(&self) -> String {
        let operations = self
            .operations
            .iter()
            .map(|operation| operation.as_str())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.id,
            self.display_name,
            self.language_ids.join(","),
            self.extensions.join(","),
            self.detection.join(","),
            operations,
        )
    }
}

/// command 解析：程序与参数模板，`{workspaceRoot}` 会被渲染为 workspace root。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspCommandSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl LspCommandSpec {
    /// 渲染最终启动命令；占位符替换按需最小化，当前仅支持 `{workspaceRoot}`。
    pub fn render(&self, workspace_root: &Path) -> LspResolvedCommand {
        let root = workspace_root.display().to_string();
        LspResolvedCommand {
            program: self.program.replace(WORKSPACE_ROOT_PLACEHOLDER, &root),
            args: self
                .args
                .iter()
                .map(|arg| arg.replace(WORKSPACE_ROOT_PLACEHOLDER, &root))
                .collect(),
        }
    }
}

/// 用户在 `[lsp.servers.<id>]` 中声明的自定义 server 配置。
///
/// 该类型直接映射 `config.toml` 段，serde 键名保持 snake_case（与其余
/// Studio 配置段一致）；catalog 内部模板 [`LspServerDefinition`] 才是
/// camelCase 的结构化协议类型。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspUserServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub language_ids: Vec<String>,
    #[serde(default)]
    pub detection: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    /// 支持的操作子集；缺省（空）表示支持全部 `lsp_query` 操作。
    #[serde(default)]
    pub operations: Vec<LspQueryOperation>,
}

impl LspUserServerConfig {
    /// 转换为 catalog 条目：自定义 server 绑定通用命令 driver。
    pub fn to_catalog_server(&self, id: &str) -> LspCatalogServer {
        LspCatalogServer {
            definition: LspServerDefinition {
                id: id.to_string(),
                display_name: self.display_name.clone().unwrap_or_else(|| id.to_string()),
                language_ids: self.language_ids.clone(),
                extensions: self.extensions.clone(),
                detection: self.detection.clone(),
                command: LspCommandSpec {
                    program: self.command.clone(),
                    args: self.args.clone(),
                },
                operations: if self.operations.is_empty() {
                    LspQueryOperation::all().to_vec()
                } else {
                    self.operations.clone()
                },
            },
            driver: Arc::new(CommandDriver::new()),
        }
    }
}

/// catalog 中一条 server：纯数据定义加生命周期 driver。
#[derive(Clone)]
pub struct LspCatalogServer {
    pub definition: LspServerDefinition,
    pub driver: Arc<dyn LspServerDriver>,
}

impl std::fmt::Debug for LspCatalogServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspCatalogServer")
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
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set()
            .entries(self.servers.values().map(|server| &server.definition))
            .finish()
    }
}

impl LspServerCatalog {
    pub fn empty() -> Self {
        Self::default()
    }

    /// 内置 catalog：当前只收录 rust-analyzer。
    pub fn builtin() -> Self {
        let mut catalog = Self::empty();
        let _ = catalog.insert(builtin_rust_analyzer());
        catalog
    }

    /// 内置 catalog 加用户声明；重复 server id 或 language_id 冲突 fail-loud。
    pub fn with_user_servers(
        user_servers: &BTreeMap<String, LspUserServerConfig>,
    ) -> Result<Self, LspCatalogError> {
        let mut catalog = Self::builtin();
        for (server_id, config) in user_servers {
            catalog.insert_checked(config.to_catalog_server(server_id))?;
        }
        Ok(catalog)
    }

    /// 注册一条 server；同 id 重复注册被拒绝。
    ///
    /// language_id 重叠在此不做全局限制，由路由层以 typed 歧义错误拒绝，
    /// 供宿主与测试构造非内置 catalog。
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

    /// 按 server id 排序的条目迭代。
    pub fn iter(&self) -> impl Iterator<Item = &LspCatalogServer> {
        self.servers.values()
    }

    /// catalog 内容指纹；catalog 变化时 workspace membership 需要重算。
    pub fn fingerprint(&self) -> String {
        self.servers
            .values()
            .map(|server| server.definition.fingerprint())
            .collect::<Vec<_>>()
            .join(";")
    }

    /// catalog × workspace 检测结果的指纹；供宿主判断是否需要重新激活。
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
        format!("{}#{}", self.fingerprint(), detection)
    }
}

fn builtin_rust_analyzer() -> LspCatalogServer {
    LspCatalogServer {
        definition: LspServerDefinition {
            id: RUST_ANALYZER_ID.to_string(),
            display_name: "rust-analyzer".to_string(),
            language_ids: vec!["rust".to_string()],
            extensions: vec![".rs".to_string()],
            detection: vec!["Cargo.toml".to_string()],
            command: LspCommandSpec {
                program: RUST_ANALYZER_ID.to_string(),
                args: Vec::new(),
            },
            operations: LspQueryOperation::all().to_vec(),
        },
        driver: Arc::new(RustAnalyzerDriver::new()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::*;

    fn user_server(language_ids: &[&str]) -> LspUserServerConfig {
        LspUserServerConfig {
            command: "purelang-lsp".to_string(),
            args: vec!["--stdio".to_string()],
            language_ids: language_ids.iter().map(|id| id.to_string()).collect(),
            detection: vec!["pure.toml".to_string()],
            extensions: vec![".pure".to_string()],
            display_name: None,
            operations: Vec::new(),
        }
    }

    #[test]
    fn builtin_catalog_contains_rust_analyzer_with_cargo_detection() {
        let catalog = LspServerCatalog::builtin();

        let server = catalog.get(RUST_ANALYZER_ID).expect("builtin entry");
        assert_eq!(server.definition.language_ids, vec!["rust".to_string()]);
        assert_eq!(server.definition.detection, vec!["Cargo.toml".to_string()]);
        assert_eq!(
            server.definition.operations,
            LspQueryOperation::all().to_vec()
        );
    }

    #[test]
    fn user_servers_merge_with_builtin_catalog() {
        let mut servers = BTreeMap::new();
        servers.insert("purelang".to_string(), user_server(&["purelang"]));

        let catalog = LspServerCatalog::with_user_servers(&servers).unwrap();

        assert_eq!(catalog.iter().count(), 2);
        let merged = catalog.get("purelang").expect("user entry");
        assert_eq!(merged.definition.display_name, "purelang");
        assert_eq!(
            merged.definition.operations,
            LspQueryOperation::all().to_vec()
        );
    }

    #[test]
    fn duplicate_server_id_fails_loud() {
        let mut catalog = LspServerCatalog::builtin();
        let duplicate = user_server(&["purelang"]);

        let error = catalog
            .insert_checked(duplicate.to_catalog_server(RUST_ANALYZER_ID))
            .unwrap_err();

        assert_eq!(
            error,
            LspCatalogError::DuplicateServerId {
                server_id: RUST_ANALYZER_ID.to_string()
            }
        );
    }

    #[test]
    fn language_conflict_with_builtin_fails_loud() {
        let mut servers = BTreeMap::new();
        servers.insert(
            "custom-rust".to_string(),
            LspUserServerConfig {
                command: "other-rust-server".to_string(),
                language_ids: vec!["rust".to_string()],
                ..user_server(&[])
            },
        );

        let error = LspServerCatalog::with_user_servers(&servers).unwrap_err();

        assert_eq!(
            error,
            LspCatalogError::ConflictingLanguage {
                language_id: "rust".to_string(),
                servers: vec!["custom-rust".to_string(), RUST_ANALYZER_ID.to_string()],
            }
        );
    }

    #[test]
    fn language_conflict_between_user_servers_fails_loud() {
        let mut servers = BTreeMap::new();
        servers.insert("first".to_string(), user_server(&["purelang"]));
        servers.insert(
            "second".to_string(),
            LspUserServerConfig {
                command: "another-server".to_string(),
                language_ids: vec!["purelang".to_string()],
                ..user_server(&[])
            },
        );

        let error = LspServerCatalog::with_user_servers(&servers).unwrap_err();

        assert_eq!(
            error,
            LspCatalogError::ConflictingLanguage {
                language_id: "purelang".to_string(),
                servers: vec!["first".to_string(), "second".to_string()],
            }
        );
    }

    #[test]
    fn user_config_operations_subset_replaces_default_all() {
        let config = LspUserServerConfig {
            operations: vec![LspQueryOperation::Hover, LspQueryOperation::DocumentSymbol],
            ..user_server(&["purelang"])
        };

        let server = config.to_catalog_server("purelang");

        assert_eq!(
            server.definition.operations,
            vec![LspQueryOperation::Hover, LspQueryOperation::DocumentSymbol]
        );
    }

    #[test]
    fn command_spec_renders_workspace_root_placeholder() {
        let spec = LspCommandSpec {
            program: "purelang-lsp".to_string(),
            args: vec!["--root".to_string(), "{workspaceRoot}".to_string()],
        };

        let rendered = spec.render(&PathBuf::from("/tmp/demo"));

        assert_eq!(rendered.program, "purelang-lsp");
        assert_eq!(
            rendered.args,
            vec!["--root".to_string(), "/tmp/demo".to_string()]
        );
    }

    #[test]
    fn workspace_fingerprint_changes_with_detection_result() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("pure-lsp-catalog-fp-{stamp}"));
        std::fs::create_dir_all(&root).unwrap();
        let catalog = LspServerCatalog::builtin();

        let without_cargo = catalog.workspace_fingerprint(&root);
        std::fs::write(root.join("Cargo.toml"), "[package]\nname='x'\n").unwrap();
        let with_cargo = catalog.workspace_fingerprint(&root);

        assert_ne!(without_cargo, with_cargo);
        let _ = std::fs::remove_dir_all(root);
    }
}
