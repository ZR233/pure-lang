//! Current SQLite checkpoint loading and the isolated v2 TOML migration boundary.
//! Legacy files are removed only after the imported transaction can be read and verified.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result, ensure};
use pl_core::thread::ThreadCheckpoint;

/// 显式的 `state.prev.toml` 回退诊断。
///
/// checkpoint 层只知道自己读到了哪一份文件，所以恢复事实在一个进程级登记表里累积：持久化观测
/// 把它作为该 Thread 的可查询恢复诊断发布，同一 Thread 的下一次成功保存会清除它。
static CHECKPOINT_RECOVERY: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();

fn checkpoint_recovery() -> &'static Mutex<BTreeMap<String, String>> {
    CHECKPOINT_RECOVERY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// 登记一次显式回退；诊断内容说明主文件为何不可用。
pub(crate) fn record_checkpoint_recovery(thread_id: &str, diagnostic: String) {
    checkpoint_recovery()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(thread_id.to_owned(), diagnostic);
}

/// 同一 Thread 成功保存 checkpoint 后清除回退诊断。
pub(crate) fn clear_checkpoint_recovery(thread_id: &str) {
    checkpoint_recovery()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(thread_id);
}

/// 当前所有未清除的回退诊断快照。
pub(crate) fn checkpoint_recovery_snapshot() -> BTreeMap<String, String> {
    checkpoint_recovery()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Lightweight per-Thread identity record for one admitted input that reached a terminal state.
///
/// It is deliberately not a history entity: it carries only the identity, content digest and the
/// framework receipt needed to answer a repeated `submitPrompt`, so neither the admitted body nor
/// its context is duplicated. Core keeps its bounded resident window; this row is the durable
/// fallback the runtime consults after a restart.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InputIdentityEntry {
    /// Content digest of the originally accepted body, for future content-equality checks.
    pub(crate) digest: String,
    /// Delivery routing receipt retained from the original admission.
    pub(crate) delivery: pl_core::thread::input::InputDelivery,
    pub(crate) id: String,
    pub(crate) ordinal: u64,
    pub(crate) revision: u64,
    pub(crate) state: pl_core::thread::input::InputState,
    /// Original admission watermark, returned as the repeat submission cursor.
    pub(crate) accepted_sequence: u64,
}

impl InputIdentityEntry {
    pub(crate) fn new(
        identity: pl_core::thread::input::InputIdentity,
        accepted_sequence: u64,
    ) -> Self {
        Self {
            digest: identity.digest,
            delivery: identity.delivery,
            id: identity.id,
            ordinal: identity.ordinal,
            revision: identity.revision,
            state: identity.state,
            accepted_sequence,
        }
    }
}

#[derive(Clone)]
pub(crate) struct StateStore {
    directory: PathBuf,
    thread_id: String,
    legacy_calls: Option<super::calls::CallsStore>,
}

impl StateStore {
    pub(crate) fn new(directory: PathBuf, thread_id: &str) -> Self {
        Self {
            directory,
            thread_id: thread_id.to_owned(),
            legacy_calls: None,
        }
    }

    pub(crate) fn with_legacy_calls(mut self, calls: super::calls::CallsStore) -> Self {
        self.legacy_calls = Some(calls);
        self
    }

