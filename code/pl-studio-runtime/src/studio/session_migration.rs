//! One-time, phased and resumable legacy storage migration.
//!
//! Normal runtime paths never open the legacy databases: activation, queries and subscriptions read
//! only the current layout. The shared `sessions.sqlite` and the versioned product database are
//! opened here, inside a coordinator whose progress is persisted before and after every mutating
//! step (`design/17` §17.7). Every legacy session is really traversed: its journal is replayed with
//! the legacy decoder, projected into timeline items/turns and written to staged current-layout
//! facts (`state.toml`, per-session `history.sqlite`, global `calls.sqlite`, attachments), while the
//! retired product tables are converted into the three canonical TOML documents
//! (`settings.toml`, `workspaces.toml`, `catalog.toml`). Nothing is published until every session
//! verifies, and any failure keeps the original bytes and resumes from the last durable phase.

mod export;
mod report;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, EntityTrait,
    QueryOrder, Statement, Value,
};

use self::export::{export_session, load_staged_catalog, staged_session_dir, verify_session};
use self::report::{MigrationPhase, MigrationState, SessionMigrationStatus, SourceFingerprint};
use super::paths::{self, StudioPaths};
use super::runtime_lock::{RuntimeLock, StudioHostKind};
use super::store_support;
use crate::studio::catalog::{CatalogEntry, CatalogStore};
use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::mappers::thread_record;
use crate::studio::records::ThreadRecord;
use crate::studio::store::settings::{SettingEntry, SettingsStore};
use crate::studio::store::workspaces::{WorkspaceEntry, WorkspaceStore};

const LEGACY_SESSIONS_FILE_NAME: &str = "sessions.sqlite";
/// Retired per-product call database name; it lived next to the product database.
const LEGACY_CALLS_FILE_NAME: &str = "calls.sqlite";
const RESET_MARKER_FILE_NAME: &str = "session-reset.json";
/// 布局切换开始前写入、`Published` 落盘后删除的 durable 公告。
///
/// 多根切换（三份 canonical 文档 + `sessions/` + `calls/`）无法用单次 rename 完成，报告本身也只
/// 由阶段推进写入；一旦报告在切换中途丢失/损坏，普通启动就无法区分"已切换一半的安装"和"现役安装
/// 缺目录"。这个公告是那次切换的独立证据：存在且没有可恢复报告时一律 fail closed。
const LAYOUT_PUBLICATION_MARKER_FILE_NAME: &str = "layout-publication.json";
/// Staged layout lives next to its final location so publication is a single directory rename.
const STAGING_DIR_NAME: &str = "sessions.staging";
const CALLS_STAGING_DIR_NAME: &str = "calls.staging";
const ARCHIVE_DIR_NAME: &str = "session-archive";
/// Subdirectory of the phase-1 backup holding the byte-preserved retired call store and its blobs.
const LEGACY_CALLS_BACKUP_DIR_NAME: &str = "calls";
/// Migration workspace for the retired call store's format upgrade; never published.
const LEGACY_CALLS_WORK_DIR_NAME: &str = "calls-import-work";

/// How a one-time legacy conversion was started.
///
/// This is the only evidence that can separate an unmigrated pre-`catalog.toml` installation from a
/// current installation that lost its canonical documents: both have the same product schema and the
/// same product tables, so the operator's explicit intent must come from outside the home. It is
/// recorded in the durable migration report so a one-time conversion stays auditable afterwards.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum MigrationTrigger {
    /// Normal startup: convert only when the home itself carries an explicit legacy signal, and fail
    /// closed otherwise.
    #[default]
    Startup,
    /// Explicit operator command through the `pl-studio-server` CLI.
    Explicit,
}

/// Result of an explicitly invoked one-time conversion; rendered by the CLI and kept auditable
/// through the durable migration report.
#[derive(Debug, Clone)]
pub struct LegacyMigrationOutcome {
    /// Resolved Studio home that was converted.
    pub home: PathBuf,
    /// Durable migration report recording trigger, phase, backup and failures.
    pub report_path: PathBuf,
    /// Persisted phase label; `None` when no report was written yet.
    pub phase: Option<&'static str>,
    pub backup_dir: Option<String>,
    pub workspace_entries: u64,
    pub settings_entries: u64,
    pub catalog_entries: u64,
    pub catalog_revision: Option<u64>,
    /// Retired per-product call database audit: source shape/counts and whether it verified.
    pub calls_source_present: bool,
    pub calls_source_schema_version: i64,
    pub calls_source_database_id: Option<String>,
    pub calls_source_model_calls: u64,
    pub calls_source_tool_calls: u64,
    pub calls_source_watermarks: u64,
    pub calls_source_bodies: u64,
    pub calls_verified_bodies: u64,
    pub calls_destination_model_calls: u64,
    pub calls_destination_tool_calls: u64,
    pub calls_verified: bool,
    pub staged_sessions: usize,
    pub verified_sessions: usize,
    /// Recorded failure; the CLI must exit non-zero when this is set.
    pub error: Option<String>,
}

impl LegacyMigrationOutcome {
    pub fn is_failure(&self) -> bool {
        self.error.is_some()
    }
}

/// Runs the explicit one-time conversion for `confirmed_home` and returns its durable outcome.
///
/// The command shares the whole phase machine with normal startup (exclusive runtime lock →
/// validation → backup → staged export → verification → atomic publication → archive) and never
/// constructs `StudioRuntime`, opens the store, or starts a session or subscription. It exits after
/// the conversion; a failed conversion leaves every original byte in place and records the failure in
/// the migration report.
pub async fn migrate_legacy_storage(
    studio_home: Option<PathBuf>,
    confirmed_home: PathBuf,
) -> anyhow::Result<LegacyMigrationOutcome> {
    let paths = StudioPaths::resolve(studio_home)?;
    let resolved_home = std::fs::canonicalize(paths.home()).with_context(|| {
        format!(
            "explicit legacy conversion requires an existing Studio home: {}",
            paths.home().display()
        )
    })?;
    let confirmed = std::fs::canonicalize(&confirmed_home).with_context(|| {
        format!(
            "explicit legacy conversion requires an existing --confirm path: {}",
            confirmed_home.display()
        )
    })?;
    ensure!(
        resolved_home == confirmed,
        "explicit legacy conversion refused: --confirm {} does not match the resolved Studio home \
         {}; nothing was changed",
        confirmed.display(),
        resolved_home.display()
    );
    // Refuse before creating any directory or lock file: a home without its original product
    // database has nothing to convert. The coordinator re-checks this under the lock.
    ensure!(
        tokio::fs::try_exists(&paths.database()).await?,
        "explicit legacy conversion refused: no original product database at {}; nothing was changed",
        paths.database().display()
    );

    let lock_path = paths.runtime_lock();
    let lock = tokio::task::spawn_blocking(move || {
        RuntimeLock::acquire(&lock_path, StudioHostKind::HttpServer)
    })
    .await
    .map_err(|error| anyhow::anyhow!("explicit legacy conversion lock task failed: {error}"))?
    .map_err(|error| {
        anyhow::anyhow!(
            "explicit legacy conversion could not acquire the Studio runtime lock ({:?}): {}",
            error.code,
            error.message
        )
    })?;
    let report_path = migration_state_path(&paths);
    let outcome = run_migration(paths.clone(), &lock, MigrationTrigger::Explicit).await;
    // Hold the exclusive lock until the durable report has been read back for the audit summary.
    let recorded = report::load(&report_path).await?;
    drop(lock);
    Ok(summarize_outcome(
        resolved_home,
        report_path,
        recorded,
        outcome.err(),
    ))
}

/// Normal-startup entry: converts only when the home carries an explicit legacy signal and fails
/// closed otherwise. Both entries share the phase machine in [`run_migration`].
///
/// Owns the resolved paths for the whole run so the returned future carries no borrowed
/// `&StudioPaths` across a suspension point: a spawned caller requires this future to be `Send` for
/// every reference lifetime, and owning the paths keeps every derived `&Path` a borrow of a local the
/// future holds rather than a reference parameter.
pub(super) async fn prepare(paths: StudioPaths, owner: &RuntimeLock) -> Result<()> {
    run_migration(paths, owner, MigrationTrigger::Startup).await
}

/// The one-time conversion phase machine shared by normal startup and the explicit command.
async fn run_migration(
    paths: StudioPaths,
    _owner: &RuntimeLock,
    trigger: MigrationTrigger,
) -> Result<()> {
    let product = paths.database();
    let sessions = paths.legacy_sessions_database();
    let migrations_dir = paths.migrations_dir();
    let state_path = migration_state_path(&paths);
    let staging = paths.home().join(STAGING_DIR_NAME);
    let calls_staging = paths.home().join(CALLS_STAGING_DIR_NAME);

    // The exclusive runtime lock is already held; this per-database lease additionally keeps an old
    // writer from reopening the retired repository while it is being converted. The coordinator owns
    // the lease for the whole run; its own readers adopt a clone of it through
    // `SqliteSessionStore::open_with_lock` instead of re-locking the same file (which would self-block).
    let sessions_lease = lock_file(&sessions.with_extension("sqlite.lock")).await?;
    ensure!(
        !tokio::fs::try_exists(product.with_file_name(RESET_MARKER_FILE_NAME)).await?,
        "unfinished legacy destructive reset requires recovery; all data preserved"
    );

    let recorded = report::load(&state_path).await?;
    // A durable announcement of an unfinished layout switch outranks every other classification:
    // canonical documents and the `sessions/`/`calls/` roots are switched step by step, so without
    // this evidence a home could look like a current installation while only half its layout was
    // published. A resumable report keeps the normal resume path; a report that already says
    // `Published` (written after the archive) makes the announcement stale; anything else refuses to
    // start on the partially switched home and preserves every original byte.
    if layout_publication_pending(&paths).await? {
        match recorded.as_ref() {
            // Resumable publication: the phase machine finishes the idempotent switch under this lock.
            Some(state) if state.phase.order() < MigrationPhase::Published.order() => {}
            // The report already records `Published` (written after the archive), so only the commit
            // step was left: retire the announcement now instead of blocking a complete layout.
            Some(_) => commit_layout_publication(&paths).await?,
            None => bail!(
                "Studio layout publication was interrupted before its migration report was durable \
                 ({}); refusing to start or convert a partially switched home. Restore the migration \
                 report or the backup it references, then retry; every original byte is preserved.",
                layout_publication_marker_path(&paths).display()
            ),
        }
    }
    let product_version = inspect(&product).await?;
    let session_version = inspect(&sessions).await?;
    ensure!(
        product_version.is_none_or(|v| matches!(v, 20..=22)),
        "unsupported Studio migration path; existing data preserved"
    );
    ensure!(
        session_version.is_none_or(|v| matches!(v, 6 | 7)),
        "unsupported session migration path; existing data preserved"
    );

    // Classify the home before touching anything. A normal startup never converts and never rebuilds:
    // the retired tables are read only by an explicit legacy conversion, and canonical documents are
    // never regenerated for a home that already owns state (rebuilding would either mask a lost fact
    // source or silently rescan every session).
    let resumable = recorded
        .as_ref()
        .is_some_and(|state| state.phase != MigrationPhase::Published);
    let legacy_product = product_version
        .is_some_and(|version| version < store_support::STUDIO_DATABASE_SCHEMA_VERSION);
    let legacy_layout = legacy_layout_sources(&paths).await?;
    // Any current-layout artifact proves this home already ran the current layout or started the
    // one-time conversion. A residual retired artifact (for example a leftover `studio/calls.sqlite`)
    // must never re-trigger a conversion or rebuild a missing canonical document for such a home.
    let current_layout = current_layout_evidence(&paths).await?;

    match trigger {
        MigrationTrigger::Startup => {
            // A fresh home has no product database, no retired session journal, no migration state
            // and no legacy layout artifact. It must not open, create or rewrite migration state;
            // `StudioStore` materializes the three empty canonical documents when it opens.
            if product_version.is_none()
                && session_version.is_none()
                && recorded.is_none()
                && !legacy_layout
                && !current_layout
            {
                return Ok(());
            }

            // Explicit, resumable legacy conversion: a retired shared session journal, a
            // pre-`catalog.toml` legacy layout (old product schema, retired per-product call
            // database, retired global attachment root) or a persisted migration that has not
            // finished. Only this path may read the retired product tables or write canonical
            // documents.
            let legacy_conversion = session_version.is_some()
                || resumable
                || (!current_layout && (legacy_product || legacy_layout));
            if !legacy_conversion {
                // No explicit legacy signal: this home already published the current layout (or only
                // ever ran it). Its canonical documents are the fact source, so a missing one is data
                // loss rather than a migration. Fail closed, keep every original byte, and never
                // rebuild it from SQLite or by scanning sessions.
                return require_canonical_documents(&paths).await;
            }
        }
        MigrationTrigger::Explicit => {
            // The operator asserts this home is an unmigrated pre-`catalog.toml` installation. The
            // assertion is validated against the home before any mutation, because a current
            // installation must never be converted or overwritten.
            validate_explicit_conversion(&paths, recorded.as_ref()).await?;
        }
    }

    let fingerprints = vec![fingerprint(&product).await?, fingerprint(&sessions).await?];
    let mut state = match recorded {
        Some(mut state) => {
            if let Some(error) = &state.error {
                tracing::warn!(
                    phase = ?state.phase,
                    updated_at = state.updated_at,
                    %error,
                    "resuming one-time legacy migration after a recorded failure"
                );
            }
            // Only the un-mutated `Detected` phase can be re-validated against its sources.
            if state.phase == MigrationPhase::Detected {
                verify_sources_unchanged(&state, &fingerprints)?;
            }
            // Record an explicit conversion, and never downgrade a recorded explicit trigger: the
            // report must keep showing how the one-time conversion was actually started.
            let explicit_trigger = trigger == MigrationTrigger::Explicit
                && state.trigger != MigrationTrigger::Explicit;
            if explicit_trigger {
                state.trigger = MigrationTrigger::Explicit;
            }
            if state.needs_upgrade() || explicit_trigger {
                report::store(&state_path, &state).await?;
                state.loaded_schema_version = state.schema_version;
            }
            state
        }
        None => {
            let state = MigrationState::new(fingerprints.clone(), unix_seconds(), trigger);
            report::store(&state_path, &state).await?;
            state
        }
    };

    let outcome: Result<()> = async {
        backup(
            &mut state,
            state_path.clone(),
            paths.clone(),
            sessions.clone(),
            product.clone(),
            product_version,
            migrations_dir.clone(),
        )
        .await?;
        upgrade_legacy_schemas(
            &mut state,
            state_path.clone(),
            sessions.clone(),
            product.clone(),
            session_version,
        )
        .await?;
        export_legacy_sessions(
            &mut state,
            ExportInputs {
                state_path: state_path.clone(),
                paths: paths.clone(),
                sessions: sessions.clone(),
                sessions_lease: sessions_lease.try_clone()?,
                product: product.clone(),
                staging: staging.clone(),
                calls_staging: calls_staging.clone(),
            },
        )
        .await?;
        verify_exported_sessions(
            &mut state,
            state_path.clone(),
            staging.clone(),
            calls_staging.clone(),
            sessions.clone(),
            sessions_lease.try_clone()?,
            paths.clone(),
        )
        .await?;
        publish(
            &mut state,
            state_path.clone(),
            paths.clone(),
            sessions.clone(),
            product.clone(),
            staging.clone(),
            calls_staging.clone(),
        )
        .await?;
        Ok(())
    }
    .await;

    if let Err(error) = outcome {
        // Best-effort durable failure record; the original error is always returned.
        state.error = Some(format!("{error:#}"));
        state.updated_at = unix_seconds();
        if let Err(record) = report::store(&state_path, &state).await {
            tracing::warn!(error = %record, "failed to record migration failure state");
        }
        return Err(error);
    }
    Ok(())
}

