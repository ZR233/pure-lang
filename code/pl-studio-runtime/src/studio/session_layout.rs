//! Lossless upgrade from the legacy aggregate session database to the per-Thread
//! `studio/sessions/<thread-id>.sqlite` layout.
//!
//! The legacy aggregate is retained as recovery material. Nothing here is a runtime
//! read path: the coordinated startup migration performs the split, then publishes a
//! layout marker that the runtime store guard requires before it reads `sessions/`.
//! The marker asserts only the base layout — the Studio-derived timeline index is a
//! later stage (see [`index_worklist`]) and is never claimed ready here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::studio::paths::{sqlite_read_only_url, validate_storage_id};

/// Layout version written into the marker; a future layout must add a new version.
pub(in crate::studio) const SESSION_LAYOUT_VERSION: u32 = 1;

const LAYOUT_MARKER_FILE: &str = "session-layout.json";
const STAGING_DIR: &str = "session-migration";
const STAGING_SESSIONS_DIR: &str = "sessions";
const PROGRESS_FILE: &str = "progress.json";
const BACKUP_DIR: &str = "session-layout-backup";

/// Published layout marker; its presence is the single gate for reading `sessions/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) struct SessionLayoutMarker {
    pub layout_version: u32,
    /// Content fingerprint of the legacy aggregate the layout was published from.
    pub source_fingerprint: String,
    pub published_at: i64,
    /// Studio-derived index state; `Pending` until the build-index stage runs.
    pub index_state: SessionIndexState,
}

/// Whether the same-database Studio-derived index has been built for a Thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) enum SessionIndexState {
    /// Base layout published; the derived index is not built yet.
    Pending,
}

/// Persisted migration progress; allows a cancelled or crashed run to resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutProgress {
    layout_version: u32,
    source_fingerprint: String,
    #[serde(default)]
    phase: LayoutPhase,
    threads: BTreeMap<String, ThreadProgress>,
}

/// Migration phase; the target file set is complete only when `Published`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum LayoutPhase {
    #[default]
    Copying,
    Published,
}

impl LayoutProgress {
    fn new(source_fingerprint: String) -> Self {
        Self {
            layout_version: SESSION_LAYOUT_VERSION,
            source_fingerprint,
            phase: LayoutPhase::Copying,
            threads: BTreeMap::new(),
        }
    }
}

/// Verification state of one Thread copy; the digest protects the encoded bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadProgress {
    entries: u64,
    history: u64,
    head_sequence: Option<u64>,
    digest: String,
}

/// Reads the published layout marker for the product database, if any.
///
/// # Errors
/// A corrupt or unknown-version marker is reported, never silently ignored.
pub(in crate::studio) async fn published_marker(
    product_database: &Path,
) -> Result<Option<SessionLayoutMarker>> {
    let marker_path = marker_path(product_database);
    match tokio::fs::symlink_metadata(&marker_path).await {
        Ok(metadata) => ensure!(
            !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata) && metadata.is_file(),
            "session layout marker must be a regular file, not a link: {}",
            marker_path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let bytes = tokio::fs::read(&marker_path).await?;
    let marker: SessionLayoutMarker = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "session layout marker is corrupt and was preserved: {}",
            marker_path.display()
        )
    })?;
    ensure!(
        marker.layout_version == SESSION_LAYOUT_VERSION,
        "unsupported session layout version {} preserved at {}",
        marker.layout_version,
        marker_path.display()
    );
    tracing::debug!(
        layout_version = marker.layout_version,
        fingerprint = %marker.source_fingerprint,
        published_at = marker.published_at,
        index_state = ?marker.index_state,
        "read published session layout marker"
    );
    Ok(Some(marker))
}

