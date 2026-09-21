//! Durable, resumable state for the one-time legacy session migration.
//!
//! The report is the single source of truth for resume decisions: every mutating step persists the
//! phase that has actually finished before the next one starts, so a crash always resumes from the
//! last durable phase and never re-publishes a partially verified layout. Reports written by older
//! coordinator versions are normalized in memory and persisted in the current schema before the
//! corresponding phase advances.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::studio::paths::thread_storage_key;
use crate::studio::storage::calls::CallsImportReport;

use super::MigrationTrigger;

pub(super) const MIGRATION_STATE_SCHEMA_VERSION: u32 = 7;
pub(super) const MIN_SUPPORTED_MIGRATION_STATE_SCHEMA_VERSION: u32 = 1;
pub(super) const MIGRATION_STATE_FILE_NAME: &str = "session-migration.json";

/// Ordered migration phases. Each transition is persisted before the next phase begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum MigrationPhase {
    /// Legacy sources identified and fingerprinted; nothing has been mutated yet.
    Detected,
    /// Consistent, non-overwriting backups exist for every legacy source.
    BackedUp,
    /// In-place product/session schema upgrades finished.
    Upgraded,
    /// Every legacy session has current `state.toml`, `history.sqlite` and `calls.sqlite` facts in
    /// the staging area; the legacy databases are untouched.
    Exported,
    /// Staged targets pass integrity, count and identity verification.
    Verified,
    /// Migration is published; later startups must not touch legacy databases again.
    Published,
}

impl MigrationPhase {
    pub(super) fn order(self) -> u8 {
        match self {
            Self::Detected => 0,
            Self::BackedUp => 1,
            Self::Upgraded => 2,
            Self::Exported => 3,
            Self::Verified => 4,
            Self::Published => 5,
        }
    }

    /// Stable audit label for the durable report and CLI summaries.
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::BackedUp => "backedUp",
            Self::Upgraded => "upgraded",
            Self::Exported => "exported",
            Self::Verified => "verified",
            Self::Published => "published",
        }
    }
}

/// Content identity of one legacy source, recorded before the first mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SourceFingerprint {
    #[serde(default)]
    pub(super) path: String,
    #[serde(default)]
    pub(super) exists: bool,
    #[serde(default)]
    pub(super) len: u64,
    #[serde(default, alias = "modified_unix_ms")]
    pub(super) modified_unix_ms: i64,
    #[serde(default, alias = "schema_version")]
    pub(super) schema_version: Option<i64>,
}

/// Per-session progress inside the export/verify/publish phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum SessionMigrationStatus {
    /// Current-layout facts are staged and the manifest is written.
    Staged,
    /// Staged facts verified against the legacy source.
    Verified,
}

fn default_session_status() -> SessionMigrationStatus {
    SessionMigrationStatus::Staged
}

/// Resumable per-session record: the durable phase and the checks a later run must reproduce.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SessionMigrationRecord {
    pub(super) thread_id: String,
    /// Stable on-disk directory key derived from `thread_id`.
    #[serde(default, alias = "storage_key")]
    pub(super) storage_key: String,
    #[serde(default = "default_session_status")]
    pub(super) status: SessionMigrationStatus,
    /// Durable Thread-commit head observed in the legacy journal.
    #[serde(default, alias = "journal_head")]
    pub(super) journal_head: u64,
    /// History watermark published by the staged `history.sqlite`.
    #[serde(default, alias = "history_watermark")]
    pub(super) history_watermark: u64,
    /// Wall clock of the staged checkpoint; a resumed re-export must reproduce it.
    #[serde(default, alias = "checkpoint_saved_at")]
    pub(super) checkpoint_saved_at: i64,
    #[serde(default, alias = "item_count")]
    pub(super) item_count: u64,
    #[serde(default, alias = "turn_count")]
    pub(super) turn_count: u64,
    #[serde(default, alias = "attachment_count")]
    pub(super) attachment_count: u64,
    /// Content hashes of every migrated attachment blob, in catalog order.
    #[serde(default, alias = "attachment_hashes")]
    pub(super) attachment_hashes: Vec<String>,
}

