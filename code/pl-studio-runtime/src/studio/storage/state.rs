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
    // Windows backs `sync_all` with `FlushFileBuffers`, which requires a writable handle;
    // opening the blob read-only returns `ERROR_ACCESS_DENIED` there.
    tokio::task::spawn_blocking(move || {
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(file)?;
        handle.sync_all()
    })
    .await??;
    #[cfg(unix)]
    if let Some(directory) = path.parent() {
        let directory = directory.to_path_buf();
        tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all()).await??;
    }
    Ok(())
}