    pub(crate) async fn load(&self) -> Result<Option<ThreadCheckpoint>> {
        let history = super::history::HistoryStore::open(
            &self.directory.join("history.sqlite"),
            &self.thread_id,
        )
        .await?;
        if let Some(checkpoint) = history.checkpoint().await? {
            self.cleanup_legacy(&history).await?;
            return Ok(Some(checkpoint));
        }
        let Some(mut checkpoint) = self.load_legacy().await? else {
            ensure!(
                history.watermark().await? == 0,
                "history exists without a recovery checkpoint"
            );
            return Ok(None);
        };
        // Old compaction receipts used one extension key per request. History owns the old
        // reductions and accounting is already cumulative; only the newest receipt is current.
        let latest_compaction = checkpoint
            .state
            .extensions
            .values()
            .filter(|record| record.payload.format() == "pl.studio.compaction")
            .max_by_key(|record| record.revision)
            .cloned();
        checkpoint
            .state
            .extensions
            .retain(|_, record| record.payload.format() != "pl.studio.compaction");
        if let Some(record) = latest_compaction {
            checkpoint
                .state
                .extensions
                .insert(crate::compaction::LATEST_RECEIPT.into(), record);
        }
        let legacy_costs = match &self.legacy_calls {
            Some(calls) => {
                calls
                    .legacy_session_costs(&self.thread_id, checkpoint.history_fence)
                    .await?
            }
            None => Vec::new(),
        };
        if let Some(calls) = &self.legacy_calls {
            let auxiliary = calls.legacy_auxiliary_usage(&self.thread_id).await?;
            let summary = &mut checkpoint.state.usage_summary;
            for (target, value) in [
                (&mut summary.inference_count, auxiliary.inference_count),
                (&mut summary.prompt_tokens, auxiliary.prompt_tokens),
                (&mut summary.completion_tokens, auxiliary.completion_tokens),
                (
                    &mut summary.cached_prompt_tokens,
                    auxiliary.cached_prompt_tokens,
                ),
                (
                    &mut summary.cache_write_tokens,
                    auxiliary.cache_write_tokens,
                ),
                (&mut summary.reasoning_tokens, auxiliary.reasoning_tokens),
                (&mut summary.total_tokens, auxiliary.total_tokens),
            ] {
                *target = target
                    .checked_add(value)
                    .context("legacy auxiliary accounting overflow")?;
            }
            summary.has_incomplete_usage |= auxiliary.has_incomplete_usage;
            summary.cache_incomplete |= auxiliary.cache_incomplete;
            summary.has_unpriced_usage |= auxiliary.has_unpriced_usage;
        }
        history
            .import_checkpoint(&checkpoint, &legacy_costs)
            .await?;
        let restored = history
            .checkpoint()
            .await?
            .context("imported checkpoint is absent")?;
        self.cleanup_legacy(&history).await?;
        Ok(Some(restored))
    }