/// Phase 1: a consistent, non-overwriting backup of every legacy source.
async fn backup(
    state: &mut MigrationState,
    state_path: PathBuf,
    paths: StudioPaths,
    sessions: PathBuf,
    product: PathBuf,
    product_version: Option<i64>,
    migrations_dir: PathBuf,
) -> Result<()> {
    // The phase futures are held inside the spawned startup task, so they must own every input and
    // be `Send`: a future that captures a borrowed parameter cannot prove `Send` for all reference
    // lifetimes (`Send is not general enough`). Each owned parameter is reborrowed through a local
    // of the same type, so the body below is unchanged.
    let state_path = state_path.as_path();
    let paths = &paths;
    let sessions = sessions.as_path();
    let product = product.as_path();
    let migrations_dir = migrations_dir.as_path();
    if state.phase.order() >= MigrationPhase::BackedUp.order() {
        return Ok(());
    }
    // A stable, recorded path makes the backup idempotent: a resumed run reuses the completed
    // backup instead of producing another copy. Only a genuinely absent backup is (re)created.
    let backup_dir = match state.backup_dir.clone() {
        Some(recorded) => PathBuf::from(recorded),
        None => migrations_dir.join("session-backup"),
    };
    tokio::fs::create_dir_all(&backup_dir).await?;
    // Consistency backup precedes any conversion; an existing valid backup is reused as-is. A source
    // that does not exist has nothing to preserve (the retired product database, for example, is the
    // only legacy source of an installation that never used the shared session journal).
    if tokio::fs::try_exists(sessions).await? {
        backup_database(sessions, &backup_dir.join(LEGACY_SESSIONS_FILE_NAME)).await?;
    }
    if product_version.is_some() {
        backup_database(product, &backup_dir.join("product.sqlite")).await?;
    }
    // The retired per-product call store is snapshotted byte-for-byte, together with its `-wal`
    // sidecar and content-addressed blobs, before anything normalizes it. The original files are only
    // ever read; a recognized older format is upgraded on a derivative workspace, so a crash or a
    // failed conversion can never change the source bytes. The snapshot also binds the phase-4
    // re-verification to the exact source that publication will archive. The identity is aggregated
    // over the live source location and the byte-preserved archive so a resumed run whose earlier
    // publication crashed after renaming only some members still resolves to the same bytes.
    let legacy_calls = legacy_product_calls_database(paths);
    let archived_calls = archived_legacy_call_store(paths);
    if tokio::fs::try_exists(&legacy_calls).await? || tokio::fs::try_exists(&archived_calls).await?
    {
        // A backup needs a live source to copy: an archive without a live store at this phase is an
        // unexplained state and is refused rather than silently trusted.
        ensure!(
            tokio::fs::try_exists(&legacy_calls).await?,
            "retired call store archive exists without a live source during backup; existing data \
             preserved"
        );
        // Snapshot the retired store, then prove that (a) the source did not change while it was copied
        // and (b) the backup bytes are exactly the recorded source identity. A concurrent writer or a
        // stale/partial backup fails closed instead of silently migrating bytes that no longer match
        // what publication will archive.
        let before =
            super::storage::calls::legacy_call_store_identity(&legacy_calls, Some(&archived_calls))
                .await?;
        ensure!(
            before.present,
            "retired call store identity is absent during backup; existing data preserved"
        );
        let snapshot = super::storage::calls::snapshot_legacy_call_store(
            &legacy_calls,
            &backup_dir.join(LEGACY_CALLS_BACKUP_DIR_NAME),
        )
        .await?;
        let after =
            super::storage::calls::legacy_call_store_identity(&legacy_calls, Some(&archived_calls))
                .await?;
        ensure!(
            before.fingerprint == after.fingerprint,
            "retired call store changed while it was being backed up; existing data preserved"
        );
        let snapshot_fingerprint =
            super::storage::calls::legacy_call_store_fingerprint(&snapshot).await?;
        ensure!(
            snapshot_fingerprint == after.fingerprint,
            "retired call store backup does not match its source ({}) and will not be trusted; \
             existing data preserved",
            snapshot.display()
        );
        state.calls_source_fingerprint = Some(after.fingerprint);
    }
    state.phase = MigrationPhase::BackedUp;
    state.backup_dir = Some(backup_dir.to_string_lossy().into_owned());
    state.updated_at = unix_seconds();
    report::store(state_path, state).await
}

/// Phase 2: in-place legacy schema upgrades. Idempotent, so a resumed run is a no-op.
async fn upgrade_legacy_schemas(
    state: &mut MigrationState,
    state_path: PathBuf,
    sessions: PathBuf,
    product: PathBuf,
    session_version: Option<i64>,
) -> Result<()> {
    let state_path = state_path.as_path();
    let sessions = sessions.as_path();
    let product = product.as_path();
    if state.phase.order() >= MigrationPhase::Upgraded.order() {
        return Ok(());
    }
    if session_version == Some(6) {
        pl_core::persistence::migration::migrate_v6(
            pl_core::persistence::SqliteSessionOptions {
                path: sessions.to_path_buf(),
            },
            super::collaboration_migration::convert,
        )
        .await?;
    }
    if inspect(product).await?.is_some() {
        // The product database is upgraded in place: its schema, not its bytes, changes, and the
        // pre-upgrade copy already exists in the recorded backup.
        let db = connect(product, DatabaseAccess::ReadWrite).await?;
        let result = async {
            store_support::upgrade_product_schema(&db).await?;
            super::store::ssh_migration::migrate_ssh_servers_to_user_config(
                &db,
                &pl_tool::remote::SshConfigFile::user_default()?,
            )
            .await
        }
        .await;
        finish_connection(db, result).await?;
        ensure!(
            inspect(product).await? == Some(store_support::STUDIO_DATABASE_SCHEMA_VERSION),
            "product schema upgrade did not reach the current version; existing data preserved"
        );
    }
    state.phase = MigrationPhase::Upgraded;
    state.updated_at = unix_seconds();
    report::store(state_path, state).await
}

/// Phase-3（export）的相位输入边界。
///
/// 把这批 owned 参数聚成一个值，既让导出相位不再以 8 个松散参数触发
/// `clippy::too_many_arguments`，也保持 future 只捕获 owned 值以证明 `Send`（启动任务要求）。
/// `sessions_lease` 是旧会话库锁的 clone，随本值一起跨线程移动，锁生命周期不缩短。
struct ExportInputs {
    state_path: PathBuf,
    paths: StudioPaths,
    sessions: PathBuf,
    sessions_lease: std::fs::File,
    product: PathBuf,
    staging: PathBuf,
    calls_staging: PathBuf,
}

/// Phase 3: traverse every legacy session and stage the converted current layout.
async fn export_legacy_sessions(state: &mut MigrationState, inputs: ExportInputs) -> Result<()> {
    let ExportInputs {
        state_path,
        paths,
        sessions,
        sessions_lease,
        product,
        staging,
        calls_staging,
    } = inputs;
    let state_path = state_path.as_path();
    let paths = &paths;
    let sessions = sessions.as_path();
    let sessions_lease = &sessions_lease;
    let product = product.as_path();
    let staging = staging.as_path();
    let calls_staging = calls_staging.as_path();
    if state.phase.order() >= MigrationPhase::Exported.order() {
        return Ok(());
    }
    let product_db = connect(product, DatabaseAccess::ReadWrite).await?;
    // An installation that never used the shared session journal has nothing session-scoped to
    // stage: its retired product directory is converted during publication. The staging areas are
    // still prepared so publication stays one atomic directory rename per layout root.
    let legacy = match tokio::fs::try_exists(sessions).await? {
        true => Some(open_legacy_sessions(sessions, sessions_lease).await?),
        false => None,
    };
    let conversion = async {
        ensure!(
            !tokio::fs::try_exists(paths.sessions_dir()).await?,
            "current session layout already exists before publication; existing data preserved"
        );
        // A resumed run recreates the staging area from scratch: conversion is deterministic, so the
        // staged bytes and the durable manifest are identical either way.
        remove_dir_if_present(staging).await?;
        tokio::fs::create_dir_all(staging).await?;
        remove_dir_if_present(calls_staging).await?;
        tokio::fs::create_dir_all(calls_staging).await?;
        if let Some(legacy) = legacy.as_ref() {
            let calls =
                super::storage::calls::CallsStore::open(&calls_staging.join("calls.sqlite"))
                    .await?;
            let ids = legacy
                .session_ids()
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            for thread_id in ids {
                // `export_session` fails closed on a session that holds records but no convertible
                // Thread commits, so no legacy session is silently dropped from the published layout.
                let updated_at = thread_updated_at(&product_db, &thread_id).await?;
                let conversion =
                    export_session(legacy, &calls, &product_db, staging, &thread_id, updated_at)
                        .await?;
                state.upsert_record(conversion.record);
                state.updated_at = unix_seconds();
                report::store(state_path, state).await?;
            }
            // 固定 ticket 排空后停止调用库 writer：目录重命名前所有调用事实与 blob 都已落盘，
            // 不会在发布期间继续写入已迁出的路径。
            calls.shutdown().await?;
        }
        // 退役的每产品调用库在此合并进 staging 调用库：journal 重放无法复现的计费/正文事实只归档
        // 会丢失，因此必须先导入并逐项校验。staging writer 已在上一步停止，本次是唯一 writer。
        import_legacy_call_store(
            state,
            state_path.to_path_buf(),
            paths.clone(),
            calls_staging.to_path_buf(),
        )
        .await?;
        Ok(())
    }
    .await;
    let shutdown = match legacy.as_ref() {
        Some(legacy) => legacy
            .shutdown()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string())),
        None => Ok(()),
    };
    let result = combine_conversion_and_shutdown(conversion, shutdown);
    finish_connection(product_db, result).await?;
    state.phase = MigrationPhase::Exported;
    state.updated_at = unix_seconds();
    report::store(state_path, state).await
}

/// 把退役的每产品调用库合并进 staged 调用库，并把审计结果写进 durable migration report。
///
/// 只在 export 阶段、staging 调用库 writer 已停止后调用，因此本函数是目标库当时唯一的 writer。
/// 数据只在 phase-1 备份出的字节副本上归一化并导入；原始退役库从不被打开或改写。源库缺失时是
/// no-op；存在但无法识别、正文缺失/损坏或与目标库冲突时 fail closed，保留原字节并把失败写进报告。
async fn import_legacy_call_store(
    state: &mut MigrationState,
    state_path: PathBuf,
    paths: StudioPaths,
    calls_staging: PathBuf,
) -> Result<()> {
    let state_path = state_path.as_path();
    let paths = &paths;
    let calls_staging = calls_staging.as_path();
    let source = legacy_product_calls_database(paths);
    if !tokio::fs::try_exists(&source).await? {
        return Ok(());
    }
    let snapshot = legacy_call_snapshot_database(state)?;
    let fingerprint = state
        .calls_source_fingerprint
        .clone()
        .context("retired call store fingerprint missing before import; existing data preserved")?;
    // The live store (or, after a crashed publication, the split source/archive bytes) must still be
    // the one that was snapshotted before anything is merged; a change here fails closed rather than
    // importing bytes that publication would then archive as different.
    let archived = archived_legacy_call_store(paths);
    let live = super::storage::calls::legacy_call_store_identity(&source, Some(&archived)).await?;
    ensure!(
        live.present && live.fingerprint == fingerprint,
        "retired call store changed since it was backed up; existing data preserved"
    );
    let destination = calls_staging.join(LEGACY_CALLS_FILE_NAME);
    let work_dir = paths.migrations_dir().join(LEGACY_CALLS_WORK_DIR_NAME);
    let audit = super::storage::calls::merge_legacy_call_store(
        &snapshot,
        &destination,
        &work_dir,
        &fingerprint,
    )
    .await?;
    ensure!(
        audit.verified,
        "retired call store import did not verify; existing data preserved"
    );
    state.calls = Some(audit);
    state.updated_at = unix_seconds();
    report::store(state_path, state).await
}

/// Phase-4 重新推导退役调用库审计，并把它绑定到保留快照与当前 staging 目标库。
///
/// 与 export 阶段的持久 `verified` 标志无关：每次进入 phase-4 都从保留快照重新归一化、重新校验
/// staged 目标库覆盖源事实，并复算源指纹要求与 phase-1 记录一致，因此源库或 staging 在 export 之后
/// 被改动都会 fail closed。该检查独立于共享会话 journal，没有旧会话库的安装同样执行。结果写进
/// durable report，`Verified` 相位只在校验通过后才推进。
async fn verify_legacy_call_audit(
    state: &mut MigrationState,
    state_path: PathBuf,
    paths: StudioPaths,
    calls_staging: PathBuf,
) -> Result<()> {
    let state_path = state_path.as_path();
    let paths = &paths;
    let calls_staging = calls_staging.as_path();
    let source = legacy_product_calls_database(paths);
    let archived = archived_legacy_call_store(paths);
    // Presence and identity are resolved over the live source *and* the byte-preserved archive: a
    // resumed run whose earlier publication already retired some members must still see a complete
    // store, while a member that appears in both locations is refused as ambiguous.
    let live = super::storage::calls::legacy_call_store_identity(&source, Some(&archived)).await?;
    let recorded_present = state
        .calls
        .as_ref()
        .is_some_and(|audit| audit.source_present);
    ensure!(
        live.present == recorded_present,
        "retired call store was not imported by this migration, or its presence changed after \
         export; existing data preserved"
    );
    let recorded_source = if live.present {
        let recorded = state
            .calls_source_fingerprint
            .clone()
            .context("retired call store fingerprint missing; existing data preserved")?;
        ensure!(
            !recorded.is_empty(),
            "retired call store fingerprint is absent in the migration report; existing data \
             preserved"
        );
        ensure!(
            live.fingerprint == recorded,
            "retired call store source changed since it was snapshotted; existing data preserved"
        );
        Some(recorded)
    } else {
        None
    };
    // The destination is whichever call store a partial publish left live: the canonical
    // `calls/calls.sqlite` once the staging directory was renamed into place, otherwise the staged
    // store. Verifying the *actual* target keeps a resumed run honest about its content/watermarks/
    // bodies even after the rename, and fails closed if neither exists.
    let destination = call_store_destination(paths, calls_staging).await?;
    let destination = match (destination, live.present) {
        (Some(destination), _) => destination,
        (None, true) => bail!(
            "neither the staged nor the published call store exists after export; existing data \
             preserved"
        ),
        (None, false) => return Ok(()),
    };
    ensure_call_destination_verified(
        state,
        state_path.to_path_buf(),
        paths.clone(),
        destination,
        recorded_source,
    )
    .await
}

/// Re-validates the actual call-store destination and binds its **logical content** to the durable
/// report.
///
/// The identity is a logical content digest (rows + content-addressed bodies), not the SQLite file
/// bytes: the destination is opened and closed many times during the migration, so a WAL checkpoint
/// can rewrite its physical layout without changing any fact. A recorded destination identity that
/// still matches therefore proves the target holds the same facts that were verified against the
/// preserved source and the old session export, so a resumed run continues without re-normalizing the
/// snapshot. Any fact change — a tampered row, watermark or body — changes the identity and stops here
/// with every original byte preserved. When no identity is recorded yet the target is re-verified from
/// the preserved snapshot (retired-source installations) before its facts are bound; journal-only
/// installations have no snapshot to re-derive from and simply bind the facts the completed export
/// produced.
async fn ensure_call_destination_verified(
    state: &mut MigrationState,
    state_path: PathBuf,
    paths: StudioPaths,
    destination: PathBuf,
    recorded_source: Option<String>,
) -> Result<()> {
    let state_path = state_path.as_path();
    let paths = &paths;
    let destination = destination.as_path();
    let recorded_source = recorded_source.as_deref();
    let destination_fingerprint =
        super::storage::calls::calls_store_fingerprint(destination).await?;
    if let Some(recorded_destination) = state.calls_destination_fingerprint.as_deref() {
        ensure!(
            recorded_destination == destination_fingerprint,
            "published call store changed after it was verified ({}); existing data preserved",
            destination.display()
        );
        return Ok(());
    }
    if let Some(recorded_source) = recorded_source {
        ensure!(
            state.calls.as_ref().is_some_and(|audit| audit.verified),
            "call store destination is unbound but the retired audit is not verified; existing data \
             preserved"
        );
        let snapshot = legacy_call_snapshot_database(state)?;
        let work_dir = paths.migrations_dir().join(LEGACY_CALLS_WORK_DIR_NAME);
        let audit = super::storage::calls::verify_legacy_call_store(
            &snapshot,
            destination,
            &work_dir,
            recorded_source,
        )
        .await?;
        ensure!(
            audit.verified,
            "retired call store verification did not complete; existing data preserved"
        );
        state.calls = Some(audit);
    }
    state.calls_destination_fingerprint =
        Some(super::storage::calls::calls_store_fingerprint(destination).await?);
    state.updated_at = unix_seconds();
    report::store(state_path, state).await
}

/// phase-1 记录下来的退役调用库字节副本路径；仅供已备份且源库存在的安装使用。
fn legacy_call_snapshot_database(state: &MigrationState) -> Result<PathBuf> {
    let backup_dir = state.backup_dir.clone().context(
        "retired call store snapshot requires a completed backup; existing data preserved",
    )?;
    Ok(PathBuf::from(backup_dir)
        .join(LEGACY_CALLS_BACKUP_DIR_NAME)
        .join(LEGACY_CALLS_FILE_NAME))
}