/// Durable migration state; the single source of truth for resume decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MigrationState {
    #[serde(alias = "schema_version")]
    pub(super) schema_version: u32,
    #[serde(default = "default_phase")]
    pub(super) phase: MigrationPhase,
    #[serde(default, alias = "updated_at")]
    pub(super) updated_at: i64,
    /// How this conversion was started: normal startup or the explicit operator command. Recorded
    /// so a one-time conversion is auditable after the fact.
    #[serde(default, alias = "trigger")]
    pub(super) trigger: MigrationTrigger,
    /// Recorded consistency-backup directory, kept for post-migration diagnostics.
    #[serde(default, alias = "backup_dir")]
    pub(super) backup_dir: Option<String>,
    #[serde(default)]
    pub(super) sources: Vec<SourceFingerprint>,
    #[serde(default)]
    pub(super) sessions: Vec<SessionMigrationRecord>,
    /// 迁移后 `catalog.toml` 中重新生成的目录摘要条目数。
    #[serde(default, alias = "catalog_entry_count")]
    pub(super) catalog_entry_count: u64,
    /// 发布时 `catalog.toml` 的 CAS revision；`None` 表示尚未生成。
    #[serde(default, alias = "catalog_revision")]
    pub(super) catalog_revision: Option<u64>,
    /// 发布的 `workspaces.toml` / `settings.toml` 条目数（审计用）。
    #[serde(default, alias = "workspace_entry_count")]
    pub(super) workspace_entry_count: u64,
    #[serde(default, alias = "settings_entry_count")]
    pub(super) settings_entry_count: u64,
    /// 退役产品调用库的一次性导入审计；`None` 表示该安装从未有过退役调用库。
    #[serde(default)]
    pub(super) calls: Option<CallsImportReport>,
    /// 退役调用库源字节的只读聚合指纹（主文件 + `-wal` + `blobs` 正文，跨实时源与字节归档解析）；
    /// phase-1 记录，phase-3/phase-4 复算并要求一致。
    #[serde(default, alias = "calls_source_fingerprint")]
    pub(super) calls_source_fingerprint: Option<String>,
    /// 退役调用库导入目标（staging 或已发布 canonical `calls/calls.sqlite`）的字节身份：主文件 +
    /// `-wal` + `blobs` 正文。phase-4 校验通过后记录；续跑时复算并要求一致，从而无需重读旧会话
    /// journal 即可证明目标未被篡改，任一变即 fail closed。
    #[serde(default, alias = "calls_destination_fingerprint")]
    pub(super) calls_destination_fingerprint: Option<String>,
    #[serde(default)]
    pub(super) error: Option<String>,
    /// Schema version read from disk before in-memory normalization.
    #[serde(skip)]
    pub(super) loaded_schema_version: u32,
}

fn default_phase() -> MigrationPhase {
    MigrationPhase::Detected
}

impl MigrationState {
    pub(super) fn new(
        sources: Vec<SourceFingerprint>,
        updated_at: i64,
        trigger: MigrationTrigger,
    ) -> Self {
        Self {
            schema_version: MIGRATION_STATE_SCHEMA_VERSION,
            phase: MigrationPhase::Detected,
            updated_at,
            trigger,
            backup_dir: None,
            sources,
            sessions: Vec::new(),
            catalog_entry_count: 0,
            catalog_revision: None,
            workspace_entry_count: 0,
            settings_entry_count: 0,
            calls: None,
            calls_source_fingerprint: None,
            calls_destination_fingerprint: None,
            error: None,
            loaded_schema_version: MIGRATION_STATE_SCHEMA_VERSION,
        }
    }

    pub(super) fn needs_upgrade(&self) -> bool {
        self.loaded_schema_version != MIGRATION_STATE_SCHEMA_VERSION
    }

    pub(super) fn upsert_record(&mut self, record: SessionMigrationRecord) {
        match self
            .sessions
            .iter_mut()
            .find(|existing| existing.thread_id == record.thread_id)
        {
            Some(existing) => *existing = record,
            None => self.sessions.push(record),
        }
    }
}

pub(super) async fn load(path: &Path) -> Result<Option<MigrationState>> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => {
            let value: serde_json::Value = serde_json::from_str(&content)
                .with_context(|| format!("invalid migration state {}", path.display()))?;
            let version = value
                .get("schemaVersion")
                .or_else(|| value.get("schema_version"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|version| u32::try_from(version).ok())
                .with_context(|| {
                    format!("migration state has no schema version {}", path.display())
                })?;
            ensure!(
                (MIN_SUPPORTED_MIGRATION_STATE_SCHEMA_VERSION..=MIGRATION_STATE_SCHEMA_VERSION)
                    .contains(&version),
                "unsupported migration state schema {version}; existing data preserved"
            );
            let mut state: MigrationState = serde_json::from_value(value)
                .with_context(|| format!("invalid migration state {}", path.display()))?;
            state.loaded_schema_version = version;
            normalize(&mut state)?;
            Ok(Some(state))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Atomically writes the durable migration report.
///
/// Deliberately not an `async fn`: the phase machine calls this through functions whose own
/// `state_path: &Path`/`state: &MigrationState` parameters carry generic lifetimes, so an `async fn`
/// here would produce a future that captures those borrowed parameters, and a `tokio::spawn`ed
/// caller cannot prove such a future is `Send` for every reference lifetime (`Send is not general
/// enough`). Consuming the arguments into owned values before the future exists keeps the returned
/// future `Send + 'static` regardless of the caller's lifetimes. What is written — pretty JSON,
/// atomic replacement via a blocking task — is unchanged, and a serialization failure still surfaces
/// as the future's error.
pub(super) fn store(
    path: &Path,
    state: &MigrationState,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let path = path.to_path_buf();
    let contents = serde_json::to_vec_pretty(state);
    async move {
        let contents = contents?;
        let directory = path
            .parent()
            .context("migration state path has no parent directory")?
            .to_path_buf();
        tokio::fs::create_dir_all(&directory).await?;
        tokio::task::spawn_blocking(move || {
            pl_tool::workspace::write_file_atomically(&path, &contents)
        })
        .await??;
        Ok(())
    }
}

fn normalize(state: &mut MigrationState) -> Result<()> {
    ensure!(
        state.schema_version <= MIGRATION_STATE_SCHEMA_VERSION,
        "unsupported migration state schema {}; existing data preserved",
        state.schema_version
    );
    state.schema_version = MIGRATION_STATE_SCHEMA_VERSION;
    for session in &mut state.sessions {
        ensure!(
            !session.thread_id.is_empty(),
            "migration state contains a session without identity"
        );
        if session.storage_key.is_empty() {
            session.storage_key = thread_storage_key(&session.thread_id);
        }
    }
    Ok(())
}