/// Splits a legacy aggregate `sessions.sqlite` into per-Thread databases, then publishes
/// the layout marker. Idempotent and resumable; a fresh install without a legacy
/// aggregate is a no-op.
///
/// The caller must already hold the exclusive runtime lock and the legacy database lock.
///
/// # Errors
/// Rejects unknown versions, corrupt data, mismatched or stale backups, unvalidated
/// Thread ids, unresolved Thread/Project relations, unknown staging material, and a
/// partial publish that cannot be verified. Existing data is preserved on every failure.
pub(in crate::studio) async fn migrate_layout(
    product_database: &Path,
    studio_dir: &Path,
) -> Result<()> {
    let staging_root = studio_dir.join(STAGING_DIR);
    if published_marker(product_database).await?.is_some() {
        // The base layout is published; still enumerate any staging residue instead of
        // silently masking unknown material.
        let known = read_progress(&staging_root.join(PROGRESS_FILE))
            .await?
            .map(|progress| progress.threads.into_keys().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        validate_staging(&staging_root, &known).await?;
        return Ok(());
    }
    let legacy = studio_dir.join("sessions.sqlite");
    if !tokio::fs::try_exists(&legacy).await? {
        // No legacy aggregate: refuse to ignore stray migration material.
        let known = BTreeSet::new();
        validate_staging(&staging_root, &known).await?;
        return Ok(());
    }
    let sessions_dir = studio_dir.join("sessions");
    let staging_sessions = staging_root.join(STAGING_SESSIONS_DIR);
    let progress_path = staging_root.join(PROGRESS_FILE);

    // A consistent backup is bound to the exact source content it was taken from, so a
    // stale backup from a different source is never reused.
    let fingerprint = pl_core::persistence::migration::database_fingerprint(&legacy).await?;
    let backup = backup_path(studio_dir, &fingerprint);
    if let Some(parent) = backup.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // Reject a link/reparse at the backup target before any read or write.
    if let Ok(metadata) = tokio::fs::symlink_metadata(&backup).await {
        ensure!(
            !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata) && metadata.is_file(),
            "session backup must be a regular file, not a link: {}",
            backup.display()
        );
    } else if let Some(parent) = backup.parent()
        && let Ok(metadata) = tokio::fs::symlink_metadata(parent).await
    {
        ensure!(
            !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata) && metadata.is_dir(),
            "session backup directory must be a real directory: {}",
            parent.display()
        );
    }
    super::session_migration::backup_database(&legacy, &backup).await?;
    let backup_fingerprint = pl_core::persistence::migration::database_fingerprint(&backup).await?;
    ensure!(
        backup_fingerprint == fingerprint,
        "session backup does not match the current aggregate; refusing a stale backup (data preserved)"
    );

    let mut progress = match read_progress(&progress_path).await? {
        Some(existing) if existing.source_fingerprint == fingerprint => existing,
        // A different source: keep the old staging and backup untouched and report a conflict.
        Some(existing) => {
            anyhow::bail!(
                "session migration staging was produced from a different source ({}) and was preserved",
                existing.source_fingerprint
            );
        }
        None => {
            let known = BTreeSet::new();
            validate_staging(&staging_root, &known).await?;
            LayoutProgress::new(fingerprint.clone())
        }
    };
    // Resume must not trust unknown staging material: only files recorded in progress remain.
    let known: BTreeSet<String> = progress.threads.keys().cloned().collect();
    validate_staging(&staging_root, &known).await?;

    // Validate directory relations before copying anything, then migrate every Thread —
    // including a directory Thread that legitimately has no history yet (empty database).
    let sessions = pl_core::persistence::migration::session_ids(&legacy).await?;
    let relations = product_thread_relations(product_database).await?;
    validate_relations(&sessions, &relations)?;
    let targets: BTreeSet<String> = sessions
        .iter()
        .cloned()
        .chain(relations.keys().cloned())
        .collect();

    tokio::fs::create_dir_all(&staging_sessions).await?;
    tokio::fs::create_dir_all(&sessions_dir).await?;

    for id in &targets {
        let staged = session_file(&staging_sessions, id)?;
        ensure_regular_target(&staged).await?;
        reject_orphan_sidecars(&staged).await?;
        if let Some(recorded) = progress.threads.get(id)
            && verify_matches(&staged, id, recorded).await?
        {
            continue;
        }
        if tokio::fs::try_exists(&staged).await? {
            tokio::fs::remove_file(&staged).await?;
        }
        let report = pl_core::persistence::migration::copy_session(&legacy, &staged, id).await?;
        progress.threads.insert(
            id.clone(),
            ThreadProgress {
                entries: report.entries,
                history: report.history,
                head_sequence: report.head_sequence,
                digest: report.digest,
            },
        );
        write_progress(&progress_path, &progress).await?;
    }

    // Publish: move every verified staged database into `sessions/`, sync files then
    // directories, and only then write the marker. A crash before the marker leaves no
    // published layout, so the runtime never observes a partially published state.
    for id in &targets {
        let published = session_file(&sessions_dir, id)?;
        ensure_regular_target(&published).await?;
        reject_orphan_sidecars(&published).await?;
        let recorded = progress
            .threads
            .get(id)
            .context("Thread has no recorded verification")?;
        if tokio::fs::try_exists(&published).await? {
            ensure!(
                verify_matches(&published, id, recorded).await?,
                "published Thread {id} does not match its verified copy"
            );
            continue;
        }
        let staged = session_file(&staging_sessions, id)?;
        ensure!(
            verify_matches(&staged, id, recorded).await?,
            "staged Thread {id} does not match its recorded verification"
        );
        tokio::fs::rename(&staged, &published).await?;
        sync_file(&published).await?;
    }
    super::session_migration::sync_directory(&sessions_dir).await?;
    super::session_migration::sync_directory(studio_dir).await?;

    progress.phase = LayoutPhase::Published;
    write_progress(&progress_path, &progress).await?;
    let marker_path = marker_path(product_database);
    write_marker(
        &marker_path,
        &SessionLayoutMarker {
            layout_version: SESSION_LAYOUT_VERSION,
            source_fingerprint: fingerprint,
            published_at: crate::studio::unix_seconds(),
            index_state: SessionIndexState::Pending,
        },
    )
    .await?;
    sync_file(&marker_path).await?;
    super::session_migration::sync_directory(studio_dir).await?;
    let awaiting_index = index_worklist(&sessions_dir).await?;
    tracing::info!(
        threads = awaiting_index.len(),
        "session layout published; the derived timeline index is still pending"
    );
    Ok(())
}