/// The call-store destination that a (possibly interrupted) publish left live.
///
/// A crash inside publication may already have renamed the staged call store into the canonical
/// `calls/` root; in that case the canonical store is the live target. Otherwise it is the staged
/// store. When neither exists there is no call store to verify.
fn call_store_destination(
    paths: &StudioPaths,
    calls_staging: &Path,
) -> impl std::future::Future<Output = Result<Option<PathBuf>>> + Send + 'static {
    let paths = paths.clone();
    let calls_staging = calls_staging.to_path_buf();
    async move {
        let paths = &paths;
        let calls_staging = calls_staging.as_path();
        let canonical = paths.calls_database();
        let staged = calls_staging.join(LEGACY_CALLS_FILE_NAME);
        let canonical_exists = tokio::fs::try_exists(&canonical).await?;
        let staged_exists = tokio::fs::try_exists(&staged).await?;
        // The canonical store can only exist before the rename when something other than this
        // publication created it (a bypass opener, an old binary or an external tool): publication
        // renames the staged root into place, so a resume that finds the canonical store has already lost
        // the staged one. Choosing either side would discard verified call facts, so this fails closed.
        ensure!(
            !(canonical_exists && staged_exists),
            "the canonical call store exists while the verified staged call store is still present \
             ({}); refusing to choose one and discard the other; every original byte is preserved",
            canonical.display()
        );
        Ok(if canonical_exists {
            Some(canonical)
        } else {
            staged_exists.then_some(staged)
        })
    }
}

/// The staged session layout, or the already-published `sessions/` root when a partial publish renamed
/// the staging root before crashing. Both hold identical bytes, so publication re-verifies and continues
/// idempotently instead of failing on a missing staging directory.
fn published_or_staged_sessions_dir(
    paths: &StudioPaths,
    staging: &Path,
) -> impl std::future::Future<Output = Result<PathBuf>> + Send + 'static {
    let paths = paths.clone();
    let staging = staging.to_path_buf();
    async move {
        let paths = &paths;
        let staging = staging.as_path();
        let canonical = paths.sessions_dir();
        let canonical_exists = tokio::fs::try_exists(&canonical).await?;
        let staged_exists = tokio::fs::try_exists(staging).await?;
        // Same rule as the call store: publication renames the staged root into place, so a canonical
        // root that exists while the staged root still does must have been created by something else.
        // Choosing either side would discard verified bytes, so this fails closed.
        ensure!(
            !(canonical_exists && staged_exists),
            "the canonical session layout exists while the verified staged session layout is still \
             present ({}); refusing to choose one and discard the other; every original byte is \
             preserved",
            canonical.display()
        );
        Ok(if canonical_exists {
            canonical
        } else {
            staging.to_path_buf()
        })
    }
}

/// Proves the retired call store is still the exact phase-1 source, resolving each member across the
/// live source and the byte-preserved archive so a publication that crashed after renaming only some
/// members still matches.
///
/// Recomputes the aggregated source identity and the snapshot identity with the bounded, streaming
/// digest and fails closed on a changed/appeared/disappeared source, a member that appears in both
/// locations, a changed snapshot, or a missing archive, always keeping every original byte. Used on
/// every resumed `Verified` path and, again, at the publication boundary.
fn ensure_calls_source_matches_recorded(
    state: &MigrationState,
    paths: &StudioPaths,
    recorded: &str,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let state = state.clone();
    let paths = paths.clone();
    let recorded = recorded.to_owned();
    async move {
        let state = &state;
        let paths = &paths;
        let recorded = recorded.as_str();
        let source = legacy_product_calls_database(paths);
        let archived = archived_legacy_call_store(paths);
        let identity =
            super::storage::calls::legacy_call_store_identity(&source, Some(&archived)).await?;
        ensure!(
            identity.present,
            "retired call store disappeared without a byte-preserved copy ({}); existing data preserved",
            source.display()
        );
        ensure!(
            identity.fingerprint == recorded,
            "retired call store no longer matches the verified source (live or archived); existing \
         data preserved"
        );
        // The phase-1 snapshot binds the audit even after every member has been retired: the archive must
        // still prove the same bytes the snapshot holds.
        let snapshot = legacy_call_snapshot_database(state)?;
        ensure!(
            tokio::fs::try_exists(&snapshot).await?,
            "retired call store snapshot is missing ({}); existing data preserved",
            snapshot.display()
        );
        let snapshot_fingerprint =
            super::storage::calls::legacy_call_store_fingerprint(&snapshot).await?;
        ensure!(
            snapshot_fingerprint == recorded,
            "retired call store snapshot changed after it was verified; existing data preserved"
        );
        Ok(())
    }
}

/// Fails closed unless the retired call store — if the report says one existed — is verified and still
/// bound to the recorded phase-1 identity. Checked at the publication boundary before any rename or
/// archive, so a changed live source or snapshot can never be published or retired as if verified.
fn ensure_calls_source_verified_for_publication(
    state: &MigrationState,
    paths: &StudioPaths,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let state = state.clone();
    let paths = paths.clone();
    async move {
        let state = &state;
        let paths = &paths;
        // Resolve presence across the live source and the byte-preserved archive so a publication that
        // crashed after renaming only some members is still recognized as the same store.
        let archived = archived_legacy_call_store(paths);
        let source_present = super::storage::calls::legacy_call_store_identity(
            &legacy_product_calls_database(paths),
            Some(&archived),
        )
        .await?
        .present;
        let Some(audit) = state.calls.as_ref().filter(|audit| audit.source_present) else {
            ensure!(
                !source_present,
                "retired call store appeared before publication and was never imported; existing data \
             preserved"
            );
            return Ok(());
        };
        let recorded = state.calls_source_fingerprint.as_deref().context(
        "retired call store fingerprint is absent in the migration report; existing data preserved",
    )?;
        ensure!(
            !recorded.is_empty(),
            "retired call store fingerprint is absent in the migration report; existing data preserved"
        );
        ensure!(
            audit.verified
                && !audit.source_fingerprint.is_empty()
                && audit.source_fingerprint == recorded,
            "retired call store audit is not verified; existing data preserved"
        );
        ensure_calls_source_matches_recorded(state, paths, recorded).await
    }
}

/// Re-checks the actual call-store destination at the publication boundary, immediately before any
/// staging rename or archive.
///
/// Phase-4 already revalidated and bound the destination; this is the last, explicit guard so a target
/// whose facts changed (a tampered row, watermark or body) between verification and publication stops
/// here with every original byte preserved. A pure WAL/checkpoint physical rewrite of the same facts
/// does not trip it. A missing destination after it was bound is data loss.
fn ensure_call_destination_unchanged(
    state: &MigrationState,
    paths: &StudioPaths,
    calls_staging: &Path,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let state = state.clone();
    let paths = paths.clone();
    let calls_staging = calls_staging.to_path_buf();
    async move {
        let state = &state;
        let paths = &paths;
        let calls_staging = calls_staging.as_path();
        let Some(recorded) = state.calls_destination_fingerprint.as_deref() else {
            return Ok(());
        };
        let Some(destination) = call_store_destination(paths, calls_staging).await? else {
            bail!(
                "published call store disappeared after it was verified; existing data preserved"
            );
        };
        let current = super::storage::calls::calls_store_fingerprint(&destination).await?;
        ensure!(
            current == recorded,
            "published call store changed before publication ({}); existing data preserved",
            destination.display()
        );
        Ok(())
    }
}

/// Phase 4: re-derive every check from the staged bytes before anything is published.
async fn verify_exported_sessions(
    state: &mut MigrationState,
    state_path: PathBuf,
    staging: PathBuf,
    calls_staging: PathBuf,
    sessions: PathBuf,
    sessions_lease: std::fs::File,
    paths: StudioPaths,
) -> Result<()> {
    let state_path = state_path.as_path();
    let staging = staging.as_path();
    let calls_staging = calls_staging.as_path();
    let sessions = sessions.as_path();
    let sessions_lease = &sessions_lease;
    let paths = &paths;
    // Phase-4 always re-derives the retired-call audit, even on a resumed `Verified` path: the
    // destination call store may have changed after the earlier verification, or a partial publish may
    // already have renamed the staging directory to the canonical one. The audit resolves whichever
    // destination is live, re-checks it against the preserved snapshot, and fails closed on any
    // corruption. This is independent of the shared session journal, so an installation that only
    // ever had the retired product database is checked here too.
    verify_legacy_call_audit(
        state,
        state_path.to_path_buf(),
        paths.clone(),
        calls_staging.to_path_buf(),
    )
    .await?;
    if state.phase.order() >= MigrationPhase::Verified.order() {
        // Every session was already verified by an earlier run and the durable record still proves the
        // staged catalog entries, so the legacy journal is not re-read just to reach publication.
        return Ok(());
    }
    if state.sessions.is_empty() && !tokio::fs::try_exists(sessions).await? {
        // No shared legacy journal was staged: every published fact comes from the retired product
        // directory, so there is no session-scoped material to re-derive.
        state.phase = MigrationPhase::Verified;
        state.updated_at = unix_seconds();
        return report::store(state_path, state).await;
    }
    // 每条会话的模型/工具调用事实都在 staged（或部分发布后已 rename 成 canonical 的）调用库里；
    // 没有这个发布目标就无法逐项核对调用身份、正文与水位，因此这里先解析它。
    let calls_destination = if state.sessions.is_empty() {
        None
    } else {
        Some(call_store_destination(paths, calls_staging).await?.context(
            "staged call store is missing before session verification; existing data preserved",
        )?)
    };
    let legacy = open_legacy_sessions(sessions, sessions_lease).await?;
    // 会话级核对在 phase-4 重跑与导出完全一致的投影，`thread_identity` 的目录事实（标题/角色/
    // 父 Thread/工作区）来自旧产品库，因此这里只读打开同一份源字节。
    let product_db = connect(&paths.database(), DatabaseAccess::ReadOnly).await?;
    let verification = async {
        let calls_destination = calls_destination.as_deref().context(
            "staged call store is missing before session verification; existing data preserved",
        )?;
        // 退役调用库只要被合并进同一个目标库，逐行统计列/终态就可能由那次已审计的对账补齐；此时
        // 会话级核对只要求旧 journal 的身份、正文与"只进不退"的水位/终态仍然成立。
        let merged_retired_calls = state
            .calls
            .as_ref()
            .is_some_and(|audit| audit.source_present);
        for index in 0..state.sessions.len() {
            let already_verified = {
                let record = &state.sessions[index];
                record.status == SessionMigrationStatus::Verified
                    && record.attachment_hashes.len() as u64 == record.attachment_count
            };
            if already_verified {
                continue;
            }
            verify_session(
                &legacy,
                &product_db,
                staging,
                calls_destination,
                merged_retired_calls,
                &state.sessions[index],
            )
            .await?;
            state.sessions[index].status = SessionMigrationStatus::Verified;
            state.updated_at = unix_seconds();
            // A crash must resume after the last verified session instead of re-verifying or
            // re-exporting the whole prefix.
            report::store(state_path, state).await?;
        }
        // 归属：发布会把整个 staging 根 rename 成 `sessions/`，因此根下任何不是本次记录会话的目录都会
        // 未经校验被发布出去。缺失的会话目录早已在上面的循环里失败，这里只拒绝多余的目录。
        verify_staged_session_inventory(paths, staging, state).await?;
        verify_legacy_attachment_coverage(paths, state).await?;
        Ok(())
    }
    .await;
    let shutdown = legacy
        .shutdown()
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()));
    finish_connection(
        product_db,
        combine_conversion_and_shutdown(verification, shutdown),
    )
    .await?;
    state.phase = MigrationPhase::Verified;
    state.updated_at = unix_seconds();
    report::store(state_path, state).await
}

/// Proves the staged session root holds exactly the sessions this migration recorded.
///
/// Publication renames the whole staging root into `sessions/`, so any other directory there would be
/// published without ever being verified. A recorded session whose directory is missing already fails
/// while its staged catalog entry is loaded; this check only rejects unrecorded directories.
fn verify_staged_session_inventory(
    paths: &StudioPaths,
    staging: &Path,
    state: &MigrationState,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let paths = paths.clone();
    let staging = staging.to_path_buf();
    let state = state.clone();
    async move {
        let paths = &paths;
        let staging = staging.as_path();
        let state = &state;
        let expected = state
            .sessions
            .iter()
            .map(|session| session.storage_key.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let root = published_or_staged_sessions_dir(paths, staging).await?;
        let mut entries = match tokio::fs::read_dir(&root).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                bail!(
                    "staged session root holds a directory with a non-UTF-8 name ({}); existing data \
                 preserved",
                    entry.path().display()
                );
            };
            ensure!(
                expected.contains(name),
                "staged session root holds an unrecorded session directory ({}); refusing to publish \
             unverified session data; every original byte is preserved",
                entry.path().display()
            );
        }
        Ok(())
    }
}

/// Verifies that every persistent blob in the retired global attachment root is represented by a
/// verified per-session blob before that root is archived. Draft files are transient and excluded.
fn verify_legacy_attachment_coverage(
    paths: &StudioPaths,
    state: &MigrationState,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let paths = paths.clone();
    let state = state.clone();
    async move {
        let paths = &paths;
        let state = &state;
        let root = paths.legacy_attachments_dir();
        if !tokio::fs::try_exists(&root).await? {
            return Ok(());
        }
        let referenced = state
            .sessions
            .iter()
            .flat_map(|session| session.attachment_hashes.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            let mut entries = match tokio::fs::read_dir(&directory).await {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let file_type = entry.file_type().await?;
                if file_type.is_dir() {
                    if entry.file_name().to_str() == Some("drafts") {
                        continue;
                    }
                    pending.push(path);
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("legacy attachment name is not UTF-8"))?;
                ensure!(
                    referenced.contains(&name),
                    "legacy attachment {} has no verified per-session blob; existing data preserved",
                    path.display()
                );
            }
        }
        Ok(())
    }
}

