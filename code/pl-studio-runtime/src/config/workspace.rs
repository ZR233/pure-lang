//! Project 工作空间声明文件（`workspaces/<project-id>.toml`）的 typed 文件层。
//!
//! 一个 Project 对应一个声明文件，文件内容是声明的唯一持久事实源；运行期由 Studio
//! 发布内存 canonical snapshot。本模块只提供纯文件读写：
//!
//! - 它不发布 snapshot，也不观察外部文件变更；磁盘上的外部编辑只有 Studio 通过显式
//!   重载命令才会生效。
//! - [`WorkspaceConfigStore`] 从显式受信任配置目录构建（`<ANYWORK_HOME>/workspaces`），
//!   测试与隔离实例通过临时目录构造，绝不读写真实用户 home。
//! - 受信任配置目录、其下每个已存在组件与目标文件都按 no-follow 语义校验：任一组件为
//!   符号链接或 reparse point 都明确失败，`read_dir`/`create_dir_all` 不会越出受信任根。
//! - 保存经同目录临时文件原子替换；内容 id 与文件名不一致、schema 未知/未来、解析或校验
//!   失败的声明一律明确失败并保留原文件，既不跳过，也不覆盖为默认值。
//!
//! 最近打开时间、关闭状态、会话目录与 worktree ownership 属于运行期动态状态，一律不
//! 写入声明文件。已创建会话的 `workspace_path` 是冻结事实，不因声明编辑而变化。
//!
//! config 不依赖 studio：把 `ProjectRecord` 映射为 [`WorkspaceDeclaration`] 由 Studio
//! 消费方负责。

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use pl_tool::workspace::path_safety::{
    PathSafetyError, is_link_or_reparse, validate_existing_path, validate_path_for_write,
};

use super::ConfigPaths;

/// 当前工作空间声明 schema 版本。
pub const WORKSPACE_DECLARATION_SCHEMA_VERSION: u32 = 1;

const WORKSPACE_DECLARATION_DIR_NAME: &str = "workspaces";
const WORKSPACE_DECLARATION_EXTENSION: &str = "toml";
/// 与存储 id 上限一致；id 必须是单一路径段。
const MAX_WORKSPACE_ID_BYTES: usize = 200;

/// 单个 Project 的 canonical 工作空间声明。
///
/// 字段使用 `snake_case`；`id` 必须等于文件 stem，`ssh_alias` 缺省表示本地项目（远端
/// 项目按 `(ssh_alias, path)` 唯一）。未知字段被显式拒绝：当前 schema 不保留 raw 兼容
/// 映射，避免下一次保存静默丢弃用户内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDeclaration {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_alias: Option<String>,
}

impl WorkspaceDeclaration {
    /// 使用当前 schema 版本构造一个声明；`ssh_alias` 为 `None` 表示本地项目。
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        path: impl Into<String>,
        ssh_alias: Option<String>,
    ) -> Self {
        Self {
            schema_version: WORKSPACE_DECLARATION_SCHEMA_VERSION,
            id: id.into(),
            name: name.into(),
            path: path.into(),
            ssh_alias,
        }
    }
}

/// 工作空间声明文件层的 typed 错误。
///
/// `NotFound` 与 `Corrupt`/`Unsupported`/`Invalid`/`Io` 明确区分：前者表示声明不存在，
/// 其余表示存在但不可用或读写失败，调用方据此决定是创建还是诊断。
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceDeclarationError {
    #[error("workspace declaration not found: {id}")]
    NotFound { id: String },
    #[error("workspace declaration at {} is corrupt: {reason}", .path.display())]
    Corrupt { path: PathBuf, reason: String },
    #[error(
        "workspace declaration at {} uses unsupported schema version {version} (expected {expected})",
        .path.display()
    )]
    Unsupported {
        path: PathBuf,
        version: u32,
        expected: u32,
    },
    #[error("workspace declaration at {} is invalid: {reason}", .path.display())]
    Invalid { path: PathBuf, reason: String },
    #[error("workspace declaration IO failed at {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// `workspaces/<project-id>.toml` 的纯文件层。
///
/// store 只读写磁盘声明文件，不持有也不发布 canonical snapshot；外部文件变更只有在
/// Studio 显式重载时才可能被观察到。`config_dir` 是受信任根，其下每个已存在组件与目标
/// 文件都按 no-follow 语义校验。
#[derive(Debug, Clone)]
pub struct WorkspaceConfigStore {
    config_dir: PathBuf,
    workspaces_dir: PathBuf,
}

