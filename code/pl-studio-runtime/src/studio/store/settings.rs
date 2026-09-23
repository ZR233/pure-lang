//! `settings.toml`: 产品设置键值对象的 canonical TOML 存储。
//!
//! 旧 `app_settings` 表属于旧会话版本，v2 不导入；普通启动与运行期只读写本文件。
//! 每个 mutation 使用同一把锁完成读取-修改-原子替换，`revision` 单调递增并提供幂等 no-op。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result, ensure};

use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;

pub(in crate::studio) const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::studio) struct SettingEntry {
    pub(in crate::studio) key: String,
    pub(in crate::studio) value: String,
    pub(in crate::studio) updated_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsDocument {
    schema_version: u32,
    revision: u64,
    entries: Vec<SettingEntry>,
}

#[derive(Clone)]
pub(in crate::studio) struct SettingsStore {
    path: PathBuf,
    inner: Arc<Mutex<SettingsDocument>>,
}

impl SettingsStore {
    pub(in crate::studio) async fn load(path: PathBuf) -> Result<Self> {
        let document = match tokio::fs::read_to_string(&path).await {
            Ok(content) => decode(&path, &content)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SettingsDocument {
                schema_version: SETTINGS_SCHEMA_VERSION,
                revision: 1,
                entries: Vec::new(),
            },
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            inner: Arc::new(Mutex::new(document)),
        })
    }

    #[allow(dead_code)]
    pub(in crate::studio) fn revision(&self) -> u64 {
        self.lock().revision
    }

    pub(in crate::studio) fn get(&self, key: &str) -> Option<String> {
        self.lock()
            .entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.clone())
    }

    pub(in crate::studio) async fn put(&self, key: &str, value: &str) -> Result<()> {
        ensure!(!key.trim().is_empty(), "setting key must not be empty");
        let key = key.to_string();
        let value = value.to_string();
        self.mutate(move |document| {
            let updated_at = unix_seconds();
            match document.entries.iter_mut().find(|entry| entry.key == key) {
                Some(existing) if existing.value == value => false,
                Some(existing) => {
                    existing.value = value;
                    existing.updated_at = updated_at;
                    true
                }
                None => {
                    document.entries.push(SettingEntry {
                        key,
                        value,
                        updated_at,
                    });
                    true
                }
            }
        })
        .await
    }

    /// 把当前内存文档写入 canonical 文件；文件已存在时不覆盖。
    ///
    /// 只有 migration 边界使用，用来补齐旧发布遗留的空缺文档；正常运行写入都经 [`Self::put`]。
    pub(in crate::studio) async fn persist_if_absent(&self) -> Result<()> {
        let inner = self.inner.clone();
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            if path.exists() {
                return Ok(());
            }
            let document = inner.lock().unwrap_or_else(PoisonError::into_inner);
            let contents = toml::to_string_pretty(&*document)
                .context("failed to serialize Studio settings")?
                .into_bytes();
            pl_tool::workspace::write_file_atomically(&path, &contents).map_err(anyhow::Error::from)
        })
        .await?
    }

    async fn mutate(
        &self,
        change: impl FnOnce(&mut SettingsDocument) -> bool + Send + 'static,
    ) -> Result<()> {
        let inner = self.inner.clone();
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || mutate_document(&inner, &path, change)).await?
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SettingsDocument> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn mutate_document(
    inner: &Mutex<SettingsDocument>,
    path: &Path,
    change: impl FnOnce(&mut SettingsDocument) -> bool,
) -> Result<()> {
    let mut document = inner.lock().unwrap_or_else(PoisonError::into_inner);
    let previous = document.clone();
    if !change(&mut document) {
        return Ok(());
    }
    document.revision = document.revision.saturating_add(1);
    let contents = toml::to_string_pretty(&*document)
        .context("failed to serialize Studio settings")?
        .into_bytes();
    match pl_tool::workspace::write_file_atomically(path, &contents) {
        Ok(()) => Ok(()),
        Err(error) => {
            *document = previous;
            Err(error.into())
        }
    }
}

fn decode(path: &Path, content: &str) -> Result<SettingsDocument> {
    let document: SettingsDocument = toml::from_str(content)
        .with_context(|| format!("invalid Studio settings {}", path.display()))?;
    ensure!(
        document.schema_version == SETTINGS_SCHEMA_VERSION,
        "unsupported Studio settings schema in {}",
        path.display()
    );
    ensure!(
        document.revision >= 1,
        "Studio settings revision is invalid"
    );
    let mut seen = std::collections::BTreeSet::new();
    for entry in &document.entries {
        ensure!(!entry.key.trim().is_empty(), "Studio settings key is empty");
        ensure!(
            seen.insert(entry.key.as_str()),
            "Studio settings contain a duplicate key"
        );
    }
    Ok(document)
}

impl StudioStore {
    pub async fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        self.settings_store().put(key, value).await
    }

    pub async fn load_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.settings_store().get(key))
    }
}