/// Extension point for the next stage.
///
/// Returns every Thread whose database will need the same-database Studio-derived
/// index. The base layout is published without that index, so this is the build-index
/// work list — it deliberately does not claim any index is ready.
///
/// # Errors
/// Returns filesystem errors while reading the sessions directory.
pub(in crate::studio) async fn index_worklist(sessions_dir: &Path) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let mut entries = match tokio::fs::read_dir(sessions_dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
        Err(error) => return Err(error.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(id) = name.strip_suffix(".sqlite") else {
            continue;
        };
        if validate_storage_id("Thread", id).is_ok() {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    Ok(ids)
}

/// Derived session database path; the Thread id is validated before it becomes a path.
fn session_file(dir: &Path, id: &str) -> Result<PathBuf> {
    validate_storage_id("Thread", id)?;
    let path = dir.join(format!("{id}.sqlite"));
    ensure!(
        path.parent() == Some(dir),
        "Thread id escapes its storage directory: {id}"
    );
    Ok(path)
}

fn marker_path(product_database: &Path) -> PathBuf {
    product_database.with_file_name(LAYOUT_MARKER_FILE)
}

fn backup_path(studio_dir: &Path, fingerprint: &str) -> PathBuf {
    studio_dir
        .join(BACKUP_DIR)
        .join(fingerprint)
        .join("sessions.sqlite")
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// A staged or published database must be a regular file, never a symlink or reparse point.
async fn ensure_regular_target(path: &Path) -> Result<()> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => {
            ensure!(
                !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata),
                "session migration target must not be a symlink or reparse point: {}",
                path.display()
            );
            ensure!(
                metadata.is_file(),
                "session migration target is not a regular file: {}",
                path.display()
            );
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// A missing database must not leave orphan `-wal`/`-shm` sidecars behind.
async fn reject_orphan_sidecars(path: &Path) -> Result<()> {
    if tokio::fs::try_exists(path).await? {
        return Ok(());
    }
    for suffix in ["-wal", "-shm"] {
        let sidecar = sidecar_path(path, suffix);
        ensure!(
            !tokio::fs::try_exists(&sidecar).await?,
            "orphan SQLite sidecar requires recovery: {}",
            sidecar.display()
        );
    }
    Ok(())
}

async fn verify_matches(path: &Path, id: &str, recorded: &ThreadProgress) -> Result<bool> {
    if !tokio::fs::try_exists(path).await? {
        return Ok(false);
    }
    let report = pl_core::persistence::migration::verify_session(path, id).await?;
    Ok(report.entries == recorded.entries
        && report.history == recorded.history
        && report.head_sequence == recorded.head_sequence
        && report.digest == recorded.digest)
}

fn validate_relations(
    sessions: &[String],
    relations: &BTreeMap<String, ThreadRelation>,
) -> Result<()> {
    for id in sessions {
        validate_storage_id("Thread", id)?;
        ensure!(
            relations.contains_key(id),
            "session {id} has no owning Thread in the product directory; data preserved"
        );
    }
    for (id, relation) in relations {
        validate_storage_id("Thread", id)?;
        ensure!(
            relations.contains_key(&relation.root_thread_id),
            "Thread {id} references a missing root {}",
            relation.root_thread_id
        );
        match &relation.parent_thread_id {
            Some(parent) => {
                let parent = relations
                    .get(parent)
                    .with_context(|| format!("Thread {id} references a missing parent {parent}"))?;
                ensure!(
                    parent.root_thread_id == relation.root_thread_id,
                    "Thread {id} and its parent disagree on the root Thread"
                );
            }
            None => ensure!(
                relation.root_thread_id == *id,
                "root Thread {id} must be its own root"
            ),
        }
        // Parent chains must terminate at the root without a cycle.
        let mut cursor = relation.parent_thread_id.clone();
        let mut depth = 0usize;
        while let Some(current) = cursor {
            depth += 1;
            ensure!(
                depth <= relations.len(),
                "Thread ancestry under {id} is cyclic"
            );
            let node = relations
                .get(&current)
                .with_context(|| format!("Thread {id} ancestry references missing {current}"))?;
            cursor = node.parent_thread_id.clone();
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadRelation {
    root_thread_id: String,
    parent_thread_id: Option<String>,
}

async fn product_thread_relations(
    product_database: &Path,
) -> Result<BTreeMap<String, ThreadRelation>> {
    ensure!(
        tokio::fs::try_exists(product_database).await?,
        "product directory database is missing; data preserved"
    );
    let mut options = ConnectOptions::new(sqlite_read_only_url(product_database));
    options
        .max_connections(1)
        .min_connections(1)
        .sqlx_logging(false);
    let db = Database::connect(options).await?;
    let result = async {
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT id, root_thread_id, parent_thread_id FROM threads".to_owned(),
            ))
            .await?;
        let mut relations = BTreeMap::new();
        for row in rows {
            let id: String = row.try_get("", "id")?;
            let root_thread_id: String = row.try_get("", "root_thread_id")?;
            let parent_thread_id: Option<String> = row.try_get("", "parent_thread_id")?;
            relations.insert(
                id,
                ThreadRelation {
                    root_thread_id,
                    parent_thread_id,
                },
            );
        }
        Ok::<_, anyhow::Error>(relations)
    }
    .await;
    match (result, db.close().await) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("failed to close product directory database"),
    }
}

/// Reads persisted progress. Corrupt or unknown-version progress is preserved, not reused.
async fn read_progress(path: &Path) -> Result<Option<LayoutProgress>> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => ensure!(
            !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata) && metadata.is_file(),
            "session migration progress must be a regular file, not a link: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let bytes = tokio::fs::read(path).await?;
    let progress: LayoutProgress = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "session migration progress is corrupt and was preserved: {}",
            path.display()
        )
    })?;
    ensure!(
        progress.layout_version == SESSION_LAYOUT_VERSION,
        "unsupported session migration progress version {} preserved at {}",
        progress.layout_version,
        path.display()
    );
    Ok(Some(progress))
}