/// Phase 5: publish the staged layout atomically, then archive the retired legacy sources.
///
/// The upgraded product database is *not* a retired source: it stays in place as the live SQLite
/// auxiliary/object store, so publication only renames the staged session/call roots and retires the
/// shared session journal, the retired per-product call database and fully converted blobs.
async fn publish(
    state: &mut MigrationState,
    state_path: PathBuf,
    paths: StudioPaths,
    sessions: PathBuf,
    product: PathBuf,
    staging: PathBuf,
    calls_staging: PathBuf,
) -> Result<()> {
    let state_path = state_path.as_path();
    let paths = &paths;
    let sessions = sessions.as_path();
    let product = product.as_path();
    let staging = staging.as_path();
    let calls_staging = calls_staging.as_path();
    if state.phase.order() >= MigrationPhase::Published.order() {
        // A report that already records the publication is the only proof that the switch finished;
        // commit any announcement a crash left behind before the removal step.
        commit_layout_publication(paths).await?;
        return Ok(());
    }
    // Publication is only legal once every recorded session verified and the retired call store (when
    // the report says one existed) was imported and verified: a resumed run must never rename or
    // archive facts that phase 4 has not proven.
    ensure!(
        state.phase.order() >= MigrationPhase::Verified.order(),
        "layout publication requires a verified migration phase; existing data preserved"
    );
    ensure!(
        state
            .sessions
            .iter()
            .all(|session| session.status == SessionMigrationStatus::Verified),
        "layout publication requires every staged session to be verified; existing data preserved"
    );
    ensure!(
        !state
            .calls
            .as_ref()
            .is_some_and(|calls| calls.source_present && !calls.verified),
        "layout publication refused an unverified retired call store import; existing data preserved"
    );
    // Before any irreversible staging rename or archive, re-prove that the retired call store (if the
    // report says one existed) is verified and still matches its recorded phase-1 identity. A resumed
    // `Verified` path that finds a changed, appeared or disappeared source, or a changed snapshot, stops
    // here with every original byte preserved instead of publishing or retiring foreign bytes.
    ensure_calls_source_verified_for_publication(state, paths).await?;
    // And re-check the actual destination call store right here, before any staging rename or archive.
    ensure_call_destination_unchanged(state, paths, calls_staging).await?;
    // Rebuild the canonical directory/settings documents from the staged per-session entries and the
    // retired product directory, and publish them before the layout rename. `merge` is idempotent,
    // so a resumed publish neither duplicates nor loses entries, and the original bytes are
    // untouched until every session has verified.
    let legacy = if tokio::fs::try_exists(product).await? {
        let product_db = connect(product, DatabaseAccess::ReadOnly).await?;
        let facts = read_legacy_directory_facts(&product_db).await;
        finish_connection(product_db, facts).await?
    } else {
        LegacyDirectoryFacts::default()
    };
    // Read the staged per-session entries from the staging root, or from the already-published
    // `sessions/` root when a partial publish renamed the staging root before crashing. Both hold
    // identical bytes, so publication can re-verify and continue idempotently instead of failing on a
    // missing staging directory.
    let session_root = published_or_staged_sessions_dir(paths, staging).await?;
    let staged_entries = staged_catalog_entries(state, &session_root)?;
    // The announcement is durable before the first canonical document or layout root changes, so an
    // interrupted switch always leaves evidence that this home was mid-publication.
    write_layout_publication_marker(paths, state).await?;
    let published = publish_canonical_documents(paths, legacy, staged_entries).await?;
    state.catalog_entry_count = published.catalog_entries;
    state.catalog_revision = Some(published.catalog_revision);
    state.workspace_entry_count = published.workspace_entries;
    state.settings_entry_count = published.settings_entries;
    state.updated_at = unix_seconds();
    // Each rename is atomic, and an already-renamed target simply means a previous publish attempt
    // reached this point before crashing, so re-running is idempotent. A target that exists *while*
    // its staged source still does was created by someone else (a bypass opener, an old binary or an
    // external tool); publishing over it, or skipping the verified source in its favour, would drop
    // verified facts, so the switch refuses instead of guessing.
    for (source, target, label) in [
        (staging, paths.sessions_dir(), "session layout"),
        (calls_staging, paths.calls_dir(), "call store layout"),
    ] {
        let staged_exists = tokio::fs::try_exists(source).await?;
        let canonical_exists = tokio::fs::try_exists(&target).await?;
        match layout_root_action(staged_exists, canonical_exists).with_context(|| {
            format!(
                "refusing to switch the migrated {label} into {}",
                target.display()
            )
        })? {
            LayoutRootAction::Rename => {
                tokio::fs::rename(source, &target).await.with_context(|| {
                    format!(
                        "failed to publish the migrated layout into {}",
                        target.display()
                    )
                })?;
            }
            LayoutRootAction::AlreadyPublished => {}
        }
    }
    sync_directory(paths.home()).await?;
    // The live product database is upgraded in place and stays where it is: it remains the SQLite
    // auxiliary/object store (`studio_objects`, Project foreign keys and the write-behind mirrors).
    // Only retired sources are archived, and the retired global attachment root is archived only
    // after a legacy session conversion actually verified its blobs.
    let sessions_converted = !state.sessions.is_empty() || tokio::fs::try_exists(sessions).await?;
    // The retired per-product call database is archived only after its facts were imported and verified
    // (checked at the top of this phase). Its blobs are retired together with its verified source bytes.
    let legacy_calls = legacy_product_calls_database(paths);
    let calls_retired = state
        .calls
        .as_ref()
        .is_some_and(|calls| calls.source_present && calls.verified);
    archive_legacy_sources(
        sessions,
        product,
        &legacy_calls,
        &paths.calls_database(),
        &paths.migrations_dir(),
        if sessions_converted {
            Some(paths.legacy_attachments_dir())
        } else {
            None
        },
        if calls_retired {
            Some(super::storage::calls::legacy_call_store_blobs_dir(
                &legacy_calls,
            ))
        } else {
            None
        },
    )
    .await?;
    state.phase = MigrationPhase::Published;
    state.error = None;
    state.updated_at = unix_seconds();
    report::store(state_path, state).await?;
    // Only now is the switch complete: the report is durable at `Published` (written after the layout
    // roots were renamed and the retired sources archived), so the announcement can be retired. That
    // unlink is the single durable commit point for every canonical reader; a crash before it leaves
    // the announcement in place and the next run continues idempotently instead of exposing a mixed
    // layout to a normal opener.
    commit_layout_publication(paths).await
}

/// 迁移期读取到的旧产品目录事实；只在持独占锁的 migration 边界读取。
#[derive(Debug, Default)]
struct LegacyDirectoryFacts {
    projects: Vec<WorkspaceEntry>,
    settings: Vec<SettingEntry>,
    threads: Vec<ThreadRecord>,
}

/// 已发布 canonical 文档的规模与 revision；用于写入可恢复的迁移进度。
#[derive(Debug, Clone, Copy)]
struct PublishedCanonicalDocuments {
    workspace_entries: u64,
    settings_entries: u64,
    catalog_entries: u64,
    catalog_revision: u64,
}

/// 读取旧产品库的 Project、设置与轻量 Thread 目录行。
fn read_legacy_directory_facts(
    db: &DatabaseConnection,
) -> impl std::future::Future<Output = Result<LegacyDirectoryFacts>> + Send + 'static {
    let db = db.clone();
    async move {
        let db = &db;
        let projects = entities::project::Entity::find()
            .order_by_asc(entities::project::Column::CreatedAt)
            .order_by_asc(entities::project::Column::Id)
            .all(db)
            .await?
            .into_iter()
            .map(|row| WorkspaceEntry {
                id: row.id,
                name: row.name,
                path: row.path,
                ssh_alias: row.ssh_alias,
                created_at: row.created_at,
                updated_at: row.updated_at,
                last_opened_at: row.last_opened_at,
                closed: row.closed != 0,
            })
            .collect();
        let settings = entities::app_setting::Entity::find()
            .order_by_asc(entities::app_setting::Column::Key)
            .all(db)
            .await?
            .into_iter()
            .map(|row| SettingEntry {
                key: row.key,
                value: row.value,
                updated_at: row.updated_at,
            })
            .collect();
        let threads = entities::thread::Entity::find()
            .order_by_asc(entities::thread::Column::CreatedAt)
            .order_by_asc(entities::thread::Column::Id)
            .all(db)
            .await?
            .into_iter()
            .map(thread_record)
            .collect::<Result<Vec<_>>>()?;
        Ok(LegacyDirectoryFacts {
            projects,
            settings,
            threads,
        })
    }
}

/// 把三份 canonical 文档写成"旧产品库 + staged 会话事实"的完整转换结果。
///
/// 转换必须保持 ID、Project 关联、关闭/归档状态、时间戳、设置值与凭据引用，并逐项核对已发布的
/// 新 TOML 与迁移源一致；任一项丢失或与源不一致都失败并保留原数据。每个文档先合并、再在缺失时
/// 落盘（[`WorkspaceStore::persist_if_absent`] 等），因此任何已有安装都拥有完整的三份文档，而
/// 普通启动只需要读取它们。
fn publish_canonical_documents(
    paths: &StudioPaths,
    legacy: LegacyDirectoryFacts,
    staged_entries: Vec<CatalogEntry>,
) -> impl std::future::Future<Output = Result<PublishedCanonicalDocuments>> + Send + 'static {
    let paths = paths.clone();
    async move {
        let paths = &paths;
        // Recoverability is checked before any canonical file is written: a retired directory row whose
        // Thread cannot be replayed from a staged journal must never become a published Thread that the
        // runtime cannot open.
        let expected_catalog =
            merge_catalog_entries(staged_entries, legacy_thread_entries(&legacy.threads)?)?;
        let workspaces = WorkspaceStore::load(paths.workspaces_file()).await?;
        let settings = SettingsStore::load(paths.settings_file()).await?;
        let catalog = CatalogStore::load(paths.catalog_file()).await?;

        // 已在 canonical 文档中的条目优先：设置值允许被用户改新，只为迁移新增条目核对取值。
        let existing_workspaces = workspaces
            .entries()
            .into_iter()
            .map(|entry| entry.id)
            .collect::<std::collections::BTreeSet<_>>();
        let existing_settings = settings
            .entries()
            .into_iter()
            .map(|entry| entry.key)
            .collect::<std::collections::BTreeSet<_>>();

        // Project/Workspace：身份、状态与关闭标记。
        let expected_workspaces = legacy.projects.clone();
        workspaces.merge(expected_workspaces.clone()).await?;
        let published_workspaces = workspaces.entries();
        for expected in &expected_workspaces {
            let published = published_workspaces
                .iter()
                .find(|entry| entry.id == expected.id)
                .with_context(|| {
                    format!(
                        "published workspaces.toml lost migrated Project {}",
                        expected.id
                    )
                })?;
            ensure!(
                published.path == expected.path && published.ssh_alias == expected.ssh_alias,
                "published workspaces.toml disagrees with the migrated Project {} directory identity",
                expected.id
            );
            if !existing_workspaces.contains(&expected.id) {
                ensure!(
                    published == expected,
                    "published workspaces.toml disagrees with migrated Project {}",
                    expected.id
                );
            }
        }
        workspaces.persist_if_absent().await?;

        // 用户设置：Provider/模型/UI 值与凭据引用逐条保留。
        let expected_settings = legacy.settings.clone();
        settings.merge(expected_settings.clone()).await?;
        for expected in &expected_settings {
            let value = settings.get(&expected.key).with_context(|| {
                format!(
                    "published settings.toml lost migrated setting {}",
                    expected.key
                )
            })?;
            if !existing_settings.contains(&expected.key) {
                ensure!(
                    value == expected.value,
                    "published settings.toml disagrees with the migrated setting {}",
                    expected.key
                );
            }
        }
        settings.persist_if_absent().await?;

        // 轻量 Thread 目录：staged 摘要提供 journal 重放后的执行事实，目录行提供归档与创建时间。
        let expected_count = expected_catalog.len();
        let verification = expected_catalog.clone();
        catalog.merge(expected_catalog).await?;
        let published_catalog = catalog.entries();
        for entry in &verification {
            ensure!(
                published_catalog.iter().any(|published| published == entry),
                "published catalog lost migrated session {}",
                entry.id
            );
        }
        ensure!(
            published_catalog.len() >= expected_count,
            "published catalog lost migrated sessions"
        );
        for entry in &published_catalog {
            if entry.project_id.is_empty() {
                continue;
            }
            ensure!(
                published_workspaces
                    .iter()
                    .any(|project| project.id == entry.project_id),
                "migrated catalog entry {} references unknown Project {}; existing data preserved",
                entry.id,
                entry.project_id
            );
        }
        catalog.persist_if_absent().await?;

        Ok(PublishedCanonicalDocuments {
            workspace_entries: u64::try_from(published_workspaces.len())
                .context("published workspaces exceed u64")?,
            settings_entries: u64::try_from(settings.entries().len())
                .context("published settings exceed u64")?,
            catalog_entries: u64::try_from(published_catalog.len())
                .context("published catalog size exceeds u64")?,
            catalog_revision: catalog.revision(),
        })
    }
}

/// 读取已通过校验的 staged 每会话目录摘要。
fn staged_catalog_entries(state: &MigrationState, staging: &Path) -> Result<Vec<CatalogEntry>> {
    let mut entries = Vec::with_capacity(state.sessions.len());
    for record in &state.sessions {
        ensure!(
            record.status == SessionMigrationStatus::Verified,
            "catalog cannot be published before every session is verified"
        );
        entries.push(load_staged_catalog(
            &staged_session_dir(staging, &record.storage_key),
            &record.thread_id,
        )?);
    }
    Ok(entries)
}

/// 旧目录行 → catalog 条目，并校验它能被 canonical `catalog.toml` 重新读回。
fn legacy_thread_entries(threads: &[ThreadRecord]) -> Result<Vec<CatalogEntry>> {
    let mut entries = Vec::with_capacity(threads.len());
    let mut seen = std::collections::BTreeSet::new();
    for record in threads {
        let entry = CatalogEntry::from_thread(&pl_protocol::Thread::from(record.clone()));
        ensure!(
            seen.insert(entry.id.clone()),
            "legacy Thread directory contains duplicate identity {}",
            entry.id
        );
        ensure!(
            entry.agent_path == entry.id,
            "legacy Thread {} directory identity is inconsistent",
            entry.id
        );
        ensure!(
            entry.parent_thread_id.is_some() || entry.root_thread_id == entry.id,
            "legacy root Thread {} is not its own root",
            entry.id
        );
        ensure!(
            entry.workspace_mode != pl_protocol::ThreadWorkspaceMode::Worktree
                || !entry.workspace_path.is_empty(),
            "legacy worktree Thread {} has no workspace address",
            entry.id
        );
        entries.push(entry);
    }
    Ok(entries)
}

/// 合并 staged（journal 重放）摘要与旧目录行。
///
/// 每个旧目录行都必须有 staged 结果：目录行本身不带可恢复的会话状态，把它发布成只有目录的
/// Thread 会得到运行时打不开的条目。缺失即失败并保留原字节。
fn merge_catalog_entries(
    staged: Vec<CatalogEntry>,
    directory_rows: Vec<CatalogEntry>,
) -> Result<Vec<CatalogEntry>> {
    let mut staged_by_id = staged
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    let unbacked = directory_rows
        .iter()
        .filter(|row| !staged_by_id.contains_key(&row.id))
        .map(|row| row.id.clone())
        .collect::<Vec<_>>();
    if !unbacked.is_empty() {
        const MAX_LISTED: usize = 8;
        let listed = unbacked
            .iter()
            .take(MAX_LISTED)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let remaining = unbacked.len().saturating_sub(MAX_LISTED);
        bail!(
            "retired product database still has {} Thread directory row(s) without a recoverable \
             session journal/checkpoint: {}{}; publishing them would create Threads that cannot be \
             opened, so nothing was converted and every original byte is preserved. Restore the \
             retired session journal/checkpoint (or remove those rows explicitly), then retry.",
            unbacked.len(),
            listed,
            if remaining > 0 {
                format!(" (+{remaining} more)")
            } else {
                String::new()
            }
        );
    }
    let mut merged = Vec::with_capacity(staged_by_id.len().max(directory_rows.len()));
    for row in directory_rows {
        match staged_by_id.remove(&row.id) {
            Some(entry) => merged.push(overlay_directory_row(entry, &row)?),
            // Unreachable: every directory row was checked for a staged journal result above.
            None => bail!(
                "internal invariant: Thread {} directory row has no staged journal result",
                row.id
            ),
        }
    }
    merged.extend(staged_by_id.into_values());
    Ok(merged)
}

/// 目录行持有目录事实（身份、标题、角色、归档、创建时间）；staged 条目持有 journal 重放后的
/// 执行事实（Mode、状态、活动时间）。迁移源的对应关系按"staged 从同一目录行读到的字段"核对。
fn overlay_directory_row(entry: CatalogEntry, row: &CatalogEntry) -> Result<CatalogEntry> {
    ensure!(
        entry.id == row.id
            && entry.project_id == row.project_id
            && entry.workspace_mode == row.workspace_mode
            && entry.workspace_path == row.workspace_path
            && entry.parent_thread_id == row.parent_thread_id,
        "staged catalog entry for {} disagrees with its legacy directory row; existing data preserved",
        entry.id
    );
    Ok(CatalogEntry {
        id: row.id.clone(),
        project_id: row.project_id.clone(),
        title: row.title.clone(),
        mode: entry.mode,
        workspace_mode: row.workspace_mode,
        workspace_path: row.workspace_path.clone(),
        parent_thread_id: row.parent_thread_id.clone(),
        root_thread_id: row.root_thread_id.clone(),
        role: row.role.clone(),
        agent_path: row.agent_path.clone(),
        status: entry.status,
        archived: row.archived || entry.archived,
        created_at: row.created_at,
        updated_at: entry.updated_at.max(row.updated_at),
    })
}

/// Moves retired legacy files into a recoverable archive; no user data is deleted, and the live
/// product database is never among the members.
///
/// Each member is renamed individually, so a crash or filesystem error may leave some members already
/// archived while others are still in place; a resumed run simply archives whatever remains. A member
/// that exists in **both** the live location and the archive is ambiguous (for example a retired call
/// database that an old writer recreated) and fails closed rather than silently dropping one copy.
///
/// The per-database lock file is deliberately **not** a member: it is operational metadata that every
/// run recreates while it holds the lock, so archiving it would either move a lock this run still owns
/// (breaking exclusivity) or leave a live lock facing an archived copy on the next retry and stall
/// forever on a false conflict. Leaving it in place preserves correct lock ownership/lifetime and never
/// discards user data.
fn archive_legacy_sources(
    sessions: &Path,
    product: &Path,
    legacy_calls: &Path,
    canonical_calls: &Path,
    migrations_dir: &Path,
    legacy_attachments: Option<PathBuf>,
    legacy_calls_blobs: Option<PathBuf>,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let sessions = sessions.to_path_buf();
    let product = product.to_path_buf();
    let legacy_calls = legacy_calls.to_path_buf();
    let canonical_calls = canonical_calls.to_path_buf();
    let migrations_dir = migrations_dir.to_path_buf();
    async move {
        let sessions = sessions.as_path();
        let product = product.as_path();
        let legacy_calls = legacy_calls.as_path();
        let canonical_calls = canonical_calls.as_path();
        let migrations_dir = migrations_dir.as_path();
        let archive = migrations_dir.join(ARCHIVE_DIR_NAME);
        tokio::fs::create_dir_all(&archive).await?;
        // The upgraded product database is never a member: it stays in place as the live SQLite
        // auxiliary/object store, and normal startup keeps reading and writing its tables.
        let mut members = vec![sessions.to_path_buf()];
        // Only a legacy call database is retired; the canonical global `calls/calls.sqlite` stays put.
        // The legacy file next to the retired product database is archived only after its facts have been
        // imported and verified into the staged call store, together with its content-addressed blobs.
        if legacy_calls != canonical_calls {
            members.push(legacy_calls.to_path_buf());
            // The retired store's WAL/SHM sidecars are part of the same byte-preserved artifact and are
            // archived with it (they are skipped when absent).
            members.push(sidecar(legacy_calls, "-wal"));
            members.push(sidecar(legacy_calls, "-shm"));
        }
        if let Some(legacy_calls_blobs) = legacy_calls_blobs {
            members.push(legacy_calls_blobs);
        }
        if let Some(legacy_attachments) = legacy_attachments {
            members.push(legacy_attachments);
        }
        ensure!(
            members.iter().all(|member| member.as_path() != product),
            "refusing to archive the live product database; existing data preserved"
        );
        for member in members {
            let Some(name) = member.file_name() else {
                continue;
            };
            // A member that is already gone was retired by an earlier, interrupted archive attempt; a
            // resumed run continues instead of failing.
            if !tokio::fs::try_exists(&member).await? {
                continue;
            }
            let destination = archive.join(name);
            ensure!(
                !tokio::fs::try_exists(&destination).await?,
                "refusing to archive {}: a file with the same name already exists in the archive; \
             existing data preserved",
                member.display()
            );
            tokio::fs::rename(&member, &destination)
                .await
                .with_context(|| {
                    format!("failed to archive retired legacy file {}", member.display())
                })?;
        }
        sync_directory(&archive).await
    }
}