impl WorkspaceConfigStore {
    /// 从显式受信任配置目录构造 `<config_dir>/workspaces`。
    ///
    /// 集成侧按 `for_config_dir(actual ANYWORK_HOME)` 调用；`config_dir` 必须显式提供，
    /// 不会回退到真实用户 home。
    pub fn new(config_dir: impl Into<PathBuf>) -> Self {
        let config_dir = config_dir.into();
        let workspaces_dir = config_dir.join(WORKSPACE_DECLARATION_DIR_NAME);
        Self {
            config_dir,
            workspaces_dir,
        }
    }

    /// 与 [`Self::new`] 同义的显式入口；`config_dir` 是产品配置目录本身（非 OS home）。
    pub fn for_config_dir(config_dir: impl Into<PathBuf>) -> Self {
        Self::new(config_dir)
    }

    /// 从显式 OS home 构造 `<home>/.anywork/workspaces`（内部追加 `.anywork`）。
    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        Self::new(ConfigPaths::from_home(home).config_dir().to_path_buf())
    }

    /// 受信任配置目录（声明目录的父级）。
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// 声明文件所在目录。
    pub fn workspaces_dir(&self) -> &Path {
        &self.workspaces_dir
    }

    /// 校验 id 并解析其声明文件路径。
    ///
    /// # Errors
    /// id 必须是非空、≤ 200 字节的安全 ASCII 标识（字母、数字、`-`、`_`、`.`），
    /// 且不得包含 `..` 或路径分隔符。
    pub fn declaration_path(&self, id: &str) -> Result<PathBuf, WorkspaceDeclarationError> {
        validate_workspace_id(id).map_err(|reason| WorkspaceDeclarationError::Invalid {
            path: self.workspaces_dir.clone(),
            reason,
        })?;
        let path = self
            .workspaces_dir
            .join(format!("{id}.{WORKSPACE_DECLARATION_EXTENSION}"));
        if path.parent() != Some(self.workspaces_dir.as_path()) {
            return Err(WorkspaceDeclarationError::Invalid {
                path: self.workspaces_dir.clone(),
                reason: format!("declaration id must resolve inside its directory: {id}"),
            });
        }
        Ok(path)
    }

    /// 读取全部声明；声明目录缺失返回空列表。
    ///
    /// 先按 no-follow 语义校验受信任目录到声明目录的边界。随后逐文件独立解析非隐藏与
    /// 隐藏的 `*.toml`（两者都是声明材料，任一损坏、schema 未知或身份不一致都整体失败，
    /// 不跳过、不覆盖）；非 `*.toml` 条目不是声明材料，忽略。
    pub fn load_all(&self) -> Result<Vec<WorkspaceDeclaration>, WorkspaceDeclarationError> {
        ensure_trusted_root(&self.config_dir)?;
        validate_path_for_write(&self.config_dir, &self.workspaces_dir)
            .map_err(|error| map_boundary_error(&self.workspaces_dir, "", error))?;
        let entries = match fs::read_dir(&self.workspaces_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(WorkspaceDeclarationError::Io {
                    path: self.workspaces_dir.clone(),
                    source: error,
                });
            }
        };
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| WorkspaceDeclarationError::Io {
                path: self.workspaces_dir.clone(),
                source: error,
            })?;
            let path = entry.path();
            if path.extension() != Some(OsStr::new(WORKSPACE_DECLARATION_EXTENSION)) {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| WorkspaceDeclarationError::Invalid {
                    path: path.clone(),
                    reason: "declaration filename is not valid UTF-8".to_string(),
                })?
                .to_string();
            candidates.push((path, id));
        }
        candidates.sort();
        let mut declarations = Vec::with_capacity(candidates.len());
        for (path, id) in candidates {
            declarations.push(load_declaration_at(&path, &id)?);
        }
        Ok(declarations)
    }

    /// 读取单个声明。
    ///
    /// # Errors
    /// 返回 [`WorkspaceDeclarationError::NotFound`]（文件缺失）、`Io`（读写失败）、
    /// `Corrupt`（无法解析、含未知字段或 id 与文件名不一致）、`Unsupported`（schema
    /// 版本未知/未来）或 `Invalid`（受信任路径链上存在链接/reparse、目标非普通文件或
    /// 字段校验失败）。
    pub fn read(&self, id: &str) -> Result<WorkspaceDeclaration, WorkspaceDeclarationError> {
        let path = self.declaration_path(id)?;
        ensure_trusted_root(&self.config_dir)?;
        validate_existing_path(&self.config_dir, &path)
            .map_err(|error| map_boundary_error(&path, id, error))?;
        load_declaration_at(&path, id)
    }

    /// 校验后原子写入单个声明，必要时创建声明目录，并返回写入路径。
    ///
    /// 写入前先完成 id/schema/字段校验与 no-follow 路径边界校验：无效声明或链接目标不会
    /// 被覆盖，也不会越出受信任配置目录创建文件。
    ///
    /// # Errors
    /// 返回 `Invalid`（id/字段校验失败、路径链存在链接/reparse 或目标非普通文件）、
    /// `Unsupported`（schema 版本非当前值）或 `Io`（目录创建或原子写失败）。
    pub fn save(
        &self,
        declaration: &WorkspaceDeclaration,
    ) -> Result<PathBuf, WorkspaceDeclarationError> {
        let path = self.declaration_path(&declaration.id)?;
        validate_declaration(declaration).map_err(|reason| WorkspaceDeclarationError::Invalid {
            path: path.clone(),
            reason,
        })?;
        if declaration.schema_version != WORKSPACE_DECLARATION_SCHEMA_VERSION {
            return Err(WorkspaceDeclarationError::Unsupported {
                path,
                version: declaration.schema_version,
                expected: WORKSPACE_DECLARATION_SCHEMA_VERSION,
            });
        }
        ensure_trusted_root(&self.config_dir)?;
        validate_path_for_write(&self.config_dir, &path)
            .map_err(|error| map_boundary_error(&path, &declaration.id, error))?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(WorkspaceDeclarationError::Invalid {
                    path,
                    reason: "declaration path is not a regular file".to_string(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(WorkspaceDeclarationError::Io {
                    path,
                    source: error,
                });
            }
        }
        let content = toml::to_string_pretty(declaration).map_err(|error| {
            WorkspaceDeclarationError::Invalid {
                path: path.clone(),
                reason: format!("failed to serialize declaration: {error}"),
            }
        })?;
        fs::create_dir_all(&self.workspaces_dir).map_err(|error| {
            WorkspaceDeclarationError::Io {
                path: self.workspaces_dir.clone(),
                source: error,
            }
        })?;
        pl_tool::workspace::write_file_atomically(&path, content.as_bytes()).map_err(|error| {
            WorkspaceDeclarationError::Io {
                path: path.clone(),
                source: error,
            }
        })?;
        Ok(path)
    }
}