async fn write_progress(path: &Path, progress: &LayoutProgress) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let bytes = serde_json::to_vec(progress)?;
    pl_tool::workspace::write_file_atomically(path, &bytes).map_err(|error| anyhow::anyhow!(error))
}

async fn write_marker(path: &Path, marker: &SessionLayoutMarker) -> Result<()> {
    let bytes = serde_json::to_vec(marker)?;
    pl_tool::workspace::write_file_atomically(path, &bytes).map_err(|error| anyhow::anyhow!(error))
}

/// Rejects unknown entries, links/reparse points and orphan sidecars in the staging tree.
///
/// Known material is exactly `progress.json`, the `sessions/` directory and
/// `sessions/<valid-id>.sqlite` files whose id is recorded in the progress set.
async fn validate_staging(staging_root: &Path, sessions: &BTreeSet<String>) -> Result<()> {
    let mut entries = match tokio::fs::read_dir(staging_root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        ensure!(
            !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata),
            "session staging entry must not be a link or reparse point: {}",
            path.display()
        );
        match entry.file_name().to_str() {
            Some(name) if name == PROGRESS_FILE => ensure!(
                metadata.is_file(),
                "staging progress must be a regular file: {}",
                path.display()
            ),
            Some(name) if name == STAGING_SESSIONS_DIR => {
                ensure!(
                    metadata.is_dir(),
                    "staging sessions must be a directory: {}",
                    path.display()
                );
                validate_staging_sessions(&path, sessions).await?;
            }
            _ => anyhow::bail!(
                "unknown session migration staging material preserved at {}",
                path.display()
            ),
        }
    }
    Ok(())
}