/// Retired layout artifacts whose presence identifies an unmigrated installation.
///
/// The product database is deliberately excluded: every installation materializes it, so it can
/// never distinguish a converted home from an unconverted one.
fn legacy_layout_sources(
    paths: &StudioPaths,
) -> impl std::future::Future<Output = Result<bool>> + Send + 'static {
    let paths = paths.clone();
    async move {
        let paths = &paths;
        for candidate in [
            legacy_product_calls_database(paths),
            paths.legacy_attachments_dir(),
        ] {
            if tokio::fs::try_exists(&candidate).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Current-layout artifacts that prove a home already ran the current layout (or started the one-time
/// conversion and has not been rolled back).
///
/// They are checked on normal startup so a residual retired artifact — a leftover per-product
/// `calls.sqlite`, for example — cannot cause a conversion or a rebuild of a missing canonical
/// document on a home that already owns a published layout, call store or draft root. The product
/// database is deliberately excluded because every installation materializes it.
fn current_layout_evidence(
    paths: &StudioPaths,
) -> impl std::future::Future<Output = Result<bool>> + Send + 'static {
    let paths = paths.clone();
    async move {
        let paths = &paths;
        for candidate in [
            paths.sessions_dir(),
            paths.calls_database(),
            paths.attachment_drafts_dir(),
        ] {
            if tokio::fs::try_exists(&candidate).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Pre-refactor per-product call database that lived next to the product database.
fn legacy_product_calls_database(paths: &StudioPaths) -> PathBuf {
    paths.database().with_file_name(LEGACY_CALLS_FILE_NAME)
}

/// Byte-preserved archive location of the retired call store once publication retired it.
fn archived_legacy_call_store(paths: &StudioPaths) -> PathBuf {
    paths
        .migrations_dir()
        .join(ARCHIVE_DIR_NAME)
        .join(LEGACY_CALLS_FILE_NAME)
}

/// Fails closed when an already-published installation lost a canonical fact source.
///
/// The coordinator never rebuilds these documents on a normal startup: doing so would read the
/// retired SQLite tables or scan every session directory, and would silently mask the loss. Every
/// original byte is preserved so an operator can restore or explicitly convert the data.
fn require_canonical_documents(
    paths: &StudioPaths,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let paths = paths.clone();
    async move {
        let paths = &paths;
        let mut missing = Vec::new();
        for (path, name) in [
            (paths.catalog_file(), "catalog.toml"),
            (paths.settings_file(), "settings.toml"),
            (paths.workspaces_file(), "workspaces.toml"),
        ] {
            if !tokio::fs::try_exists(&path).await? {
                missing.push(format!("{name} ({})", path.display()));
            }
        }
        ensure!(
            missing.is_empty(),
            "Studio canonical documents are missing on an installation that already owns state: {}. A \
         normal startup never rebuilds them from the retired product database or by scanning \
         sessions, so startup stops with every existing byte preserved. Restore the listed \
         document(s) from backup; if this installation predates `catalog.toml` and never ran the \
         current layout, convert its retired `projects`/`app_settings`/`threads` rows explicitly \
         with `pl-studio-server --studio-home <home> migrate-legacy-storage --confirm <home>` \
         instead of removing data.",
            missing.join(", ")
        );
        Ok(())
    }
}

/// Validates an operator-asserted one-time conversion before anything is mutated.
///
/// The explicit command is the only way to convert a current-schema home whose canonical documents
/// were never published, because no on-disk fact can separate that case from a current installation
/// that lost its documents. The assertion is therefore checked strictly: a home that already ran the
/// current layout, or already published (or even partially published) the canonical documents, is
/// refused instead of being overwritten.
fn validate_explicit_conversion(
    paths: &StudioPaths,
    recorded: Option<&MigrationState>,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let paths = paths.clone();
    let recorded = recorded.cloned();
    async move {
        let paths = &paths;
        let recorded = recorded.as_ref();
        ensure!(
            tokio::fs::try_exists(&paths.database()).await?,
            "explicit legacy conversion refused: no original product database at {}; nothing was changed",
            paths.database().display()
        );
        ensure!(
            recorded.is_none_or(|state| state.phase != MigrationPhase::Published),
            "explicit legacy conversion refused: this home already published the current layout (report \
         {}); nothing was changed",
            migration_state_path(paths).display()
        );

        let mut current_layout = Vec::new();
        for (path, label) in [
            (paths.sessions_dir(), "published session layout"),
            (paths.calls_database(), "current call store"),
            (paths.attachment_drafts_dir(), "attachment draft root"),
        ] {
            if tokio::fs::try_exists(&path).await? {
                current_layout.push(format!("{label} ({})", path.display()));
            }
        }
        ensure!(
            current_layout.is_empty(),
            "explicit legacy conversion refused: this home already ran the current layout ({}) and \
         converting it would rewrite live facts; nothing was changed. An unfinished migration is \
         completed by the next normal startup.",
            current_layout.join(", ")
        );

        // A durable, unfinished migration is resumed rather than refused: its canonical documents may
        // already be partially written by the previous attempt, and re-running the idempotent merge is
        // the only way to finish it. Without one, any existing canonical document means the fact source
        // is current (or partially published) and must never be overwritten.
        if recorded.is_some_and(|state| state.phase != MigrationPhase::Published) {
            return Ok(());
        }
        let mut existing_documents = Vec::new();
        for (path, name) in [
            (paths.catalog_file(), "catalog.toml"),
            (paths.settings_file(), "settings.toml"),
            (paths.workspaces_file(), "workspaces.toml"),
        ] {
            if tokio::fs::try_exists(&path).await? {
                existing_documents.push(format!("{name} ({})", path.display()));
            }
        }
        ensure!(
            existing_documents.is_empty(),
            "explicit legacy conversion refused: canonical documents already exist ({}); refusing to \
         overwrite a partially published or current fact source, and nothing was changed",
            existing_documents.join(", ")
        );
        Ok(())
    }
}

fn migration_state_path(paths: &StudioPaths) -> PathBuf {
    paths
        .migrations_dir()
        .join(report::MIGRATION_STATE_FILE_NAME)
}

/// 布局切换公告的位置；store 与迁移协调器共用同一个判定入口。
pub(in crate::studio) fn layout_publication_marker_path(paths: &StudioPaths) -> PathBuf {
    paths
        .migrations_dir()
        .join(LAYOUT_PUBLICATION_MARKER_FILE_NAME)
}

/// Durable announcement of an in-progress layout publication.
///
/// It only has to be readable and auditable, so it records the resolved home, the phase the switch
/// started from and how many sessions were staged. The migration report stays the phase authority.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LayoutPublicationMarker {
    home: String,
    started_at: i64,
    phase: &'static str,
    session_count: usize,
}

/// Announces the layout switch before any canonical document or layout root is touched.
///
/// The announcement is written first and removed only after the report records `Published`, so a
/// crash anywhere inside the switch leaves durable evidence that a home is mid-switch. An existing
/// announcement is not rewritten: publication is idempotent and the original boundary is the
/// auditable fact.
fn write_layout_publication_marker(
    paths: &StudioPaths,
    state: &MigrationState,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let paths = paths.clone();
    let state = state.clone();
    async move {
        let paths = &paths;
        let state = &state;
        let path = layout_publication_marker_path(paths);
        if tokio::fs::try_exists(&path).await? {
            return Ok(());
        }
        let marker = LayoutPublicationMarker {
            home: paths.home().to_string_lossy().into_owned(),
            started_at: unix_seconds(),
            phase: state.phase.label(),
            session_count: state.sessions.len(),
        };
        let bytes = serde_json::to_vec_pretty(&marker)?;
        let directory = path
            .parent()
            .context("layout publication marker has no parent directory")?
            .to_path_buf();
        tokio::fs::create_dir_all(&directory).await?;
        tokio::task::spawn_blocking(move || {
            pl_tool::workspace::write_file_atomically(&path, &bytes)
        })
        .await??;
        Ok(())
    }
}

fn remove_layout_publication_marker(
    paths: &StudioPaths,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let paths = paths.clone();
    async move {
        let paths = &paths;
        let path = layout_publication_marker_path(paths);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Whether a layout switch is announced but not yet committed.
///
/// This is the single publication predicate shared by the migration coordinator and every canonical
/// reader: the announcement is written before the first canonical mutation and retired only once
/// every verified document, session root, call root and reference is in place and the durable report
/// records `Published`.
pub(in crate::studio) fn layout_publication_pending(
    paths: &StudioPaths,
) -> impl std::future::Future<Output = Result<bool>> + Send + 'static {
    let paths = paths.clone();
    async move {
        let paths = &paths;
        Ok(tokio::fs::try_exists(&layout_publication_marker_path(paths)).await?)
    }
}

/// Retires the announcement and fsyncs its directory: this unlink is the durable commit point of the
/// whole layout switch.
fn commit_layout_publication(
    paths: &StudioPaths,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let paths = paths.clone();
    async move {
        let paths = &paths;
        remove_layout_publication_marker(paths).await?;
        let migrations = paths.migrations_dir();
        if tokio::fs::try_exists(&migrations).await? {
            sync_directory(&migrations).await?;
        }
        Ok(())
    }
}

/// The authoritative gate for every canonical-layout reader and creator.
///
/// Layout publication switches several roots (`catalog.toml`, `settings.toml`, `workspaces.toml`,
/// `sessions/`, `calls/`) and is committed by a single durable step (retiring the announcement after
/// the report says `Published`). While that announcement exists the canonical layout is *not*
/// committed: no reader may classify it, read it, or create a canonical root of its own. Creating one
/// (for example an empty `calls/calls.sqlite`) would make publication skip the verified staged store
/// and drop call facts, so this gate refuses before any file is created.
///
/// Fresh homes and already-current installations have no announcement and are never blocked. A stale
/// announcement (report already `Published`) is retired by the next normal startup, which holds the
/// exclusive runtime lock and resumes the phase machine.
pub(in crate::studio) fn ensure_layout_committed(
    paths: &StudioPaths,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let paths = paths.clone();
    async move {
        let paths = &paths;
        if layout_publication_pending(paths).await? {
            bail!(
                "Studio layout publication is not committed yet ({}); refusing to open, classify or \
             create the canonical layout. Finish the running migration (a normal startup resumes it \
             under the exclusive runtime lock) before using another entry point; every original byte \
             is preserved.",
                layout_publication_marker_path(paths).display()
            );
        }
        Ok(())
    }
}

/// What publication may do with one staged layout root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutRootAction {
    /// The canonical root is absent: rename the verified staged root into place.
    Rename,
    /// The canonical root exists and the staged root is gone: a previous attempt renamed it already.
    AlreadyPublished,
}

/// Decides whether one staged layout root can be switched into its canonical location.
///
/// * staged only → rename (normal, and the mid-publication resume case for the root not yet renamed);
/// * canonical only → already published by a previous attempt of this same publication;
/// * both → ambiguous: publication renames the staged root away, so a canonical root that exists
///   while the staged one still does came from somewhere else (bypass opener, old binary, external
///   tool). Publishing over it or skipping the verified staged root would discard verified facts.
/// * neither → the verified staged artifact disappeared before it was published (data loss).
fn layout_root_action(staged_exists: bool, canonical_exists: bool) -> Result<LayoutRootAction> {
    match (staged_exists, canonical_exists) {
        (true, false) => Ok(LayoutRootAction::Rename),
        (false, true) => Ok(LayoutRootAction::AlreadyPublished),
        (true, true) => bail!(
            "the canonical layout root already exists while the verified staged root is still \
             present; refusing to choose one and discard the other, and every original byte is \
             preserved. Remove or rename the foreign canonical root, or restore the migration report \
             that produced it, then retry"
        ),
        (false, false) => bail!(
            "the verified staged layout root disappeared before publication; existing data is \
             preserved and publication stops instead of marking an incomplete layout as complete"
        ),
    }
}

fn summarize_outcome(
    home: PathBuf,
    report_path: PathBuf,
    recorded: Option<MigrationState>,
    error: Option<anyhow::Error>,
) -> LegacyMigrationOutcome {
    let staged_sessions = recorded.as_ref().map_or(0, |state| state.sessions.len());
    let verified_sessions = recorded.as_ref().map_or(0, |state| {
        state
            .sessions
            .iter()
            .filter(|record| record.status == SessionMigrationStatus::Verified)
            .count()
    });
    let (phase, backup_dir, workspace_entries, settings_entries, catalog_entries, catalog_revision) =
        match recorded.as_ref() {
            Some(state) => (
                Some(state.phase.label()),
                state.backup_dir.clone(),
                state.workspace_entry_count,
                state.settings_entry_count,
                state.catalog_entry_count,
                state.catalog_revision,
            ),
            None => (None, None, 0, 0, 0, None),
        };
    let calls = recorded
        .as_ref()
        .and_then(|state| state.calls.clone())
        .unwrap_or_default();
    LegacyMigrationOutcome {
        home,
        report_path,
        phase,
        backup_dir,
        workspace_entries,
        settings_entries,
        catalog_entries,
        catalog_revision,
        calls_source_present: calls.source_present,
        calls_source_schema_version: calls.source_schema_version,
        calls_source_database_id: calls.source_database_id.clone(),
        calls_source_model_calls: calls.source_model_calls,
        calls_source_tool_calls: calls.source_tool_calls,
        calls_source_watermarks: calls.source_watermarks,
        calls_source_bodies: calls.source_bodies,
        calls_verified_bodies: calls.verified_bodies,
        calls_destination_model_calls: calls.destination_model_calls,
        calls_destination_tool_calls: calls.destination_tool_calls,
        calls_verified: calls.verified,
        staged_sessions,
        verified_sessions,
        // The captured error is the failure of this run; the recorded one is the durable copy.
        error: error
            .map(|error| format!("{error:#}"))
            .or_else(|| recorded.and_then(|state| state.error)),
    }
}

fn thread_updated_at(
    product: &DatabaseConnection,
    thread_id: &str,
) -> impl std::future::Future<Output = Result<i64>> + Send + 'static {
    let product = product.clone();
    let thread_id = thread_id.to_owned();
    async move {
        let row = product
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT updated_at FROM threads WHERE id=?",
                [Value::String(Some(thread_id))],
            ))
            .await?;
        match row {
            Some(row) => Ok(row.try_get::<i64>("", "updated_at")?),
            None => Ok(unix_seconds()),
        }
    }
}

fn open_legacy_sessions(
    path: &Path,
    lease: &std::fs::File,
) -> impl std::future::Future<Output = Result<pl_core::persistence::SqliteSessionStore>> + Send + 'static
{
    // Adopt the lease this run already owns: cloning the handle shares the same lock, so the reader
    // opens the retired repository without contending with the coordinator that holds it.
    let path = path.to_path_buf();
    let adopted = lease.try_clone();
    async move {
        let adopted = adopted.context("failed to adopt the session database lease")?;
        pl_core::persistence::SqliteSessionStore::open_with_lock(
            pl_core::persistence::SqliteSessionOptions { path },
            adopted,
        )
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))
    }
}