/// 校验受信任根本身是否为真实目录（缺失允许，稍后创建）。
fn ensure_trusted_root(root: &Path) -> Result<(), WorkspaceDeclarationError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if is_link_or_reparse(&metadata) => Err(WorkspaceDeclarationError::Invalid {
            path: root.to_path_buf(),
            reason: "trusted config directory is a symbolic link or reparse point".to_string(),
        }),
        Ok(metadata) if !metadata.is_dir() => Err(WorkspaceDeclarationError::Invalid {
            path: root.to_path_buf(),
            reason: "trusted config directory is not a directory".to_string(),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(WorkspaceDeclarationError::Io {
            path: root.to_path_buf(),
            source: error,
        }),
    }
}

fn map_boundary_error(
    candidate: &Path,
    id: &str,
    error: PathSafetyError,
) -> WorkspaceDeclarationError {
    match error {
        PathSafetyError::LinkOrReparse { path } => WorkspaceDeclarationError::Invalid {
            path,
            reason: "path component is a symbolic link or reparse point".to_string(),
        },
        PathSafetyError::OutsideRoot { .. } => WorkspaceDeclarationError::Invalid {
            path: candidate.to_path_buf(),
            reason: "declaration path escapes its trusted root".to_string(),
        },
        PathSafetyError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            WorkspaceDeclarationError::NotFound { id: id.to_string() }
        }
        PathSafetyError::Io { path, source, .. } => WorkspaceDeclarationError::Io { path, source },
    }
}

