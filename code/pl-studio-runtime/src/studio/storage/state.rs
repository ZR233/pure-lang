//! Atomic per-Thread current-state checkpoints.
//!
//! Publication order is fixed by `design/15` §15.4: the caller must have made the checkpoint's
//! history fence durable, every oversized body is written to the session's content-addressed blob
//! root and fenced, and only then is `state.toml` replaced atomically while the previous valid file
//! is retained as `state.prev.toml`. A failed publish keeps the old snapshot and the caller's
//! latest pending checkpoint, so a retry never observes a torn file.
//!
//! Loading is the inverse: each reference is read, verified and refilled before the checkpoint
//! leaves this module, so an owner never resumes from a body it cannot resolve and a missing or
//! corrupt blob is a hard error instead of an empty body.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result, bail, ensure};
use pl_core::thread::{CHECKPOINT_BODY_THRESHOLD_BYTES, ExtractedCheckpointBody, ThreadCheckpoint};

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

#[derive(Debug, Clone)]
pub(crate) struct StateStore {
    directory: PathBuf,
    thread_id: String,
}

impl StateStore {
    pub(crate) fn new(directory: PathBuf, thread_id: &str) -> Self {
        Self {
            directory,
            thread_id: thread_id.to_owned(),
        }
    }

    pub(crate) async fn load(&self) -> Result<Option<ThreadCheckpoint>> {
        let mut primary_error = None;
        for path in [self.path(), self.previous_path()] {
            let content = match tokio::fs::read_to_string(&path).await {
                Ok(content) => content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
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

    /// Atomically publishes `checkpoint` as `state.toml`.
    ///
    /// The caller must have already made `checkpoint.history_fence` durable. Oversized bodies leave
    /// the file first: each one is written to the per-Thread content-addressed blob root, and every
    /// reference the file will name is then fenced (present plus synced) — only then is the TOML
    /// replaced. The previous valid file is copied to `state.prev.toml` before the new one is
    /// written, so a crash between the two writes still leaves one loadable snapshot, and the
    /// failure keeps the caller's latest pending checkpoint.
    ///
    /// The transformation is applied to a copy: the caller's checkpoint, the live owner and the
    /// next model request keep the complete inline bodies.
    pub(crate) async fn publish(&self, checkpoint: &ThreadCheckpoint) -> Result<()> {
        ensure!(
            checkpoint.thread_id == self.thread_id,
            "Thread checkpoint belongs to another Thread"
        );
        ensure!(
            checkpoint.history_fence <= checkpoint.state_revision
                && checkpoint.state_revision == checkpoint.state.commit_sequence,
            "Thread checkpoint fence is inconsistent with its state revision"
        );
        let (externalized, bodies) = checkpoint.externalize_bodies(CHECKPOINT_BODY_THRESHOLD_BYTES);
        for body in &bodies {
            self.persist_body(body).await?;
        }
        self.fence_bodies(&externalized).await?;
        let contents = toml::to_string(&externalized)?.into_bytes();
        let path = self.path();
        let previous = self.previous_path();
        tokio::fs::create_dir_all(&self.directory).await?;
        if let Ok(current) = tokio::fs::read(&path).await {
            tokio::task::spawn_blocking(move || {
                pl_tool::workspace::write_file_atomically(&previous, &current)
            })
            .await??;
        }
        tokio::task::spawn_blocking(move || {
            pl_tool::workspace::write_file_atomically(&path, &contents)
        })
        .await??;
        // 成功保存后 `state.prev.toml` 回退不再是最新事实，清除它的显式诊断。
        clear_checkpoint_recovery(&self.thread_id);
        Ok(())
    }

    /// Parses one checkpoint file and materializes every body it references.
    ///
    /// Externalized bodies are restored here, before the checkpoint is handed to any caller, so
    /// activation, the owner and the next model request always see the exact original bytes. A
    /// missing, corrupt or mismatchingly-targeted blob fails closed; it is never read as an empty
    /// body, and it never silently becomes a truncated model context.
    async fn read_checkpoint(&self, content: &str, path: &Path) -> Result<ThreadCheckpoint> {
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

    /// Writes one extracted body to its content-addressed path.
    ///
    /// The write is idempotent: identical bytes share one file, so a repeated save or a re-published
    /// digest never writes a second copy, and an abandoned body is left in place instead of being
    /// deleted. A file that is already there is never rewritten here — even when its bytes may be
    /// wrong — because it can still be the only copy of bytes another `state.prev.toml` references;
    /// whether it really holds the declared body is decided by [`Self::fence_bodies`] before any new
    /// TOML may name it.
    async fn persist_body(&self, body: &ExtractedCheckpointBody) -> Result<()> {
        let path = self.blob_path(body.entry.reference.digest())?;
        if tokio::fs::try_exists(&path).await? {
            return Ok(());
        }
        let directory = path
            .parent()
            .context("checkpoint body has no parent directory")?
            .to_path_buf();
        tokio::fs::create_dir_all(&directory).await?;
        let contents = body.bytes.clone();
        tokio::task::spawn_blocking(move || {
            pl_tool::workspace::write_file_atomically(&path, &contents)
        })
        .await??;
        Ok(())
    }

    /// Fences every body the checkpoint names: present, byte-exact, with its file and directory
    /// entry synced.
    ///
    /// A published `state.toml` must never name a blob whose bytes are not provably durable and
    /// provably equal to the body the reference declares. Existing a content-addressed path is not
    /// proof: a truncated, replaced or corrupted file under a valid digest path fails publication
    /// instead of being published as if it were durable, and it is never overwritten in place.
    async fn fence_bodies(&self, checkpoint: &ThreadCheckpoint) -> Result<()> {
        for entry in &checkpoint.external_bodies {
            let path = self.blob_path(entry.reference.digest())?;
            let bytes = tokio::fs::read(&path).await.with_context(|| {
                format!(
                    "checkpoint names an unreadable body blob {} for Thread {}",
                    path.display(),
                    self.thread_id
                )
            })?;
            if let Err(error) = entry.reference.verify(&bytes) {
                bail!(
                    "checkpoint body blob {} does not hold {} for Thread {}: {error}",
                    path.display(),
                    entry.reference.digest(),
                    self.thread_id
                );
            }
            sync_checkpoint_blob(&path).await?;
        }
        Ok(())
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

/// Makes one checkpoint body blob and its directory entry durable.
///
/// Directory sync is skipped on Windows, which has no equivalent operation.
async fn sync_checkpoint_blob(path: &Path) -> Result<()> {
    let file = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::File::open(file)?.sync_all()).await??;
    #[cfg(unix)]
    if let Some(directory) = path.parent() {
        let directory = directory.to_path_buf();
        tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all()).await??;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::context::{
        ContextContent, ContextRecord, ContextSnapshot, ContextSource, OpaquePayload,
    };
    use pl_core::thread::inbox::{InboxRecord, ThreadMessage};
    use pl_core::thread::{ThreadSnapshot, ToolDelivery, ToolDeliveryTarget, ToolOutcome};
    use pl_core::tool::ToolOutput;

    /// 与外置阈值相比足够大的正文，保证它必须离开 state.toml。
    const OVERSIZED: usize = CHECKPOINT_BODY_THRESHOLD_BYTES + 4096;

    fn store(directory: &tempfile::TempDir) -> StateStore {
        StateStore::new(directory.path().to_path_buf(), "thread")
    }

    fn captured(revision: u64, state: ThreadSnapshot) -> ThreadCheckpoint {
        ThreadCheckpoint::capture("thread".to_owned(), revision, state)
    }

    async fn state_file(directory: &tempfile::TempDir) -> String {
        tokio::fs::read_to_string(directory.path().join("state.toml"))
            .await
            .unwrap()
    }

    /// 一条大型当前上下文：同一份正文既作为 Text，也作为 opaque assistant frame。
    fn large_context(revision: u64, label: &str) -> ThreadSnapshot {
        let body = format!("{label}:{}", "x".repeat(OVERSIZED));
        let record = ContextRecord {
            id: format!("record-{revision}"),
            turn_id: None,
            source: ContextSource::Assistant,
            content: vec![
                ContextContent::Text {
                    text: body.as_str().into(),
                },
                ContextContent::Opaque {
                    payload: OpaquePayload::new("pl.model.assistant", 2, body.as_str()).unwrap(),
                },
            ],
            tool_calls: Vec::new(),
        };
        ThreadSnapshot {
            commit_sequence: revision,
            context: ContextSnapshot {
                revision,
                records: vec![record].into(),
            },
            ..Default::default()
        }
    }

    /// 尚未外置正文的大上下文离开文件，载入时按引用精确还原。
    #[tokio::test]
    async fn oversized_current_bodies_leave_the_file_and_restore_exactly() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory);
        let published = captured(1, large_context(1, "assistant"));
        store.publish(&published).await.unwrap();

        let contents = state_file(&directory).await;
        assert!(
            contents.contains("externalBodies"),
            "checkpoint names no body"
        );
        assert!(
            !contents.contains(&"x".repeat(256)),
            "the checkpoint still inlines the large body"
        );
        assert!(
            contents.len() < CHECKPOINT_BODY_THRESHOLD_BYTES,
            "checkpoint is {} bytes",
            contents.len()
        );

        // 重复保存是幂等的：内容寻址 blob 只写一次，文件逐字节相同。
        store.publish(&published).await.unwrap();
        assert_eq!(state_file(&directory).await, contents);

        // 冷载入必须先物化完整正文，下一次模型请求看到的正是发布前的原始字节。
        let loaded = store.load().await.unwrap().unwrap();
        assert!(loaded.is_materialized());
        assert_eq!(
            loaded.state.context.records,
            published.state.context.records
        );
        assert_eq!(
            loaded.state.context.records[0].content,
            published.state.context.records[0].content
        );
    }

    /// 待交付的工具结果正文同样外置，并在载入后按原格式还原。
    #[tokio::test]
    async fn pending_delivery_bodies_are_externalized_and_restored() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory);
        let body = format!("delivery:{}", "y".repeat(OVERSIZED));
        let delivery = ToolDelivery {
            target: ToolDeliveryTarget::Inbox {
                message_id: "message-1".to_owned(),
            },
            call_id: "call-1".to_owned(),
            tool_id: "tool-1".to_owned(),
            output: ToolOutput::new(
                OpaquePayload::new("pl.tool.result", 1, body.as_str()).unwrap(),
                vec![ContextContent::Text {
                    text: body.as_str().into(),
                }],
            ),
            delivered_context: vec![ContextContent::Text {
                text: body.as_str().into(),
            }],
            outcome: ToolOutcome::Succeeded,
        };
        let state = ThreadSnapshot {
            commit_sequence: 1,
            inbox: vec![InboxRecord {
                sequence: 1,
                message: ThreadMessage {
                    id: "message-1".to_owned(),
                    source_id: "child".to_owned(),
                    payload: OpaquePayload::text("notify"),
                    context: Vec::new(),
                },
            }]
            .into(),
            deliveries: vec![delivery].into(),
            ..Default::default()
        };
        let published = captured(1, state);
        assert_eq!(
            published.state.deliveries.len(),
            1,
            "a delivery still owed to model context has to stay resident"
        );
        store.publish(&published).await.unwrap();

        let contents = state_file(&directory).await;
        assert!(!contents.contains(&"y".repeat(256)));
        assert!(contents.len() < CHECKPOINT_BODY_THRESHOLD_BYTES);

        let loaded = store.load().await.unwrap().unwrap();
        let restored = &loaded.state.deliveries[0];
        assert_eq!(restored.call_id, "call-1");
        assert_eq!(restored.tool_id, "tool-1");
        assert!(matches!(restored.outcome, ToolOutcome::Succeeded));
        assert_eq!(restored.output.payload().content(), body.as_str());
        assert_eq!(
            restored.output.context().to_vec(),
            vec![ContextContent::Text {
                text: body.as_str().into(),
            }]
        );
        assert_eq!(
            restored.delivered_context,
            vec![ContextContent::Text {
                text: body.as_str().into(),
            }]
        );
    }

    /// 缺失或被替换的正文 blob 必须失败闭锁，绝不当作空正文载入。
    #[tokio::test]
    async fn missing_or_replaced_body_blob_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory);
        store
            .publish(&captured(1, large_context(1, "assistant")))
            .await
            .unwrap();