/// Owns its input before building the future: the returned future never captures a borrowed
/// parameter, so a spawned task can prove it `Send` for every reference lifetime.
fn remove_dir_if_present(
    path: &Path,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let path = path.to_path_buf();
    async move {
        match tokio::fs::remove_dir_all(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn fingerprint(
    path: &Path,
) -> impl std::future::Future<Output = Result<SourceFingerprint>> + Send + 'static {
    let path = path.to_path_buf();
    async move {
        let path = path.as_path();
        let metadata = match tokio::fs::metadata(path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SourceFingerprint {
                    path: path.to_string_lossy().into_owned(),
                    exists: false,
                    len: 0,
                    modified_unix_ms: 0,
                    schema_version: None,
                });
            }
            Err(error) => return Err(error.into()),
        };
        let modified_unix_ms = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
            .unwrap_or(0);
        Ok(SourceFingerprint {
            path: path.to_string_lossy().into_owned(),
            exists: true,
            len: metadata.len(),
            modified_unix_ms,
            schema_version: inspect(path).await?,
        })
    }
}

fn verify_sources_unchanged(
    recorded: &MigrationState,
    current: &[SourceFingerprint],
) -> Result<()> {
    for source in &recorded.sources {
        let Some(now) = current
            .iter()
            .find(|candidate| candidate.path == source.path)
        else {
            continue;
        };
        ensure!(
            now.exists == source.exists
                && now.len == source.len
                && now.modified_unix_ms == source.modified_unix_ms
                && now.schema_version == source.schema_version,
            "legacy migration source changed since migration started; existing data preserved"
        );
    }
    Ok(())
}

fn lock_file(
    path: &Path,
) -> impl std::future::Future<Output = Result<std::fs::File>> + Send + 'static {
    let path = path.to_path_buf();
    async move {
        tokio::task::spawn_blocking(move || {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)?;
            fs4::FileExt::try_lock(&file)
                .context("session database is owned; migration postponed")?;
            Ok(file)
        })
        .await?
    }
}

fn regular_file(path: &Path) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let path = path.to_path_buf();
    async move {
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        ensure!(
            metadata.is_file() && !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata),
            "database migration target is not a regular file: {}",
            path.display()
        );
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum DatabaseAccess {
    ReadOnly,
    ReadWrite,
}

fn connect(
    path: &Path,
    access: DatabaseAccess,
) -> impl std::future::Future<Output = Result<DatabaseConnection>> + Send + 'static {
    let path = path.to_path_buf();
    async move {
        let mut options = ConnectOptions::new(match access {
            DatabaseAccess::ReadOnly => paths::sqlite_read_only_url(&path),
            DatabaseAccess::ReadWrite => paths::sqlite_url(&path),
        });
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        Ok(Database::connect(options).await?)
    }
}

fn inspect(path: &Path) -> impl std::future::Future<Output = Result<Option<i64>>> + Send + 'static {
    let path = path.to_path_buf();
    async move {
        let path = path.as_path();
        if !tokio::fs::try_exists(path).await? {
            for suffix in ["-wal", "-shm"] {
                ensure!(
                    !tokio::fs::try_exists(sidecar(path, suffix)).await?,
                    "orphan SQLite sidecar requires recovery"
                );
            }
            return Ok(None);
        }
        regular_file(path).await?;
        let db = connect(path, DatabaseAccess::ReadOnly).await?;
        let result = async {
            let rows = db.query_all_raw(sql("PRAGMA quick_check")).await?;
            let results = rows
                .into_iter()
                .map(|row| row.try_get::<String>("", "quick_check"))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ensure!(
                results.as_slice() == ["ok"],
                "corrupt database preserved: {}",
                results.join("; ")
            );
            let row = db
                .query_one_raw(sql("PRAGMA user_version"))
                .await?
                .context("missing SQLite version")?;
            Ok(Some(row.try_get::<i64>("", "user_version")?))
        }
        .await;
        finish_connection(db, result).await
    }
}

fn backup_database(
    source: &Path,
    destination: &Path,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let source = source.to_path_buf();
    let destination = destination.to_path_buf();
    async move {
        let source = source.as_path();
        let destination = destination.as_path();
        if tokio::fs::try_exists(destination).await? {
            inspect(destination)
                .await?
                .context("missing completed backup")?;
            return Ok(());
        }
        let partial = destination.with_extension("partial");
        if tokio::fs::try_exists(&partial).await? {
            regular_file(&partial).await?;
            tokio::fs::remove_file(&partial).await?;
        }
        regular_file(source).await?;
        let db = connect(source, DatabaseAccess::ReadWrite).await?;
        let result = async {
            let checkpoint = db
                .query_one_raw(sql("PRAGMA wal_checkpoint(TRUNCATE)"))
                .await?
                .context("missing WAL checkpoint result")?;
            ensure!(
                checkpoint.try_get::<i64>("", "busy")? == 0,
                "SQLite checkpoint is busy; migration postponed"
            );
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "VACUUM INTO ?",
                [Value::String(Some(
                    partial.to_str().context("non-UTF8 backup path")?.to_owned(),
                ))],
            ))
            .await?;
            Ok(())
        }
        .await;
        finish_connection(db, result).await?;
        // Windows requires a writable handle for `FlushFileBuffers`, which backs
        // `sync_all`; opening the SQLite-created file read-only returns
        // `ERROR_ACCESS_DENIED` there.
        let partial_file = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&partial)
            .await?;
        partial_file.sync_all().await?;
        drop(partial_file);
        tokio::fs::rename(&partial, destination).await?;
        sync_directory(destination.parent().context("backup has no parent")?).await
    }
}

async fn finish_connection<T: Send>(db: DatabaseConnection, result: Result<T>) -> Result<T> {
    match (result, db.close().await) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("failed to close migration database"),
        (Err(error), Err(close)) => {
            Err(error).context(format!("migration connection cleanup also failed: {close}"))
        }
    }
}

fn combine_conversion_and_shutdown<T>(result: Result<T>, shutdown: Result<()>) -> Result<T> {
    match (result, shutdown) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.context("failed to close legacy session storage")),
        (Err(error), Err(shutdown_error)) => Err(error.context(format!(
            "legacy session storage cleanup also failed: {shutdown_error}"
        ))),
    }
}