fn load_declaration_at(
    path: &Path,
    id: &str,
) -> Result<WorkspaceDeclaration, WorkspaceDeclarationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) => {
            return Err(WorkspaceDeclarationError::Invalid {
                path: path.to_path_buf(),
                reason: "declaration path is a symbolic link or reparse point".to_string(),
            });
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(WorkspaceDeclarationError::Invalid {
                path: path.to_path_buf(),
                reason: "declaration path is not a regular file".to_string(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(WorkspaceDeclarationError::NotFound { id: id.to_string() });
        }
        Err(error) => {
            return Err(WorkspaceDeclarationError::Io {
                path: path.to_path_buf(),
                source: error,
            });
        }
    }
    let bytes = fs::read(path).map_err(|error| WorkspaceDeclarationError::Io {
        path: path.to_path_buf(),
        source: error,
    })?;
    let content = std::str::from_utf8(&bytes).map_err(|_| WorkspaceDeclarationError::Corrupt {
        path: path.to_path_buf(),
        reason: "declaration is not valid UTF-8".to_string(),
    })?;
    let declaration: WorkspaceDeclaration =
        toml::from_str(content).map_err(|error| WorkspaceDeclarationError::Corrupt {
            path: path.to_path_buf(),
            reason: error.message().to_string(),
        })?;
    if declaration.schema_version != WORKSPACE_DECLARATION_SCHEMA_VERSION {
        return Err(WorkspaceDeclarationError::Unsupported {
            path: path.to_path_buf(),
            version: declaration.schema_version,
            expected: WORKSPACE_DECLARATION_SCHEMA_VERSION,
        });
    }
    if declaration.id != id {
        return Err(WorkspaceDeclarationError::Corrupt {
            path: path.to_path_buf(),
            reason: format!(
                "declaration id `{}` does not match file name `{id}`",
                declaration.id
            ),
        });
    }
    validate_declaration(&declaration).map_err(|reason| WorkspaceDeclarationError::Invalid {
        path: path.to_path_buf(),
        reason,
    })?;
    Ok(declaration)
}

fn validate_declaration(declaration: &WorkspaceDeclaration) -> Result<(), String> {
    validate_workspace_id(&declaration.id)?;
    // 名称与路径是既有 Project 事实：只校验存在性必需约束，原样保全，不截断或改写。
    // `rename_project` 自身的 80 字符上限属于该用户命令，不是声明格式的历史不变量。
    if declaration.name.trim().is_empty() {
        return Err("name must not be empty".to_string());
    }
    if declaration.path.trim().is_empty() {
        return Err("path must not be empty".to_string());
    }
    if let Some(ssh_alias) = &declaration.ssh_alias {
        pl_tool::remote::validate_alias(ssh_alias)
            .map_err(|error| format!("invalid ssh_alias: {error}"))?;
    }
    Ok(())
}

fn validate_workspace_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > MAX_WORKSPACE_ID_BYTES {
        return Err(format!("id must be 1..={MAX_WORKSPACE_ID_BYTES} bytes"));
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!(
            "id must use only ASCII letters, digits, '-', '_' or '.': {id}"
        ));
    }
    if id == "." || id == ".." || id.contains("..") {
        return Err(format!("id must not traverse directories: {id}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_store() -> (TempDir, WorkspaceConfigStore) {
        let dir = TempDir::new().unwrap();
        let store = WorkspaceConfigStore::new(dir.path());
        (dir, store)
    }

    fn sample(id: &str) -> WorkspaceDeclaration {
        WorkspaceDeclaration::new(id, "Demo", "/tmp/demo", None)
    }

    #[test]
    fn save_creates_directory_and_round_trips_existing_values_verbatim() {
        let (_dir, store) = temp_store();
        assert!(!store.workspaces_dir().exists());
        // 目录名派生的既有名称可以超过 rename 命令的 80 字符上限；文件层必须原样保全。
        let long_name = "n".repeat(120);
        let declaration =
            WorkspaceDeclaration::new("project-1", long_name.clone(), "/tmp/demo", None);

        let path = store.save(&declaration).unwrap();

        assert!(path.is_file());
        let loaded = store.read("project-1").unwrap();
        assert_eq!(loaded, declaration);
        assert_eq!(loaded.name, long_name);
    }

    #[test]
    fn save_is_idempotent() {
        let (_dir, store) = temp_store();
        let declaration = sample("project-1");

        store.save(&declaration).unwrap();
        let first = fs::read(store.declaration_path("project-1").unwrap()).unwrap();
        store.save(&declaration).unwrap();
        let second = fs::read(store.declaration_path("project-1").unwrap()).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn atomic_update_replaces_previous_content_without_residue() {
        let (_dir, store) = temp_store();
        store.save(&sample("project-1")).unwrap();
        let mut updated = sample("project-1");
        updated.name = "Renamed".to_string();

        store.save(&updated).unwrap();

        assert_eq!(store.read("project-1").unwrap().name, "Renamed");
        let residue = fs::read_dir(store.workspaces_dir())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".pure-write-")
            })
            .count();
        assert_eq!(residue, 0);
    }

    #[test]
    fn load_all_returns_empty_when_directory_is_missing() {
        let (_dir, store) = temp_store();

        assert!(store.load_all().unwrap().is_empty());
        assert!(!store.workspaces_dir().exists());
    }

    #[test]
    fn load_all_returns_sorted_declarations() {
        let (_dir, store) = temp_store();
        store.save(&sample("project-b")).unwrap();
        store.save(&sample("project-a")).unwrap();
        store.save(&sample("project-c")).unwrap();

        let ids = store
            .load_all()
            .unwrap()
            .into_iter()
            .map(|declaration| declaration.id)
            .collect::<Vec<_>>();

        assert_eq!(ids, ["project-a", "project-b", "project-c"]);
    }

    #[test]
    fn load_all_ignores_non_toml_files() {
        let (_dir, store) = temp_store();
        store.save(&sample("project-1")).unwrap();
        fs::write(store.workspaces_dir().join("readme.md"), "notes").unwrap();

        let declarations = store.load_all().unwrap();

        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].id, "project-1");
    }

    #[test]
    fn load_all_does_not_skip_hidden_toml_declarations() {
        let (_dir, store) = temp_store();
        fs::create_dir_all(store.workspaces_dir()).unwrap();
        let hidden = store.workspaces_dir().join(".hidden.toml");
        let content =
            "schema_version = 1\nid = \".hidden\"\nname = \"Hidden\"\npath = \"/tmp/hidden\"\n";
        fs::write(&hidden, content).unwrap();

        let declarations = store.load_all().unwrap();

        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].id, ".hidden");
    }

    #[test]
    fn read_missing_declaration_is_not_found() {
        let (_dir, store) = temp_store();

        let error = store.read("project-missing").unwrap_err();

        assert!(matches!(
            error,
            WorkspaceDeclarationError::NotFound { ref id } if id == "project-missing"
        ));
    }

    #[test]
    fn load_all_fails_on_corrupt_file_without_skipping_others() {
        let (_dir, store) = temp_store();
        store.save(&sample("project-1")).unwrap();
        let broken = store.declaration_path("project-2").unwrap();
        fs::write(&broken, "not = [toml").unwrap();

        let error = store.load_all().unwrap_err();

        assert!(matches!(error, WorkspaceDeclarationError::Corrupt { .. }));
        assert_eq!(fs::read_to_string(&broken).unwrap(), "not = [toml");
        assert_eq!(store.read("project-1").unwrap(), sample("project-1"));
    }

    #[test]
    fn unknown_schema_version_is_rejected_and_preserved() {
        let (_dir, store) = temp_store();
        let path = store.declaration_path("project-1").unwrap();
        fs::create_dir_all(store.workspaces_dir()).unwrap();
        let content =
            "schema_version = 2\nid = \"project-1\"\nname = \"Demo\"\npath = \"/tmp/demo\"\n";
        fs::write(&path, content).unwrap();

        let error = store.read("project-1").unwrap_err();

        assert!(matches!(
            error,
            WorkspaceDeclarationError::Unsupported { version: 2, .. }
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn unknown_declaration_fields_are_rejected_and_preserved() {
        let (_dir, store) = temp_store();
        let path = store.declaration_path("project-1").unwrap();
        fs::create_dir_all(store.workspaces_dir()).unwrap();
        let content = "schema_version = 1\nid = \"project-1\"\nname = \"Demo\"\npath = \"/tmp/demo\"\nextra = 1\n";
        fs::write(&path, content).unwrap();

        let error = store.read("project-1").unwrap_err();

        assert!(matches!(error, WorkspaceDeclarationError::Corrupt { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn identity_mismatch_is_rejected_and_preserved() {
        let (_dir, store) = temp_store();
        let path = store.declaration_path("project-1").unwrap();
        fs::create_dir_all(store.workspaces_dir()).unwrap();
        let content =
            "schema_version = 1\nid = \"project-other\"\nname = \"Demo\"\npath = \"/tmp/demo\"\n";
        fs::write(&path, content).unwrap();

        let error = store.read("project-1").unwrap_err();

        assert!(matches!(error, WorkspaceDeclarationError::Corrupt { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn invalid_fields_are_rejected_and_preserved() {
        let (_dir, store) = temp_store();
        let path = store.declaration_path("project-1").unwrap();
        fs::create_dir_all(store.workspaces_dir()).unwrap();
        let content =
            "schema_version = 1\nid = \"project-1\"\nname = \"  \"\npath = \"/tmp/demo\"\n";
        fs::write(&path, content).unwrap();

        let error = store.read("project-1").unwrap_err();

        assert!(matches!(error, WorkspaceDeclarationError::Invalid { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn invalid_ssh_alias_is_rejected() {
        let (_dir, store) = temp_store();
        let declaration =
            WorkspaceDeclaration::new("project-1", "Demo", "/tmp/demo", Some("*".into()));

        let error = store.save(&declaration).unwrap_err();

        assert!(matches!(error, WorkspaceDeclarationError::Invalid { .. }));
        assert!(!store.declaration_path("project-1").unwrap().exists());
    }

    #[test]
    fn invalid_id_is_rejected_before_touching_filesystem() {
        let (_dir, store) = temp_store();
        for id in ["../evil", "a/b", "..", "with space", ""] {
            assert!(
                store
                    .save(&WorkspaceDeclaration::new(id, "Demo", "/tmp/demo", None))
                    .is_err(),
                "save must reject id `{id}`"
            );
            assert!(store.read(id).is_err(), "read must reject id `{id}`");
        }
        assert!(!store.workspaces_dir().exists());
    }

    #[test]
    fn save_rejects_invalid_declaration_without_overwriting() {
        let (_dir, store) = temp_store();
        store.save(&sample("project-1")).unwrap();
        let path = store.declaration_path("project-1").unwrap();
        let original = fs::read(&path).unwrap();
        let mut invalid = sample("project-1");
        invalid.name = "   ".to_string();

        let error = store.save(&invalid).unwrap_err();

        assert!(matches!(error, WorkspaceDeclarationError::Invalid { .. }));
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn custom_nested_config_directory_is_allowed() {
        let root = TempDir::new().unwrap();
        let config_dir = root.path().join("custom").join("anywork");
        let store = WorkspaceConfigStore::new(&config_dir);

        store.save(&sample("project-1")).unwrap();

        assert!(
            config_dir
                .join("workspaces")
                .join("project-1.toml")
                .is_file()
        );
        assert_eq!(store.read("project-1").unwrap(), sample("project-1"));
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_declaration_is_rejected_and_target_preserved() {
        let (dir, store) = temp_store();
        fs::create_dir_all(store.workspaces_dir()).unwrap();
        let outside = dir.path().join("outside.toml");
        fs::write(&outside, "schema_version = 1\n").unwrap();
        let link = store.declaration_path("project-1").unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        assert!(matches!(
            store.read("project-1").unwrap_err(),
            WorkspaceDeclarationError::Invalid { .. }
        ));
        assert!(matches!(
            store.load_all().unwrap_err(),
            WorkspaceDeclarationError::Invalid { .. }
        ));
        assert!(matches!(
            store.save(&sample("project-1")).unwrap_err(),
            WorkspaceDeclarationError::Invalid { .. }
        ));
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            "schema_version = 1\n"
        );
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_workspaces_directory_is_rejected_and_target_untouched() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("workspaces")).unwrap();
        let store = WorkspaceConfigStore::new(root.path());

        assert!(matches!(
            store.save(&sample("project-1")).unwrap_err(),
            WorkspaceDeclarationError::Invalid { .. }
        ));
        assert!(matches!(
            store.load_all().unwrap_err(),
            WorkspaceDeclarationError::Invalid { .. }
        ));
        assert!(matches!(
            store.read("project-1").unwrap_err(),
            WorkspaceDeclarationError::Invalid { .. }
        ));
        assert!(!outside.path().join("project-1.toml").exists());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_trusted_config_dir_is_rejected() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let anchor = root.path().join("anchor");
        std::os::unix::fs::symlink(outside.path(), &anchor).unwrap();
        let store = WorkspaceConfigStore::new(&anchor);

        assert!(matches!(
            store.save(&sample("project-1")).unwrap_err(),
            WorkspaceDeclarationError::Invalid { .. }
        ));
        assert!(!outside.path().join("workspaces").exists());
    }
}