        let on_disk: ThreadCheckpoint = toml::from_str(&state_file(&directory).await).unwrap();
        let reference = on_disk.pending_body().unwrap().reference.clone();
        let blob = store.blob_path(reference.digest()).unwrap();
        assert!(blob.starts_with(directory.path().join("blobs").join("checkpoint")));

        // 摘要不符：损坏的正文被拒绝。
        tokio::fs::write(&blob, b"replaced body").await.unwrap();
        assert!(store.load().await.is_err());

        // 缺失：无法解析引用时同样失败闭锁。
        tokio::fs::remove_file(&blob).await.unwrap();
        assert!(store.load().await.is_err());
    }

    /// 内容寻址路径上已存在但字节不符的 blob 绝不能被发布成 durable，也绝不就地覆盖。
    #[tokio::test]
    async fn existing_mismatched_body_blob_is_never_published() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory);
        let published = captured(1, large_context(1, "assistant"));
        store.publish(&published).await.unwrap();
        let before = state_file(&directory).await;

        let on_disk: ThreadCheckpoint = toml::from_str(&before).unwrap();
        let blob = store
            .blob_path(on_disk.pending_body().unwrap().reference.digest())
            .unwrap();
        let corrupted = b"corrupted body under a valid digest path".to_vec();
        tokio::fs::write(&blob, &corrupted).await.unwrap();

        // 再次发布同一状态：路径存在但字节不符，必须在替换 TOML 之前失败闭锁。
        assert!(store.publish(&published).await.is_err());
        // 损坏文件保持原样：不覆盖可恢复数据，`state.toml` 也没有被替换。
        assert_eq!(tokio::fs::read(&blob).await.unwrap(), corrupted);
        assert_eq!(state_file(&directory).await, before);
        // 该快照的引用同样不可验证，载入失败闭锁而不是返回空正文。
        assert!(store.load().await.is_err());
    }

    /// 迁移前的内联 schema-1 文件仍然可以无损载入。
    #[tokio::test]
    async fn legacy_inline_checkpoint_still_loads() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory);
        let published = captured(1, large_context(1, "legacy"));
        let mut document = toml::Value::try_from(&published).unwrap();
        document
            .as_table_mut()
            .unwrap()
            .insert("schemaVersion".to_owned(), toml::Value::Integer(1));
        let legacy = toml::to_string(&document).unwrap();
        assert!(
            legacy.len() > CHECKPOINT_BODY_THRESHOLD_BYTES,
            "the legacy fixture has to stay an inline checkpoint"
        );
        tokio::fs::write(directory.path().join("state.toml"), legacy)
            .await
            .unwrap();

        let loaded = store.load().await.unwrap().unwrap();
        assert_eq!(
            loaded.schema_version,
            ThreadCheckpoint::LEGACY_SCHEMA_VERSION
        );
        assert!(loaded.is_materialized());
        assert_eq!(
            loaded.state.context.records,
            published.state.context.records
        );
    }

    /// 主文件不可用时回退 `state.prev.toml`，并从它自己的引用恢复完整正文。
    #[tokio::test]
    async fn previous_checkpoint_restores_from_its_own_blobs() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory);
        let first = captured(1, large_context(1, "first"));
        store.publish(&first).await.unwrap();
        // 第二份正文与第一份不同，回退必须依赖第一份自己的 blob，而不是重新写出的正文。
        store
            .publish(&captured(2, large_context(2, "second")))
            .await
            .unwrap();

        tokio::fs::write(directory.path().join("state.toml"), "schemaVersion = 3\n")
            .await
            .unwrap();
        let loaded = store.load().await.unwrap().unwrap();
        assert_eq!(loaded.state.commit_sequence, 1);
        assert_eq!(loaded.state.context.records, first.state.context.records);
        assert!(loaded.is_materialized());
    }

    /// 当前与迁移前 schema 正常解析；未来 schema 显式失败闭锁，不做任何宽容解码。
    #[test]
    fn unsupported_checkpoint_schema_fails_closed() {
        let checkpoint = ThreadCheckpoint::capture(
            "thread".to_owned(),
            1,
            pl_core::thread::ThreadSnapshot {
                commit_sequence: 1,
                ..Default::default()
            },
        );
        let current = toml::to_string(&checkpoint).unwrap();
        assert!(parse_checkpoint(&current, Path::new("state.toml")).is_ok());

        let mut document = toml::Value::try_from(&checkpoint).unwrap();
        // Re-acquire the table per write instead of holding one mutable borrow across
        // `toml::to_string(&document)` below.
        let set_schema = |document: &mut toml::Value, version: i64| {
            document
                .as_table_mut()
                .expect("Thread checkpoint serializes as a TOML table")
                .insert("schemaVersion".to_owned(), toml::Value::Integer(version));
        };
        set_schema(
            &mut document,
            i64::from(ThreadCheckpoint::LEGACY_SCHEMA_VERSION),
        );
        let legacy = toml::to_string(&document).unwrap();
        assert!(parse_checkpoint(&legacy, Path::new("state.toml")).is_ok());

        set_schema(&mut document, 3);
        let future = toml::to_string(&document).unwrap();
        let error = parse_checkpoint(&future, Path::new("state.toml")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported Thread checkpoint schema 3"),
            "unexpected error: {error}"
        );
    }
}