fn sql(value: &str) -> Statement {
    Statement::from_string(DatabaseBackend::Sqlite, value)
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

fn sync_directory(path: &Path) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let path = path.to_path_buf();
    async move {
        #[cfg(unix)]
        {
            tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all()).await??;
        }
        #[cfg(not(unix))]
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use pl_core::thread::cold::{ColdStore, ThreadWrite};
    use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement, Value};

    use super::report::{
        MigrationPhase, MigrationState, SessionMigrationRecord, SessionMigrationStatus,
    };
    use super::{
        ARCHIVE_DIR_NAME, CALLS_STAGING_DIR_NAME, LayoutRootAction, MigrationTrigger,
        STAGING_DIR_NAME, archive_legacy_sources, call_store_destination,
        commit_layout_publication, ensure_layout_committed, layout_publication_marker_path,
        layout_publication_pending, layout_root_action, migrate_legacy_storage,
        migration_state_path, published_or_staged_sessions_dir, run_migration,
    };
    use crate::studio::catalog::CatalogEntry;
    use crate::studio::paths::StudioPaths;
    use crate::studio::runtime_lock::{RuntimeLock, StudioHostKind};
    use crate::studio::storage::calls::legacy_call_store_blobs_dir;
    use crate::studio::store::StudioStore;
    use crate::studio::store_support;

    fn write_file(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().expect("path has a parent")).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// The single publication predicate every canonical reader shares: while the announcement exists
    /// the layout is uncommitted and the gate refuses to open, classify or create it; retiring the
    /// announcement is the one durable commit step.
    #[tokio::test]
    async fn publication_announcement_gates_every_opener_until_committed() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        let marker = layout_publication_marker_path(&paths);
        write_file(&marker, b"{\"home\":\"test\"}");

        assert!(layout_publication_pending(&paths).await.unwrap());
        assert!(
            ensure_layout_committed(&paths).await.is_err(),
            "an announced but uncommitted layout must fail closed for every opener"
        );

        commit_layout_publication(&paths).await.unwrap();
        assert!(!layout_publication_pending(&paths).await.unwrap());
        assert!(ensure_layout_committed(&paths).await.is_ok());

        // Committing an absent announcement is a no-op (idempotent resume).
        commit_layout_publication(&paths).await.unwrap();
    }

    /// Each staged layout root switch is decided by four quadrants: rename when only the staged root
    /// exists, treat as already-published when only the canonical root exists, and fail closed when
    /// both (a bypass opener would be discarded) or neither (the verified source vanished) exist.
    #[test]
    fn layout_root_action_covers_every_publication_quadrant() {
        assert_eq!(
            layout_root_action(true, false).unwrap(),
            LayoutRootAction::Rename
        );
        assert_eq!(
            layout_root_action(false, true).unwrap(),
            LayoutRootAction::AlreadyPublished
        );
        assert!(layout_root_action(true, true).is_err());
        assert!(layout_root_action(false, false).is_err());
    }

    /// A canonical call store that appears while the verified staged store is still present is a
    /// bypass collision: choosing either side would discard verified call facts, so it fails closed.
    /// A single side resolves to that side.
    #[tokio::test]
    async fn call_store_destination_refuses_a_bypass_collision() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        let calls_staging = paths.home().join(CALLS_STAGING_DIR_NAME);
        let staged = calls_staging.join("calls.sqlite");

        assert_eq!(
            call_store_destination(&paths, &calls_staging)
                .await
                .unwrap(),
            None
        );

        write_file(&staged, b"staged");
        assert_eq!(
            call_store_destination(&paths, &calls_staging)
                .await
                .unwrap(),
            Some(staged.clone())
        );

        // A canonical store appearing while the verified staged store is still present is refused.
        write_file(&paths.calls_database(), b"canonical");
        assert!(
            call_store_destination(&paths, &calls_staging)
                .await
                .is_err()
        );
    }

    /// The same collision rule protects the session layout root.
    #[tokio::test]
    async fn session_layout_root_refuses_a_bypass_collision() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        let staging = paths.home().join("sessions.staging");
        std::fs::create_dir_all(&staging).unwrap();

        assert_eq!(
            published_or_staged_sessions_dir(&paths, &staging)
                .await
                .unwrap(),
            staging.clone()
        );

        std::fs::create_dir_all(paths.sessions_dir()).unwrap();
        assert!(
            published_or_staged_sessions_dir(&paths, &staging)
                .await
                .is_err()
        );
    }

    /// Archiving retires the legacy session/call members, keeps the product database and the lock file
    /// this run owns, and is idempotent when the members were already retired by an earlier attempt.
    #[tokio::test]
    async fn archive_retires_members_and_resumes_after_a_partial_move() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("studio/sessions.sqlite");
        let product = root.path().join("studio/studio.sqlite");
        let legacy_calls = root.path().join("studio/calls.sqlite");
        let canonical_calls = root.path().join("calls/calls.sqlite");
        let migrations = root.path().join("migrations");
        let lock = sessions.with_extension("sqlite.lock");
        let blobs = legacy_call_store_blobs_dir(&legacy_calls);
        write_file(&sessions, b"sessions");
        write_file(&lock, b"");
        write_file(&product, b"product");
        write_file(&legacy_calls, b"calls");
        write_file(&blobs.join("blob-aaaa"), b"body");

        archive_legacy_sources(
            &sessions,
            &product,
            &legacy_calls,
            &canonical_calls,
            &migrations,
            None,
            Some(blobs.clone()),
        )
        .await
        .unwrap();

        let archive = migrations.join(ARCHIVE_DIR_NAME);
        assert!(archive.join("sessions.sqlite").is_file());
        assert!(archive.join("calls.sqlite").is_file());
        assert!(archive.join("blobs").is_dir());
        // The live product database and the lock this run still owns are never archived.
        assert!(product.is_file());
        assert!(lock.is_file());

        // A resumed run that finds every member already retired is a no-op.
        archive_legacy_sources(
            &sessions,
            &product,
            &legacy_calls,
            &canonical_calls,
            &migrations,
            None,
            Some(blobs),
        )
        .await
        .unwrap();
    }

    /// A member present both live and in the archive is ambiguous and must fail closed.
    #[tokio::test]
    async fn archive_fails_closed_when_a_member_exists_live_and_archived() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("studio/sessions.sqlite");
        let product = root.path().join("studio/studio.sqlite");
        let legacy_calls = root.path().join("studio/calls.sqlite");
        let canonical_calls = root.path().join("calls/calls.sqlite");
        let migrations = root.path().join("migrations");
        write_file(&sessions, b"live");
        write_file(
            &migrations.join(ARCHIVE_DIR_NAME).join("sessions.sqlite"),
            b"archived",
        );

        assert!(
            archive_legacy_sources(
                &sessions,
                &product,
                &legacy_calls,
                &canonical_calls,
                &migrations,
                None,
                None,
            )
            .await
            .is_err()
        );
    }

    // ---------------------------------------------------------------------------------------------
    // r17 deterministic publication fault-acceptance fixtures.
    //
    // These drive the *real* production conversion entry points (`migrate_legacy_storage`, the startup
    // `run_migration` phase machine and the product `StudioStore::open` opener) against unique temp
    // homes. They never touch the real `$HOME/.anywork`: every path comes from a `tempfile::TempDir`
    // and is discarded when the guard drops. Publication is a multi-root switch, so each test seeds a
    // durable phase and a disk state that a real crash could leave behind, then asserts the resume
    // either continues idempotently or fails closed with every original byte preserved.
    // ---------------------------------------------------------------------------------------------

    const FIXTURE_THREAD: &str = "thread-fixture";

    fn fixture_storage_key() -> String {
        crate::studio::paths::thread_storage_key(FIXTURE_THREAD)
    }

    /// One staged per-session summary. Its fields are the minimum `catalog.toml` merge re-reads; the
    /// empty `project_id` keeps the fixture independent of any retired Project row.
    fn catalog_entry(thread_id: &str) -> CatalogEntry {
        CatalogEntry {
            id: thread_id.to_owned(),
            project_id: String::new(),
            title: "Migrated thread".to_owned(),
            mode: pl_protocol::ThreadModeId::simple(),
            workspace_mode: pl_protocol::ThreadWorkspaceMode::Local,
            workspace_path: String::new(),
            parent_thread_id: None,
            root_thread_id: thread_id.to_owned(),
            role: "root".to_owned(),
            agent_path: thread_id.to_owned(),
            status: pl_protocol::ThreadStatus::Idle,
            created_at: 7,
            updated_at: 9,
            archived: false,
        }
    }

    /// Writes one session directory under `root/<storage-key>` holding only the staged catalog summary.
    fn seed_session_dir(root: &Path, storage_key: &str, thread_id: &str) {
        let directory = root.join(storage_key);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(super::export::STAGED_CATALOG_ENTRY_FILE_NAME),
            toml::to_string_pretty(&catalog_entry(thread_id)).unwrap(),
        )
        .unwrap();
    }

    /// Records a durable `verified` report covering one staged session and no retired call store.
    async fn seed_verified_report(paths: &StudioPaths, thread_id: &str, storage_key: &str) {
        let mut state = MigrationState::new(Vec::new(), 1, MigrationTrigger::Explicit);
        state.phase = MigrationPhase::Verified;
        state.upsert_record(SessionMigrationRecord {
            thread_id: thread_id.to_owned(),
            storage_key: storage_key.to_owned(),
            status: SessionMigrationStatus::Verified,
            journal_head: 1,
            history_watermark: 1,
            checkpoint_saved_at: 0,
            item_count: 0,
            turn_count: 0,
            attachment_count: 0,
            attachment_hashes: Vec::new(),
        });
        super::report::store(&migration_state_path(paths), &state)
            .await
            .unwrap();
    }

    /// Announces an in-progress layout switch exactly as the publication boundary does.
    fn seed_layout_marker(paths: &StudioPaths) {
        write_file(
            &layout_publication_marker_path(paths),
            b"{\"home\":\"fixture\"}",
        );
    }

    /// Runs the startup phase machine under the real exclusive runtime lock.
    async fn run_startup(paths: &StudioPaths) -> anyhow::Result<()> {
        let lock = RuntimeLock::acquire(&paths.runtime_lock(), StudioHostKind::Test).unwrap();
        let result = run_migration(paths.clone(), &lock, MigrationTrigger::Startup).await;
        drop(lock);
        result
    }

    async fn open_product_database(path: &Path) -> sea_orm::DatabaseConnection {
        let mut options = ConnectOptions::new(format!("sqlite://{}?mode=rwc", path.display()));
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        store_support::initialize_studio_schema(&db).await.unwrap();
        db
    }

    /// A current-schema product database holding one Project and one Setting and no Thread rows.
    async fn build_product_only_database(path: &Path) {
        let db = open_product_database(path).await;
        db.execute_unprepared(
            "INSERT INTO projects (id, name, path, ssh_alias, created_at, updated_at,
                last_opened_at, closed)
             VALUES ('project-migrated', 'Migrated', '/workspace/migrated', NULL, 3, 4, 4, 0);",
        )
        .await
        .unwrap();
        db.execute_unprepared(
            "INSERT INTO app_settings (key, value, updated_at)
             VALUES ('ui.theme', '\"dark\"', 6);",
        )
        .await
        .unwrap();
        db.close().await.unwrap();
    }

    /// A current-schema product database whose single Thread row has no recoverable journal.
    async fn build_directory_only_thread_database(path: &Path) {
        let db = open_product_database(path).await;
        db.execute_unprepared(
            "INSERT INTO projects (id, name, path, ssh_alias, created_at, updated_at,
                last_opened_at, closed)
             VALUES ('project-migrated', 'Migrated', '/workspace/migrated', NULL, 3, 4, 4, 0);",
        )
        .await
        .unwrap();
        db.execute_unprepared(
            "INSERT INTO threads (id, project_id, title, mode, root_thread_id, parent_thread_id,
                role, agent_path, state_json, revision, event_sequence, metadata_json, usage_json,
                last_context_tokens, trace_sequence, created_at, updated_at, archived)
             VALUES ('thread-fixture', 'project-migrated', 'Directory only', 'mode.simple',
                'thread-fixture', NULL, 'root', 'thread-fixture',
                '{\"kind\":\"idle\",\"error\":null}', 0, 0, '{}', '{}', NULL, 0, 7, 9, 0);",
        )
        .await
        .unwrap();
        db.close().await.unwrap();
    }

    fn assert_published(paths: &StudioPaths) {
        for document in [
            paths.workspaces_file(),
            paths.settings_file(),
            paths.catalog_file(),
        ] {
            assert!(
                document.is_file(),
                "missing canonical document {}",
                document.display()
            );
        }
        assert!(
            paths.calls_dir().is_dir(),
            "the canonical call root was not published"
        );
        assert!(
            !layout_publication_marker_path(paths).exists(),
            "the publication announcement was not retired"
        );
    }

    /// The whole public conversion entry runs for a product-only home: the retired product database
    /// is upgraded in place and stays live, its Project/Setting facts become the canonical documents,
    /// and an already-published home refuses a second conversion without changing a byte.
    #[tokio::test]
    async fn explicit_conversion_publishes_a_product_only_home_end_to_end() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("studio")).unwrap();
        build_product_only_database(&home.join("studio/studio.sqlite")).await;

        let outcome = migrate_legacy_storage(Some(home.clone()), home.clone())
            .await
            .unwrap();
        assert!(
            !outcome.is_failure(),
            "product-only conversion failed: {:?}",
            outcome.error
        );
        assert_eq!(outcome.phase, Some("published"));
        assert_eq!(outcome.error, None);

        let paths = StudioPaths::resolve(Some(home.clone())).unwrap();
        assert_published(&paths);
        assert!(
            paths.database().is_file(),
            "the live product database must not be archived"
        );
        assert!(
            std::fs::read_to_string(paths.workspaces_file())
                .unwrap()
                .contains("project-migrated"),
            "the migrated Project must reach workspaces.toml"
        );

        // A second explicit conversion is refused and leaves the published fact source untouched.
        let catalog_before = std::fs::read(paths.catalog_file()).unwrap();
        let refused = migrate_legacy_storage(Some(home.clone()), home.clone())
            .await
            .unwrap();
        assert!(
            refused.is_failure(),
            "an already published home must refuse a second conversion"
        );
        assert_eq!(
            std::fs::read(paths.catalog_file()).unwrap(),
            catalog_before,
            "a refused conversion must not rewrite the fact source"
        );
    }

    /// A retired Thread directory row with no recoverable journal is never published as an unopenable
    /// shell: the conversion fails closed, keeps the original product database and writes no document.
    #[tokio::test]
    async fn explicit_conversion_refuses_a_directory_only_thread_without_a_journal() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("studio")).unwrap();
        build_directory_only_thread_database(&home.join("studio/studio.sqlite")).await;

        let outcome = migrate_legacy_storage(Some(home.clone()), home.clone())
            .await
            .unwrap();
        assert!(
            outcome.is_failure(),
            "a Thread row without a journal must not be converted"
        );

        let paths = StudioPaths::resolve(Some(home.clone())).unwrap();
        assert!(
            !paths.catalog_file().exists(),
            "no canonical document may be published for an unverifiable Thread"
        );
        assert!(
            !paths.sessions_dir().exists(),
            "no canonical session root may be published for an unverifiable Thread"
        );
        assert!(
            paths.database().is_file(),
            "the original product database is preserved"
        );
    }

    /// A verified resume publishes every staged root, records `Published` and retires the announcement;
    /// re-announcing publication on the complete layout is then retired without re-publishing.
    #[tokio::test]
    async fn verified_resume_publishes_all_roots_and_commits() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let storage_key = fixture_storage_key();
        seed_verified_report(&paths, FIXTURE_THREAD, &storage_key).await;
        seed_session_dir(
            &paths.home().join(STAGING_DIR_NAME),
            &storage_key,
            FIXTURE_THREAD,
        );
        std::fs::create_dir_all(paths.home().join(CALLS_STAGING_DIR_NAME)).unwrap();

        run_startup(&paths).await.unwrap();

        assert!(
            paths
                .sessions_dir()
                .join(&storage_key)
                .join(super::export::STAGED_CATALOG_ENTRY_FILE_NAME)
                .is_file(),
            "the staged session directory was not carried into canonical sessions"
        );
        assert_published(&paths);
        let recorded = super::report::load(&migration_state_path(&paths))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recorded.phase, MigrationPhase::Published);

        seed_layout_marker(&paths);
        run_startup(&paths).await.unwrap();
        assert!(
            !layout_publication_marker_path(&paths).exists(),
            "a stale announcement on a complete layout must be retired"
        );
    }

    /// Publication renames the whole staging root, so a session directory carries every nested fact —
    /// including the per-Thread checkpoint body blobs under `blobs/checkpoint/...` that `state.toml`
    /// references — into canonical `sessions/` byte-for-byte instead of copying only top-level files.
    #[tokio::test]
    async fn publication_carries_the_whole_session_directory_including_checkpoint_blobs() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let storage_key = fixture_storage_key();
        seed_verified_report(&paths, FIXTURE_THREAD, &storage_key).await;
        let staging = paths.home().join(STAGING_DIR_NAME);
        seed_session_dir(&staging, &storage_key, FIXTURE_THREAD);
        let staged_blob = staging
            .join(&storage_key)
            .join("blobs/checkpoint/ab")
            .join("deadbeef");
        write_file(&staged_blob, b"checkpoint-body-bytes");
        std::fs::create_dir_all(paths.home().join(CALLS_STAGING_DIR_NAME)).unwrap();

        run_startup(&paths).await.unwrap();

        let published_blob = paths
            .sessions_dir()
            .join(&storage_key)
            .join("blobs/checkpoint/ab")
            .join("deadbeef");
        assert_eq!(
            std::fs::read(&published_blob).unwrap(),
            b"checkpoint-body-bytes",
            "the checkpoint body blob must be carried into the canonical session directory"
        );
    }

    /// A crash after the session root rename but before the call root rename must still finish both
    /// switches instead of skipping the verified staged call root.
    #[tokio::test]
    async fn verified_resume_continues_after_sessions_renamed_before_calls() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let storage_key = fixture_storage_key();
        seed_verified_report(&paths, FIXTURE_THREAD, &storage_key).await;
        // The session root is already canonical; only the call root is still staged.
        seed_session_dir(&paths.sessions_dir(), &storage_key, FIXTURE_THREAD);
        std::fs::create_dir_all(paths.home().join(CALLS_STAGING_DIR_NAME)).unwrap();
        seed_layout_marker(&paths);

        run_startup(&paths).await.unwrap();

        assert_published(&paths);
        let recorded = super::report::load(&migration_state_path(&paths))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recorded.phase, MigrationPhase::Published);
    }

    /// A crash after both roots were renamed resumes as an idempotent no-op and retires the
    /// announcement.
    #[tokio::test]
    async fn verified_resume_is_idempotent_after_both_roots_renamed() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let storage_key = fixture_storage_key();
        seed_verified_report(&paths, FIXTURE_THREAD, &storage_key).await;
        seed_session_dir(&paths.sessions_dir(), &storage_key, FIXTURE_THREAD);
        std::fs::create_dir_all(paths.calls_dir()).unwrap();
        seed_layout_marker(&paths);

        run_startup(&paths).await.unwrap();

        assert_published(&paths);
    }

    /// A crash after the first canonical document was written leaves a valid but partial document set;
    /// publication must complete it and still publish both layout roots.
    #[tokio::test]
    async fn verified_resume_repairs_partial_canonical_documents() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let storage_key = fixture_storage_key();
        seed_verified_report(&paths, FIXTURE_THREAD, &storage_key).await;
        seed_session_dir(
            &paths.home().join(STAGING_DIR_NAME),
            &storage_key,
            FIXTURE_THREAD,
        );
        std::fs::create_dir_all(paths.home().join(CALLS_STAGING_DIR_NAME)).unwrap();
        // Only the first of the three documents survived the crash.
        crate::studio::store::workspaces::WorkspaceStore::load(paths.workspaces_file())
            .await
            .unwrap()
            .persist_if_absent()
            .await
            .unwrap();
        assert!(paths.workspaces_file().is_file());
        assert!(!paths.settings_file().exists() && !paths.catalog_file().exists());
        seed_layout_marker(&paths);

        run_startup(&paths).await.unwrap();

        assert_published(&paths);
        assert!(paths.settings_file().is_file() && paths.catalog_file().is_file());
    }

    /// A canonical call store that appears while the verified staged store is still present (an empty
    /// root a bypass opener created) is a collision: publication refuses instead of skipping the
    /// verified staged calls, and every original byte is preserved.
    #[tokio::test]
    async fn canonical_call_root_collision_fails_closed_and_preserves_bytes() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let storage_key = fixture_storage_key();
        seed_verified_report(&paths, FIXTURE_THREAD, &storage_key).await;
        seed_session_dir(
            &paths.home().join(STAGING_DIR_NAME),
            &storage_key,
            FIXTURE_THREAD,
        );
        let staged_calls = paths
            .home()
            .join(CALLS_STAGING_DIR_NAME)
            .join(super::LEGACY_CALLS_FILE_NAME);
        write_file(&staged_calls, b"staged-call-bytes");
        write_file(&paths.calls_database(), b"bypass-empty-call-bytes");
        seed_layout_marker(&paths);

        let error = run_startup(&paths).await.unwrap_err();
        assert!(
            format!("{error:#}").contains("refusing to choose one"),
            "unexpected collision error: {error:#}"
        );

        // Neither side is renamed away or archived: the collision is resolved by a human, not guessed.
        assert_eq!(std::fs::read(&staged_calls).unwrap(), b"staged-call-bytes");
        assert_eq!(
            std::fs::read(paths.calls_database()).unwrap(),
            b"bypass-empty-call-bytes"
        );
        assert!(
            !paths
                .migrations_dir()
                .join(ARCHIVE_DIR_NAME)
                .join(super::LEGACY_CALLS_FILE_NAME)
                .exists(),
            "no unverified source may be archived"
        );
    }

    /// The real product opener is the bypass a half-published home could otherwise be consumed with:
    /// while the announcement stands it must fail closed and create no canonical root.
    #[tokio::test]
    async fn announced_publication_blocks_the_store_opener_and_creates_no_canonical_root() {
        let root = tempfile::tempdir().unwrap();
        let paths = StudioPaths::resolve(Some(root.path().to_path_buf())).unwrap();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        seed_layout_marker(&paths);

        let opened = StudioStore::open(paths.database()).await;
        assert!(
            opened.is_err(),
            "an announced but uncommitted layout must not open"
        );
        assert!(
            !paths.calls_database().exists(),
            "the opener must not create an empty canonical call store"
        );
        assert!(
            !paths.sessions_dir().exists(),
            "the opener must not create a canonical session root"
        );
        assert!(ensure_layout_committed(&paths).await.is_err());

        commit_layout_publication(&paths).await.unwrap();
        assert!(ensure_layout_committed(&paths).await.is_ok());
    }

    // ---------------------------------------------------------------------------------------------
    // r18 real legacy-home fixture.
    //
    // Unlike the phase-only fixtures above, this builds an actual pre-`catalog.toml` home using the
    // real formats: a product database with a Project and a Thread directory row, a real schema-7
    // `sessions.sqlite` journal written through the production cold-store writer (accepted input →
    // running Turn → terminal Turn), and one content-addressed attachment resource. It then runs the
    // real `migrate_legacy_storage` entry and inspects the canonical session facts the migration
    // produced. Every path lives under a `tempfile::TempDir`; the real `$HOME/.anywork` is untouched.
    // ---------------------------------------------------------------------------------------------

    const FIXTURE_INPUT: &str = "input-fixture";
    const FIXTURE_TURN: &str = "turn-fixture";
    const FIXTURE_CALL_ID: &str = "model-call-fixture";
    const FIXTURE_CALL_BODY: &[u8] =
        b"{\"kind\":\"modelCall\",\"text\":\"migrated retired call body\"}";

    fn hex_sha256(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(bytes)
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write as _;
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }

    /// The committed journal of a real (unmigrated) session: an accepted visible prompt, a Turn that
    /// references it, and the Turn reaching a terminal state. Replaying this through the production
    /// decoder yields a session with visible ordered input text, a Turn, and a durable input identity.
    fn fixture_journal() -> Vec<pl_core::thread::ThreadEffectBatch> {
        use pl_core::context::OpaquePayload;
        use pl_core::thread::input::{
            InputChange, InputDelivery, InputRecord, InputState, ThreadInput,
        };
        use pl_core::thread::{ThreadEffectBatch, TurnOutcome, TurnRecord, TurnState};

        let payload = OpaquePayload::new(
            "pl.studio.prompt",
            1,
            r#"{"text":"hello migration","presentation":"visible","attachments":[]}"#,
        )
        .expect("static product input payload");
        let accepted = InputChange::Accepted(InputRecord {
            accepted_sequence: 1,
            delivery: InputDelivery::NextTurn,
            ordinal: 1,
            revision: 1,
            state: InputState::Pending,
            input: ThreadInput {
                id: FIXTURE_INPUT.to_owned(),
                payload,
                context: Vec::new(),
            },
        });
        let running = TurnRecord {
            elapsed_ms: None,
            input_id: Some(FIXTURE_INPUT.to_owned()),
            turn_id: FIXTURE_TURN.to_owned(),
            state: TurnState::Running,
            model_steps: 0,
        };
        let finished = TurnRecord {
            elapsed_ms: Some(5),
            input_id: Some(FIXTURE_INPUT.to_owned()),
            turn_id: FIXTURE_TURN.to_owned(),
            state: TurnState::Finished(TurnOutcome::Completed),
            model_steps: 1,
        };
        vec![
            ThreadEffectBatch {
                thread_id: FIXTURE_THREAD.to_owned(),
                sequence: 1,
                committed_at: 100,
                inputs: vec![accepted].into(),
                ..Default::default()
            },
            ThreadEffectBatch {
                thread_id: FIXTURE_THREAD.to_owned(),
                sequence: 2,
                committed_at: 200,
                turn: Some(running),
                inputs: vec![InputChange::Transition {
                    id: FIXTURE_INPUT.to_owned(),
                    revision: 2,
                    state: InputState::Discarded,
                }]
                .into(),
                ..Default::default()
            },
            ThreadEffectBatch {
                thread_id: FIXTURE_THREAD.to_owned(),
                sequence: 3,
                committed_at: 300,
                turn: Some(finished),
                ..Default::default()
            },
        ]
    }

    /// Writes the real schema-7 journal through the production cold-store writer and returns the
    /// durable Thread head the migration must reproduce.
    async fn write_legacy_journal(paths: &StudioPaths) -> u64 {
        let journal = fixture_journal();
        let head = journal.last().map(|effect| effect.sequence).unwrap_or(0);
        let store = pl_core::persistence::SqliteSessionStore::open(
            pl_core::persistence::SqliteSessionOptions {
                path: paths.legacy_sessions_database(),
            },
        )
        .await
        .expect("open legacy session database");
        for effect in journal {
            let sequence = effect.sequence;
            store
                .admit(
                    FIXTURE_THREAD,
                    ThreadWrite {
                        effect: Arc::new(effect),
                        checkpoint: pl_core::thread::ThreadCheckpoint::capture(
                            FIXTURE_THREAD.to_owned(),
                            sequence,
                            pl_core::thread::ThreadSnapshot {
                                commit_sequence: sequence,
                                ..Default::default()
                            },
                        ),
                    },
                )
                .expect("admit legacy commit");
        }
        // The inherent zero-argument `SqliteSessionStore::flush` shadows the `ColdStore::flush`
        // trait method; it drains the writer through the admission watermark captured here, so after
        // it returns every admitted legacy commit is durable and the production migration may read
        // the journal. The persistence snapshot is asserted so a silently incomplete flush cannot
        // hand a half-written legacy store to the migration.
        store.flush().await.expect("flush legacy journal");
        let persistence = store.persistence();
        assert!(
            persistence.error.is_none() && persistence.durable >= persistence.admitted,
            "legacy journal was not fully durable before migration: {persistence:?}"
        );
        store
            .shutdown()
            .await
            .expect("shutdown legacy session store");
        drop(store);
        head
    }

    /// Registers one content-addressed attachment resource in the real session database and returns
    /// its hash. The blob lives under the retired global attachment root so coverage is exercised.
    async fn write_legacy_attachment(paths: &StudioPaths, body: &[u8]) -> String {
        let hash = hex_sha256(body);
        let root = paths.legacy_attachments_dir();
        std::fs::create_dir_all(&root).unwrap();
        let storage_path = root.join(&hash);
        std::fs::write(&storage_path, body).unwrap();
        let record = crate::studio::records::AttachmentRecord {
            id: "attachment-fixture".to_owned(),
            thread_id: FIXTURE_THREAD.to_owned(),
            modality: pl_protocol::studio::StudioAttachmentModality::File,
            media_type: "text/plain".to_owned(),
            filename: Some("note.txt".to_owned()),
            storage_path: storage_path.to_string_lossy().into_owned(),
            byte_size: body.len() as u64,
            content_sha256: hash.clone(),
            width: None,
            height: None,
            created_at: 150,
        };
        let entry = pl_core::storage::SessionEntry {
            session_id: FIXTURE_THREAD.to_owned(),
            id: format!("pl.resource.{}", record.id),
            turn_id: None,
            type_id: "studio.attachment".to_owned(),
            schema_version: 1,
            ordinal: 10,
            revision: 1,
            created_at: 150,
            updated_at: 150,
            payload: serde_json::to_string(&record).unwrap(),
        };
        let envelope = serde_json::to_string(&entry).unwrap();
        let payload_hash = pl_core::context::content_hash(envelope.as_bytes());
        let mut options = ConnectOptions::new(format!(
            "sqlite://{}",
            paths.legacy_sessions_database().display()
        ));
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO session_entries(session_id, id, type_id, ordinal, turn_id, envelope, \
             payload_hash) VALUES(?, ?, ?, ?, ?, ?, ?)",
            [
                Value::String(Some(entry.session_id.clone())),
                Value::String(Some(entry.id.clone())),
                Value::String(Some(entry.type_id.clone())),
                Value::BigInt(Some(i64::try_from(entry.ordinal).unwrap())),
                Value::String(None),
                Value::String(Some(envelope)),
                Value::String(Some(payload_hash)),
            ],
        ))
        .await
        .unwrap();
        db.close().await.unwrap();
        hash
    }

    /// Writes the retired per-product call store (`studio/calls.sqlite` + `studio/blobs`) with one
    /// terminal model call bound to the migrated Thread/Turn, and returns its content-addressed body.
    async fn write_legacy_call_store(paths: &StudioPaths) -> String {
        crate::studio::storage::calls::write_legacy_call_store_fixture(
            &super::legacy_product_calls_database(paths),
            FIXTURE_THREAD,
            FIXTURE_TURN,
            FIXTURE_CALL_ID,
            FIXTURE_CALL_BODY,
            3,
        )
        .await
        .expect("write retired call store fixture")
    }

    /// Builds a complete pre-`catalog.toml` home and returns the journal head, attachment hash and
    /// retired model-call body reference.
    async fn seed_real_legacy_home(paths: &StudioPaths) -> (u64, String, String) {
        build_directory_only_thread_database(&paths.database()).await;
        let head = write_legacy_journal(paths).await;
        let attachment = write_legacy_attachment(paths, b"attachment body bytes").await;
        let call_body = write_legacy_call_store(paths).await;
        (head, attachment, call_body)
    }

    /// Reads the canonical global call store and asserts the retired model call survived verbatim:
    /// Rewinds the durable migration report to `verified` so the next startup resumes the
    /// publication phase against the already-staged facts (used to script crash boundaries).
    async fn reset_report_to_verified(paths: &StudioPaths) {
        let mut state = super::report::load(&migration_state_path(paths))
            .await
            .unwrap()
            .expect("migration report exists");
        state.phase = MigrationPhase::Verified;
        state.error = None;
        super::report::store(&migration_state_path(paths), &state)
            .await
            .unwrap();
    }

    /// Reads the canonical global call store and asserts the retired model call survived verbatim:
    /// identity, association, status/terminal, usage/accounting, provider/model metadata, its
    /// content-addressed body reference and registration, and a watermark covering the head.
    async fn assert_canonical_model_call(paths: &StudioPaths, body_ref: &str) {
        let mut options =
            ConnectOptions::new(format!("sqlite://{}", paths.calls_database().display()));
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT turn_id, attempt_id, revision, terminal, status, input_tokens, output_tokens, \
                 total_tokens, decode_millis, provider_instance_id, configured_model, body_ref \
                 FROM model_calls WHERE thread_id=? AND call_id=?",
                [
                    Value::String(Some(FIXTURE_THREAD.to_owned())),
                    Value::String(Some(FIXTURE_CALL_ID.to_owned())),
                ],
            ))
            .await
            .unwrap()
            .expect("the retired model call must reach the canonical store");
        assert_eq!(row.try_get::<String>("", "turn_id").unwrap(), FIXTURE_TURN);
        assert_eq!(
            row.try_get::<String>("", "attempt_id").unwrap(),
            FIXTURE_CALL_ID
        );
        assert!(row.try_get::<i64>("", "revision").unwrap() >= 2);
        assert_eq!(row.try_get::<i64>("", "terminal").unwrap(), 1);
        assert_eq!(row.try_get::<String>("", "status").unwrap(), "completed");
        assert_eq!(
            row.try_get::<Option<i64>>("", "input_tokens").unwrap(),
            Some(10)
        );
        assert_eq!(
            row.try_get::<Option<i64>>("", "output_tokens").unwrap(),
            Some(20)
        );
        assert_eq!(
            row.try_get::<Option<i64>>("", "total_tokens").unwrap(),
            Some(30)
        );
        assert_eq!(
            row.try_get::<Option<i64>>("", "decode_millis").unwrap(),
            Some(50)
        );
        assert_eq!(
            row.try_get::<Option<String>>("", "provider_instance_id")
                .unwrap()
                .as_deref(),
            Some("provider-fixture")
        );
        assert_eq!(
            row.try_get::<Option<String>>("", "configured_model")
                .unwrap()
                .as_deref(),
            Some("model-fixture")
        );
        assert_eq!(
            row.try_get::<Option<String>>("", "body_ref")
                .unwrap()
                .as_deref(),
            Some(body_ref)
        );
        let registered = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM call_bodies WHERE body_ref=?",
                [Value::String(Some(body_ref.to_owned()))],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "count")
            .unwrap();
        assert_eq!(registered, 1, "the call body must be registered");
        let watermark = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT admitted_write_seq, durable_write_seq FROM call_watermarks WHERE thread_id=?",
                [Value::String(Some(FIXTURE_THREAD.to_owned()))],
            ))
            .await
            .unwrap()
            .expect("the canonical store must hold a durable watermark for the thread");
        let admitted = watermark.try_get::<i64>("", "admitted_write_seq").unwrap();
        let durable = watermark.try_get::<i64>("", "durable_write_seq").unwrap();
        assert!(durable >= 3 && admitted >= durable);
        db.close().await.unwrap();
    }

    /// The full migration promise: a real schema-7 journal and a real attachment convert losslessly
    /// into the current layout; the canonical history preserves order/body/input identity; the
    /// retired session journal and attachment root are archived, not deleted; and a second explicit
    /// conversion refuses without touching the published fact source.
    #[tokio::test]
    async fn real_legacy_home_converts_journal_and_attachment_losslessly() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("studio")).unwrap();
        let paths = StudioPaths::resolve(Some(home.clone())).unwrap();
        let (journal_head, attachment_hash, call_body_ref) = seed_real_legacy_home(&paths).await;
        assert!(journal_head >= 3);
        assert!(call_body_ref.starts_with("sha256:"));

        let outcome = migrate_legacy_storage(Some(home.clone()), home.clone())
            .await
            .unwrap();
        assert!(
            !outcome.is_failure(),
            "real legacy conversion failed: {:?}",
            outcome.error
        );
        assert_eq!(outcome.phase, Some("published"));
        assert_eq!(outcome.staged_sessions, 1);
        assert_eq!(outcome.verified_sessions, 1);
        assert!(outcome.calls_source_present && outcome.calls_verified);
        assert_eq!(outcome.calls_source_model_calls, 1);
        assert_eq!(outcome.calls_source_watermarks, 1);
        assert_eq!(outcome.calls_source_bodies, 1);
        assert_eq!(outcome.calls_destination_model_calls, 1);

        assert_published(&paths);
        let catalog = std::fs::read_to_string(paths.catalog_file()).unwrap();
        assert!(catalog.contains(FIXTURE_THREAD));
        assert!(
            catalog.contains("project-migrated"),
            "the migrated Thread must keep its Project membership"
        );

        let storage_key = fixture_storage_key();
        let session_dir = paths.sessions_dir().join(&storage_key);
        assert!(
            session_dir.join("state.toml").is_file()
                && session_dir.join("history.sqlite").is_file(),
            "canonical session facts were not published"
        );

        // The current checkpoint must reproduce the legacy Thread head (the migration additionally
        // appends a settlement commit when the session was not closed), never a lower watermark.
        let checkpoint =
            crate::studio::storage::state::StateStore::new(session_dir.clone(), FIXTURE_THREAD)
                .load()
                .await
                .unwrap()
                .expect("published checkpoint");
        assert!(checkpoint.state_revision >= journal_head);
        assert_eq!(checkpoint.history_fence, checkpoint.state_revision);
        assert_eq!(
            checkpoint.schema_version,
            pl_core::thread::ThreadCheckpoint::SCHEMA_VERSION,
            "the migrated session must publish the current checkpoint schema"
        );

        let history = crate::studio::storage::history::HistoryStore::open(
            &session_dir.join("history.sqlite"),
            FIXTURE_THREAD,
        )
        .await
        .unwrap();
        assert_eq!(
            history.watermark().await.unwrap(),
            checkpoint.state_revision
        );
        let identity = history
            .input_identity(FIXTURE_INPUT)
            .await
            .unwrap()
            .expect("durable input identity");
        assert!(
            !identity.entry.digest.is_empty(),
            "the migrated input identity must carry the original body digest"
        );
        let items = history.items_for_turn(FIXTURE_TURN).await.unwrap();
        let rendered = format!("{items:?}");
        assert!(
            rendered.contains("hello migration"),
            "the visible input body must survive the migration: {rendered}"
        );
        assert!(
            rendered.contains("turn-fixture"),
            "the Turn item must survive the migration: {rendered}"
        );
        // 每条物化条目都必须在与实时 streaming 共用的 `history_ordinals` 表里留下同一 ordinal 的
        // 预留：缺行会让普通启动在首次预览一条已物化条目时重新编号，与 durable 条目错位。
        let reserved = history
            .reserved_ordinals(items.iter().map(|item| item.id.clone()))
            .await
            .unwrap();
        assert_eq!(
            reserved.len(),
            items.len(),
            "every migrated item identity must hold a durable ordinal reservation: {reserved:?}"
        );

        // The retired global call database survives into the canonical store with its exact
        // identity/usage/body facts, its body is content-addressed, and its watermark covers the head.
        assert_canonical_model_call(&paths, &call_body_ref).await;
        let call_hex = call_body_ref.strip_prefix("sha256:").unwrap();
        let canonical_blob = paths.calls_dir().join("blobs").join(call_hex);
        assert_eq!(
            std::fs::read(&canonical_blob).unwrap(),
            FIXTURE_CALL_BODY,
            "the retired call body bytes must reach the canonical call blob store"
        );

        // The retired journal, call source + blobs and the global attachment root are archived, not
        // deleted; the canonical global `calls/calls.sqlite` is not archived.
        let archive = paths.migrations_dir().join(ARCHIVE_DIR_NAME);
        assert!(
            archive.join("sessions.sqlite").is_file(),
            "the retired session journal must be preserved in the archive"
        );
        assert!(
            !paths.legacy_sessions_database().exists(),
            "the retired session journal must be moved out of the live layout"
        );
        assert!(
            archive.join("calls.sqlite").is_file(),
            "the retired call database must be preserved in the archive"
        );
        assert!(
            archive.join("blobs").join(call_hex).is_file(),
            "the retired call blobs must be preserved in the archive"
        );
        assert!(
            !super::legacy_product_calls_database(&paths).exists(),
            "the retired call database must be moved out of the live layout"
        );
        assert!(
            paths.calls_database().is_file(),
            "the canonical call store must stay live"
        );
        assert!(archive.join("attachments").is_dir());
        assert!(
            archive.join("attachments").join(&attachment_hash).is_file(),
            "the migrated attachment blob must be preserved in the archive"
        );

        // A stale announcement left by a crash after `Published` is retired by the next startup and
        // the already-published layout is not re-converted.
        let catalog_published = std::fs::read(paths.catalog_file()).unwrap();
        seed_layout_marker(&paths);
        run_startup(&paths).await.unwrap();
        assert!(!layout_publication_marker_path(&paths).exists());
        assert_eq!(
            std::fs::read(paths.catalog_file()).unwrap(),
            catalog_published
        );

        // A second explicit conversion is refused and leaves the published fact source untouched.
        let refused = migrate_legacy_storage(Some(home.clone()), home.clone())
            .await
            .unwrap();
        assert!(refused.is_failure());
        assert_eq!(
            std::fs::read(paths.catalog_file()).unwrap(),
            catalog_published
        );
    }

    /// Publication boundaries replayed against the SAME real home (journal + calls + attachment).
    /// Each boundary is scripted by rewinding the durable report to `verified` and re-arranging the
    /// canonical roots exactly as a crash could have left them; startup must either resume to a
    /// coherent publication or fail closed with every source and staged byte intact.
    #[tokio::test]
    async fn real_legacy_home_resumes_publication_boundaries() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("studio")).unwrap();
        let paths = StudioPaths::resolve(Some(home.clone())).unwrap();
        let (_, _, call_body_ref) = seed_real_legacy_home(&paths).await;
        let outcome = migrate_legacy_storage(Some(home.clone()), home.clone())
            .await
            .unwrap();
        assert!(
            !outcome.is_failure(),
            "baseline real legacy conversion failed: {:?}",
            outcome.error
        );
        assert!(paths.calls_dir().is_dir());

        // (a) sessions already renamed, call root still staged: startup must finish only the call
        // root switch without losing the verified call facts.
        let staging_calls = paths.home().join(CALLS_STAGING_DIR_NAME);
        std::fs::rename(paths.calls_dir(), &staging_calls).unwrap();
        reset_report_to_verified(&paths).await;
        seed_layout_marker(&paths);
        run_startup(&paths).await.unwrap();
        assert!(
            paths.calls_dir().is_dir(),
            "the pending call root rename must resume"
        );
        assert!(!layout_publication_marker_path(&paths).exists());
        assert_canonical_model_call(&paths, &call_body_ref).await;

        // (b) both roots already renamed, marker still standing: idempotent resume, content intact.
        reset_report_to_verified(&paths).await;
        seed_layout_marker(&paths);
        run_startup(&paths).await.unwrap();
        assert!(!layout_publication_marker_path(&paths).exists());
        assert_canonical_model_call(&paths, &call_body_ref).await;

        // (c) a canonical call root that already exists while a verified staged root is still present
        // is a collision: startup must refuse instead of choosing either side and drop facts.
        let staged_calls = staging_calls.join(super::LEGACY_CALLS_FILE_NAME);
        write_file(&staged_calls, b"foreign-staged-call-store");
        reset_report_to_verified(&paths).await;
        seed_layout_marker(&paths);
        let error = run_startup(&paths).await.unwrap_err();
        assert!(
            format!("{error:#}").contains("refusing to choose one"),
            "unexpected collision error: {error:#}"
        );
        assert_eq!(
            std::fs::read(&staged_calls).unwrap(),
            b"foreign-staged-call-store",
            "the conflicting staged bytes must be preserved"
        );
        assert!(
            paths.calls_database().is_file(),
            "the canonical call store must be preserved"
        );
    }
}