    async fn cleanup_legacy(&self, history: &super::history::HistoryStore) -> Result<()> {
        if !history.legacy_cleanup_pending().await? {
            return Ok(());
        }
        for path in [self.path(), self.previous_path()] {
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("remove migrated checkpoint {}", path.display()));
                }
            }
        }
        // This dedicated namespace belongs only to the replaced TOML manifests. Attachment
        // and command-output blobs are siblings and must never enter this cleanup.
        match tokio::fs::remove_dir_all(self.checkpoint_blobs_dir()).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("remove migrated checkpoint bodies"),
        }
        history.finish_legacy_cleanup().await?;
        Ok(())
    }

    async fn load_legacy(&self) -> Result<Option<ThreadCheckpoint>> {
        let mut primary_error = None;
        for path in [self.path(), self.previous_path()] {
            let content = match tokio::fs::read_to_string(&path).await {
                Ok(content) => content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if let Ok(value) = toml::from_str::<toml::Value>(&content)
                && let Some(version) = value.get("schemaVersion").and_then(toml::Value::as_integer)
            {
                ensure!(
                    (1..=i64::from(ThreadCheckpoint::SCHEMA_VERSION)).contains(&version),
                    "unsupported checkpoint schema {version}; source files preserved"
                );
            }
            let checkpoint = match self.read_checkpoint(&content, &path).await {
                Ok(checkpoint) => checkpoint,
                Err(error) if path == self.path() => {
                    primary_error = Some(error);
                    continue;
                }
                Err(error) => return Err(primary_error.unwrap_or(error)),
            };
            ensure!(
                checkpoint.thread_id == self.thread_id,
                "Thread checkpoint belongs to another Thread"
            );
            if path == self.previous_path() {
                // 显式恢复诊断：回退到 `state.prev.toml` 是一次真实恢复，不是普通加载。只有
                // `state.toml` 缺失或不可解析时才会走到这里，因此必须留下可追溯的原因。
                let primary = primary_error.as_ref().map_or_else(
                    || "state.toml was missing".to_owned(),
                    std::string::ToString::to_string,
                );
                record_checkpoint_recovery(
                    &self.thread_id,
                    format!("recovered the Thread checkpoint from state.prev.toml ({primary})"),
                );
                tracing::warn!(
                    thread_id = %self.thread_id,
                    recovered_from = %path.display(),
                    primary = %primary,
                    "recovered the Thread checkpoint from state.prev.toml"
                );
            }
            return Ok(Some(checkpoint));
        }
        match primary_error {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }

    /// Parses one checkpoint file and materializes every body it references.
    ///
    /// Externalized bodies are restored here, before the checkpoint is handed to any caller, so
    /// activation, the owner and the next model request always see the exact original bytes. A
    /// missing, corrupt or mismatchingly-targeted blob fails closed; it is never read as an empty
    /// body, and it never silently becomes a truncated model context.
    async fn read_checkpoint(&self, content: &str, path: &Path) -> Result<ThreadCheckpoint> {
        let value: toml::Value = toml::from_str(content)
            .with_context(|| format!("invalid Thread checkpoint {}", path.display()))?;
        let version = value
            .get("schemaVersion")
            .and_then(toml::Value::as_integer)
            .context("checkpoint schemaVersion is missing")?;
        if ThreadCheckpoint::is_legacy_schema(u32::try_from(version)?) {
            let mut legacy = pl_core::thread::LegacyThreadCheckpoint::decode_json(
                &serde_json::to_string(&value)?,
            )?;
            // Even bodies discarded by the new layout are checked before deleting the old manifest.
            for entry in legacy.dropped_attempt_bodies() {
                let bytes = tokio::fs::read(self.blob_path(entry.reference.digest())?).await?;
                entry.reference.verify(&bytes)?;
            }
            while let Some(entry) = legacy.pending_body().cloned() {
                let bytes = tokio::fs::read(self.blob_path(entry.reference.digest())?).await?;
                legacy.materialize_body(&entry.reference, &bytes)?;
            }
            let route = self.legacy_route(&value).await?;
            let mut checkpoint = legacy.into_current()?;
            if !checkpoint
                .state
                .extensions
                .contains_key(crate::studio::model_route::MODEL_ROUTE_EXTENSION)
                && let Some(route) = route
            {
                checkpoint.state.extension_sequence = checkpoint
                    .state
                    .extension_sequence
                    .checked_add(1)
                    .context("legacy extension sequence exhausted")?;
                checkpoint.state.extensions.insert(
                    crate::studio::model_route::MODEL_ROUTE_EXTENSION.into(),
                    pl_core::thread::extensions::ExtensionRecord {
                        revision: checkpoint.state.extension_sequence,
                        payload: crate::studio::model_route::encode(&route)?,
                    },
                );
            }
            crate::compaction::migrate_checkpoint_sources(&mut checkpoint)?;
            return Ok(checkpoint);
        }
        let mut checkpoint = parse_checkpoint(content, path)?;
        while let Some(entry) = checkpoint.pending_body().cloned() {
            let blob = self.blob_path(entry.reference.digest())?;
            let bytes = tokio::fs::read(&blob).await.with_context(|| {
                format!(
                    "checkpoint body {} is missing for Thread {}",
                    entry.reference.digest(),
                    self.thread_id
                )
            })?;
            checkpoint
                .materialize_body(&entry.reference, &bytes)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "checkpoint body {} is unusable for Thread {}: {error}",
                        entry.reference.digest(),
                        self.thread_id
                    )
                })?;
        }
        Ok(checkpoint)
    }

    /// Preserve the selected model at the migration boundary without retaining tool schemas.
    async fn legacy_route(
        &self,
        value: &toml::Value,
    ) -> Result<Option<pl_model::config::ModelRouteConfig>> {
        let root = serde_json::to_value(value)?;
        let Some(attempts) = root
            .get("state")
            .and_then(|state| state.get("attempts"))
            .and_then(serde_json::Value::as_array)
        else {
            return Ok(None);
        };
        for (index, attempt) in attempts.iter().enumerate().rev() {
            let Some(metadata) = attempt
                .get("requestMetadata")
                .filter(|value| !value.is_null())
            else {
                continue;
            };
            let mut payload: pl_core::context::OpaquePayload =
                serde_json::from_value(metadata.clone())?;
            if payload.format() != "pl.model.prepared-request" {
                continue;
            }
            if let Some(entries) = root
                .get("externalBodies")
                .and_then(serde_json::Value::as_array)
            {
                for entry in entries {
                    if let Ok(entry) = serde_json::from_value::<
                        pl_core::thread::LegacyCheckpointExternalBody,
                    >(entry.clone())
                        && matches!(entry.slot, pl_core::thread::LegacyCheckpointBodySlot::AttemptMetadata { attempt_index } if attempt_index == index)
                    {
                        let bytes =
                            tokio::fs::read(self.blob_path(entry.reference.digest())?).await?;
                        entry.reference.verify(&bytes)?;
                        payload = pl_core::context::OpaquePayload::new(
                            payload.format(),
                            payload.version(),
                            String::from_utf8(bytes)?,
                        )?;
                    }
                }
            }
            let receipt = pl_model::runtime::model_request_receipt(&payload)?;
            return Ok(Some(pl_model::config::ModelRouteConfig {
                provider: pl_model::config::ProviderId::new(receipt.binding.provider_instance_id)?,
                model: receipt.binding.requested_model,
                effort: receipt
                    .reasoning
                    .and_then(|reasoning| reasoning.effort)
                    .map(pl_model::config::ReasoningEffort::new),
            }));
        }
        Ok(None)
    }

    /// Root of the checkpoint's own content-addressed blob store inside the session directory.
    ///
    /// It lives beside the attachment blobs, under the same session `blobs` root, so relocating a
    /// session directory carries both and every reference stays resolvable without absolute paths.
    fn checkpoint_blobs_dir(&self) -> PathBuf {
        self.directory.join("blobs").join("checkpoint")
    }

    /// Absolute path of one referenced body, derived from its content address.
    fn blob_path(&self, digest: &str) -> Result<PathBuf> {
        let hex = digest
            .strip_prefix("sha256:")
            .with_context(|| format!("checkpoint body digest {digest} is not a SHA-256 address"))?;
        ensure!(
            hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "checkpoint body digest {digest} is not a SHA-256 address"
        );
        Ok(self.checkpoint_blobs_dir().join(&hex[..2]).join(hex))
    }

    fn path(&self) -> PathBuf {
        self.directory.join("state.toml")
    }

    fn previous_path(&self) -> PathBuf {
        self.directory.join("state.prev.toml")
    }
}

/// 解析一份 checkpoint 文件，并拒绝本实现无法解释的未来 schema 版本。
///
/// 尚未外置正文的 schema-1 文件原样接受，因为它已经在文件里保留完整正文。未来版本则显式失败
/// 闭锁：本层不做任何"尽力解读新布局"的宽容解码——一个不认识的版本无法区分真实正文与引用，调用
/// 方因此把它当作不可用的快照处理：主文件不可用时回退 `state.prev.toml` 并登记显式恢复诊断，两份
/// 都不可用时报错。
fn parse_checkpoint(content: &str, path: &Path) -> Result<ThreadCheckpoint> {
    let checkpoint: ThreadCheckpoint = toml::from_str(content)
        .with_context(|| format!("invalid Thread checkpoint {}", path.display()))?;
    ensure!(
        ThreadCheckpoint::supports_schema(checkpoint.schema_version),
        "unsupported Thread checkpoint schema {} in {}; this build reads schema {}",
        checkpoint.schema_version,
        path.display(),
        ThreadCheckpoint::SCHEMA_VERSION
    );
    Ok(checkpoint)
}