async fn validate_staging_sessions(sessions_dir: &Path, sessions: &BTreeSet<String>) -> Result<()> {
    let mut entries = tokio::fs::read_dir(sessions_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        ensure!(
            !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata) && metadata.is_file(),
            "staged session must be a regular file, not a link or sidecar: {}",
            path.display()
        );
        let file_name = entry.file_name();
        let id = file_name
            .to_str()
            .and_then(|name| name.strip_suffix(".sqlite"))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown session migration staging material preserved at {}",
                    path.display()
                )
            })?;
        ensure!(
            validate_storage_id("Thread", id).is_ok() && sessions.contains(id),
            "unknown session migration staging material preserved at {}",
            path.display()
        );
    }
    Ok(())
}

async fn sync_file(path: &Path) -> Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all()).await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::StudioStore;
    use crate::studio::entity as entities;
    use pl_core::context::OpaquePayload;
    use pl_core::model::{
        DynModelSession, ModelError, ModelRequest, ModelSession, PreparedModelCall,
    };
    use pl_core::persistence::{SqliteSessionOptions, SqliteSessionStore};
    use pl_core::thread::cold::ColdStoreHandle;
    use pl_core::thread::input::ThreadInput;
    use pl_core::thread::{ThreadHandle, journal::ThreadCommit};
    use sea_orm::ActiveValue::Set;
    use sea_orm::{ActiveModelTrait, ConnectionTrait};
    use std::sync::Arc;

    struct NoModel;
    impl ModelSession for NoModel {
        async fn prepare(&mut self, _: ModelRequest) -> Result<PreparedModelCall, ModelError> {
            panic!("layout migration must never run a model")
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    fn encoded(commits: &[Arc<ThreadCommit>]) -> Vec<String> {
        commits
            .iter()
            .map(|commit| commit.encode().unwrap().content().to_string())
            .collect()
    }

    struct ThreadSpec {
        id: String,
        root: String,
        parent: Option<String>,
    }

    fn spec(id: &str, root: &str, parent: Option<&str>) -> ThreadSpec {
        ThreadSpec {
            id: id.into(),
            root: root.into(),
            parent: parent.map(Into::into),
        }
    }

    async fn seed_product_database(product: &Path, project_path: &str, threads: &[ThreadSpec]) {
        let store = StudioStore::open(product).await.expect("product schema");
        let now = 1_700_000_000_i64;
        let project_id = "project-fixture".to_string();
        entities::project::ActiveModel {
            id: Set(project_id.clone()),
            name: Set("Fixture".into()),
            path: Set(project_path.into()),
            ssh_alias: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            last_opened_at: Set(Some(now)),
            closed: Set(0),
        }
        .insert(store.database())
        .await
        .expect("project row");
        for thread in threads {
            entities::thread::ActiveModel {
                id: Set(thread.id.clone()),
                project_id: Set(project_id.clone()),
                title: Set(thread.id.clone()),
                mode: Set("simple".into()),
                root_thread_id: Set(thread.root.clone()),
                parent_thread_id: Set(thread.parent.clone()),
                role: Set("planner".into()),
                agent_path: Set(thread.id.clone()),
                state_json: Set("{\"kind\":\"idle\"}".into()),
                revision: Set(0),
                runtime_revision: Set(None),
                event_sequence: Set(0),
                metadata_json: Set("{}".into()),
                usage_json: Set("{}".into()),
                last_context_tokens: Set(None),
                trace_sequence: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
                archived: Set(0),
                workspace_mode: Set("local".into()),
                workspace_path: Set(project_path.into()),
                ..Default::default()
            }
            .insert(store.database())
            .await
            .expect("thread row");
        }
    }

    async fn seed_legacy_aggregate(legacy: &Path, thread_ids: &[String]) {
        let store = SqliteSessionStore::open(SqliteSessionOptions {
            path: legacy.to_path_buf(),
        })
        .await
        .expect("legacy aggregate");
        for (index, id) in thread_ids.iter().enumerate() {
            let handle = ThreadHandle::start(id.clone(), DynModelSession::new(NoModel)).unwrap();
            handle
                .attach_storage(ColdStoreHandle::new(store.clone()))
                .await
                .unwrap();
            handle
                .submit_input(ThreadInput {
                    id: format!("input-{index}"),
                    payload: OpaquePayload::new("future.input", 1, format!("payload for {id}"))
                        .unwrap(),
                    context: Vec::new(),
                })
                .await
                .unwrap();
            store
                .register_resource(
                    id,
                    &format!("attachment-{index}"),
                    OpaquePayload::new("studio.attachment", 1, "{}").unwrap(),
                )
                .unwrap();
            handle.close().await.unwrap();
        }
        store.flush().await.unwrap();
        store.shutdown().await.unwrap();
    }

    /// Injects an arbitrary session identity straight into the aggregate, as a corrupt or
    /// hostile legacy database could.
    async fn inject_session_id(legacy: &Path, session_id: &str) {
        let db = Database::connect(format!("sqlite://{}?mode=rwc", legacy.display()))
            .await
            .unwrap();
        db.execute_unprepared(&format!(
            "INSERT INTO session_history_heads(session_id,sequence,payload_hash) \
             VALUES('{session_id}',0,'deadbeef')"
        ))
        .await
        .unwrap();
        db.close().await.unwrap();
    }

    struct Fixture {
        product: PathBuf,
        studio_dir: PathBuf,
        legacy: PathBuf,
        threads: Vec<String>,
    }

    async fn fixture_with(
        threads: Vec<ThreadSpec>,
        legacy_ids: &[String],
    ) -> (tempfile::TempDir, Fixture) {
        let home = tempfile::tempdir().unwrap();
        let studio_dir = home.path().join("studio");
        tokio::fs::create_dir_all(&studio_dir).await.unwrap();
        let product = studio_dir.join("studio.sqlite");
        let legacy = studio_dir.join("sessions.sqlite");
        let project_path = home.path().join("workspace");
        seed_product_database(&product, project_path.to_str().unwrap(), &threads).await;
        seed_legacy_aggregate(&legacy, legacy_ids).await;
        (
            home,
            Fixture {
                product,
                studio_dir,
                legacy,
                threads: legacy_ids.to_vec(),
            },
        )
    }

    async fn fixture() -> (tempfile::TempDir, Fixture) {
        fixture_with(
            vec![
                spec("thread-root-a", "thread-root-a", None),
                spec("thread-root-b", "thread-root-b", None),
                spec("thread-child-a", "thread-root-a", Some("thread-root-a")),
                // A directory Thread that legitimately has no history yet.
                spec("thread-empty", "thread-empty", None),
            ],
            &[
                "thread-root-a".to_string(),
                "thread-root-b".to_string(),
                "thread-child-a".to_string(),
            ],
        )
        .await
    }

    async fn legacy_journals(legacy: &Path, ids: &[String]) -> BTreeMap<String, Vec<String>> {
        let store = SqliteSessionStore::open(SqliteSessionOptions {
            path: legacy.to_path_buf(),
        })
        .await
        .unwrap();
        let mut journals = BTreeMap::new();
        for id in ids {
            journals.insert(
                id.clone(),
                encoded(&store.read_thread_journal(id).await.unwrap()),
            );
        }
        store.shutdown().await.unwrap();
        journals
    }

    #[tokio::test]
    async fn splits_a_non_empty_aggregate_into_per_thread_databases_losslessly() {
        let (_home, fixture) = fixture().await;
        let expected = legacy_journals(&fixture.legacy, &fixture.threads).await;

        migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap();

        let marker = published_marker(&fixture.product).await.unwrap().unwrap();
        assert!(marker.index_state == SessionIndexState::Pending);
        assert!(
            fixture.legacy.exists(),
            "legacy aggregate is recovery material"
        );
        for id in &fixture.threads {
            let path = fixture
                .studio_dir
                .join("sessions")
                .join(format!("{id}.sqlite"));
            assert!(path.exists(), "per-Thread database missing for {id}");
            let report = pl_core::persistence::migration::verify_session(&path, id)
                .await
                .unwrap();
            assert!(report.entries >= 1, "entries copied for {id}: {report:?}");
        }
        // A directory-only Thread still receives its own empty database.
        let empty = fixture
            .studio_dir
            .join("sessions")
            .join("thread-empty.sqlite");
        assert!(empty.exists(), "empty Thread gets an independent database");
        let report = pl_core::persistence::migration::verify_session(&empty, "thread-empty")
            .await
            .unwrap();
        assert_eq!(report.entries, 0);
        assert_eq!(report.history, 0);

        // The runtime store guard now accepts the published layout and reads history back.
        let store = StudioStore::open(&fixture.product).await.unwrap();
        for id in &fixture.threads {
            let journal = store.sessions().read_thread_journal(id).await.unwrap();
            assert_eq!(
                encoded(&journal),
                expected[id],
                "journal preserved for {id}"
            );
        }
        assert!(
            store
                .sessions()
                .read_thread_journal("thread-empty")
                .await
                .unwrap()
                .is_empty()
        );
        store.sessions().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn rerunning_a_published_layout_is_a_noop_and_resumes_after_interruption() {
        let (_home, fixture) = fixture().await;
        migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap();
        // Remove the marker only: the staged copies and progress remain, so the next run resumes.
        tokio::fs::remove_file(fixture.product.with_file_name(LAYOUT_MARKER_FILE))
            .await
            .unwrap();
        migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap();
        assert!(published_marker(&fixture.product).await.unwrap().is_some());
        migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap();
        let marker = published_marker(&fixture.product).await.unwrap().unwrap();
        assert_eq!(marker.layout_version, SESSION_LAYOUT_VERSION);
    }

    #[tokio::test]
    async fn unknown_legacy_schema_is_rejected_and_preserved() {
        let (_home, fixture) = fixture().await;
        let db = Database::connect(format!("sqlite://{}?mode=rw", fixture.legacy.display()))
            .await
            .unwrap();
        db.execute_unprepared("PRAGMA user_version=99")
            .await
            .unwrap();
        db.close().await.unwrap();

        let error = migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("unsupported session schema"),
            "{error}"
        );
        assert!(published_marker(&fixture.product).await.unwrap().is_none());
        assert!(
            !fixture
                .studio_dir
                .join("sessions")
                .join("thread-root-a.sqlite")
                .exists()
        );
    }

    #[tokio::test]
    async fn a_changed_source_is_not_silently_reused() {
        let (_home, fixture) = fixture().await;
        migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap();
        tokio::fs::remove_file(fixture.product.with_file_name(LAYOUT_MARKER_FILE))
            .await
            .unwrap();
        // Grow the source with an unowned session; the run must fail and preserve.
        seed_legacy_aggregate(&fixture.legacy, &["thread-late".to_string()]).await;
        let progress = fixture
            .studio_dir
            .join("session-migration")
            .join("progress.json");
        let error = migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("different source"), "{error}");
        assert!(published_marker(&fixture.product).await.unwrap().is_none());
        // The old staging is preserved, never cleared.
        assert!(progress.exists(), "old staging progress must be preserved");
    }

    #[tokio::test]
    async fn unknown_staging_material_is_rejected_and_preserved() {
        let (_home, fixture) = fixture().await;
        let staging = fixture.studio_dir.join("session-migration");
        tokio::fs::create_dir_all(&staging).await.unwrap();
        let stray = staging.join("stray.txt");
        tokio::fs::write(&stray, "keep me").await.unwrap();

        let error = migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unknown session migration staging material"),
            "{error}"
        );
        assert!(stray.exists(), "unknown material must be preserved");
    }

    #[tokio::test]
    async fn a_published_marker_still_reports_unknown_staging_residue() {
        let (_home, fixture) = fixture().await;
        migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap();
        let stray = fixture
            .studio_dir
            .join("session-migration")
            .join("residue.toml");
        tokio::fs::write(&stray, "residue").await.unwrap();

        let error = migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unknown session migration staging material"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_linked_control_file_is_rejected() {
        let (_home, fixture) = fixture().await;
        let staging = fixture.studio_dir.join("session-migration");
        tokio::fs::create_dir_all(&staging).await.unwrap();
        std::os::unix::fs::symlink(
            fixture.studio_dir.join("elsewhere.json"),
            staging.join("progress.json"),
        )
        .unwrap();

        let error = migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not a link"), "{error}");
    }

    #[tokio::test]
    async fn a_traversal_session_id_is_rejected_before_any_path_is_derived() {
        let (_home, fixture) = fixture().await;
        inject_session_id(&fixture.legacy, "../evil").await;
        let error = migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Thread id"), "{error}");
        assert!(published_marker(&fixture.product).await.unwrap().is_none());
        assert!(
            !fixture
                .studio_dir
                .join("session-migration")
                .join("sessions")
                .exists()
        );
    }

    #[tokio::test]
    async fn unknown_or_corrupt_progress_is_preserved() {
        for (name, contents) in [
            (
                "unknown",
                "{\"layoutVersion\":999,\"sourceFingerprint\":\"x\",\"threads\":{}}",
            ),
            ("corrupt", "not-json"),
        ] {
            let (_home, fixture) = fixture().await;
            let staging = fixture.studio_dir.join("session-migration");
            tokio::fs::create_dir_all(&staging).await.unwrap();
            let progress = staging.join("progress.json");
            tokio::fs::write(&progress, contents).await.unwrap();

            let error = migrate_layout(&fixture.product, &fixture.studio_dir)
                .await
                .unwrap_err();
            assert!(!error.to_string().is_empty(), "{name}");
            assert!(
                progress.exists(),
                "{name}: unknown material must be preserved"
            );
            assert!(published_marker(&fixture.product).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn a_cyclic_thread_ancestry_is_rejected_and_preserved() {
        // A parent cycle between two Threads.
        let (_home, fixture) = fixture_with(
            vec![
                spec("cycle-a", "cycle-b", Some("cycle-b")),
                spec("cycle-b", "cycle-b", Some("cycle-a")),
            ],
            &["cycle-a".to_string()],
        )
        .await;
        let error = migrate_layout(&fixture.product, &fixture.studio_dir)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("cyclic"), "{error}");
        assert!(published_marker(&fixture.product).await.unwrap().is_none());
    }
}
