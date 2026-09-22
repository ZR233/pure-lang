//! Preserves a failed legacy home before permitting a fresh startup.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use super::PermanentMigrationReason;
use crate::studio::paths::StudioPaths;

const MARKER: &str = "fresh-start-recovery.json";
const NOTICE: &str = "fresh-start-notice.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Archiving,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryMarker {
    version: u32,
    archive_name: String,
    reason: PermanentMigrationReason,
    started_at: i64,
    phase: Phase,
    fresh_attempt: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct RecoveryNotice {
    pub(crate) archive: PathBuf,
    pub(crate) reason: PermanentMigrationReason,
}

fn marker_path(paths: &StudioPaths) -> PathBuf {
    paths.data_dir().join(MARKER)
}

fn notice_path(paths: &StudioPaths) -> PathBuf {
    paths.data_dir().join(NOTICE)
}

fn archive_path(paths: &StudioPaths, marker: &RecoveryMarker) -> Result<PathBuf> {
    let home = paths.home();
    let parent = home
        .parent()
        .context("Studio home has no parent for a recovery archive")?;
    let prefix = format!(
        "{}.recovery-",
        home.file_name()
            .context("Studio home has no name")?
            .to_string_lossy()
    );
    ensure!(
        marker.version == 1
            && marker.archive_name.starts_with(&prefix)
            && marker.archive_name.len() > prefix.len()
            && marker.archive_name[prefix.len()..]
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric()),
        "invalid fresh-start recovery marker; existing data preserved"
    );
    Ok(parent.join(&marker.archive_name))
}

async fn load(path: &Path) -> Result<Option<RecoveryMarker>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
            format!("invalid fresh-start recovery record {}", path.display())
        })?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

async fn store(path: &Path, marker: &RecoveryMarker) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(marker)?;
    let parent = path
        .parent()
        .context("recovery record has no parent")?
        .to_path_buf();
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || pl_tool::workspace::write_file_atomically(&path, &bytes))
        .await??;
    super::sync_directory(&parent).await
}

/// Completes an interrupted archive before any legacy or canonical opener can see the home.
pub(super) async fn resume(paths: &StudioPaths) -> Result<bool> {
    let Some(mut marker) = load(&marker_path(paths)).await? else {
        return Ok(false);
    };
    let archive = archive_path(paths, &marker)?;
    let metadata = tokio::fs::symlink_metadata(&archive).await?;
    ensure!(
        metadata.is_dir() && !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata),
        "fresh-start archive is missing; existing data preserved"
    );
    match marker.phase {
        Phase::Archiving => finish_archive(paths, &archive, &mut marker).await?,
        Phase::Archived => {
            // No user-facing runtime was published while the marker remained. Preserve even the
            // incomplete fresh attempt before retrying its initialization.
            let interrupted = archive.join(format!("interrupted-fresh-{}", marker.fresh_attempt));
            move_home_contents(paths, &interrupted).await?;
            marker.fresh_attempt = marker
                .fresh_attempt
                .checked_add(1)
                .context("fresh-start attempt overflow")?;
            store(&marker_path(paths), &marker).await?;
        }
    }
    Ok(true)
}

pub(super) async fn archive_failed_home(
    paths: &StudioPaths,
    reason: PermanentMigrationReason,
) -> Result<()> {
    ensure!(
        load(&marker_path(paths)).await?.is_none() && load(&notice_path(paths)).await?.is_none(),
        "another fresh-start recovery already exists; existing data preserved"
    );
    let home = paths.home();
    let parent = home
        .parent()
        .context("Studio home has no parent for a recovery archive")?;
    let prefix = format!(
        "{}.recovery-",
        home.file_name()
            .context("Studio home has no name")?
            .to_string_lossy()
    );
    let archive = tempfile::Builder::new()
        .prefix(&prefix)
        .tempdir_in(parent)?
        .keep();
    let archive_name = archive
        .file_name()
        .context("recovery archive has no name")?
        .to_string_lossy()
        .into_owned();
    let mut marker = RecoveryMarker {
        version: 1,
        archive_name,
        reason,
        started_at: crate::studio::ids::unix_seconds(),
        phase: Phase::Archiving,
        fresh_attempt: 0,
    };
    store(&marker_path(paths), &marker).await?;
    finish_archive(paths, &archive, &mut marker).await
}

async fn finish_archive(
    paths: &StudioPaths,
    archive: &Path,
    marker: &mut RecoveryMarker,
) -> Result<()> {
    move_home_contents(paths, archive).await?;
    marker.phase = Phase::Archived;
    store(&marker_path(paths), marker).await?;
    tracing::warn!(
        archive = %archive.display(),
        reason = ?marker.reason,
        "unrecoverable Studio migration archived; starting with default state"
    );
    Ok(())
}

async fn move_home_contents(paths: &StudioPaths, archive: &Path) -> Result<()> {
    tokio::fs::create_dir_all(archive).await?;
    for entry in list_entries(paths.home()).await? {
        if entry != "studio" {
            move_entry(&paths.home().join(&entry), &archive.join(&entry)).await?;
        }
    }
    let studio_target = archive.join("studio");
    tokio::fs::create_dir_all(&studio_target).await?;
    for entry in list_entries(paths.data_dir()).await? {
        if entry != "runtime.lock" && entry != MARKER {
            move_entry(&paths.data_dir().join(&entry), &studio_target.join(&entry)).await?;
        }
    }
    ensure!(
        list_entries(paths.home()).await? == ["studio"],
        "Studio home changed during fresh-start archival; existing data preserved"
    );
    ensure!(
        list_entries(paths.data_dir())
            .await?
            .iter()
            .all(|name| name == "runtime.lock" || name == MARKER),
        "Studio data directory changed during fresh-start archival; existing data preserved"
    );
    super::sync_directory(paths.home()).await?;
    super::sync_directory(archive).await?;
    Ok(())
}

async fn list_entries(path: &Path) -> Result<Vec<std::ffi::OsString>> {
    let mut directory = tokio::fs::read_dir(path).await?;
    let mut entries = Vec::new();
    while let Some(entry) = directory.next_entry().await? {
        entries.push(entry.file_name());
    }
    entries.sort();
    Ok(entries)
}

async fn move_entry(source: &Path, target: &Path) -> Result<()> {
    // A source recreated after a crash and an already archived target are different facts.
    // Never overwrite or silently choose either one.
    ensure!(
        !entry_exists(target).await?,
        "fresh-start archive collision at {}; existing data preserved",
        target.display()
    );
    tokio::fs::rename(source, target).await.with_context(|| {
        format!(
            "failed to archive Studio data {} into {}",
            source.display(),
            target.display()
        )
    })?;
    super::sync_directory(source.parent().context("archived entry has no parent")?).await?;
    super::sync_directory(target.parent().context("archive entry has no parent")?).await
}

/// Commits the recovery only after the default database and configuration were initialized.
pub(crate) async fn finalize(paths: &StudioPaths) -> Result<()> {
    let path = marker_path(paths);
    let Some(marker) = load(&path).await? else {
        return Ok(());
    };
    ensure!(
        marker.phase == Phase::Archived,
        "fresh-start archive is incomplete"
    );
    let archive = archive_path(paths, &marker)?;
    ensure!(
        tokio::fs::try_exists(&archive).await?,
        "fresh-start archive is missing"
    );
    let notice = notice_path(paths);
    if entry_exists(&notice).await? {
        bail!("fresh-start notice already exists; refusing to overwrite it");
    }
    tokio::fs::rename(&path, &notice).await?;
    super::sync_directory(paths.data_dir()).await
}

pub(crate) async fn read_notice(paths: &StudioPaths) -> Result<Option<RecoveryNotice>> {
    let Some(marker) = load(&notice_path(paths)).await? else {
        return Ok(None);
    };
    ensure!(
        marker.phase == Phase::Archived,
        "fresh-start notice is incomplete"
    );
    Ok(Some(RecoveryNotice {
        archive: archive_path(paths, &marker)?,
        reason: marker.reason,
    }))
}

async fn entry_exists(path: &Path) -> Result<bool> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, StudioPaths, PathBuf, RecoveryMarker) {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let paths = StudioPaths::resolve(Some(home)).unwrap();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let archive_name = "home.recovery-fixture".to_owned();
        let archive = root.path().join(&archive_name);
        std::fs::create_dir_all(&archive).unwrap();
        let marker = RecoveryMarker {
            version: 1,
            archive_name,
            reason: PermanentMigrationReason::MissingMigrationFacts,
            started_at: 1,
            phase: Phase::Archiving,
            fresh_attempt: 0,
        };
        (root, paths, archive, marker)
    }

    #[tokio::test]
    async fn interrupted_archive_resumes_and_keeps_all_original_entries() {
        let (_root, paths, archive, marker) = fixture();
        std::fs::write(archive.join("config.toml"), b"old config").unwrap();
        std::fs::write(paths.database(), b"old product").unwrap();
        std::fs::create_dir_all(paths.migrations_dir()).unwrap();
        std::fs::write(
            paths.migrations_dir().join("session-migration.json"),
            b"old report",
        )
        .unwrap();
        store(&marker_path(&paths), &marker).await.unwrap();

        assert!(resume(&paths).await.unwrap());
        assert!(!paths.database().is_file());
        assert_eq!(
            std::fs::read(archive.join("config.toml")).unwrap(),
            b"old config"
        );
        assert_eq!(
            std::fs::read(archive.join("studio/studio.sqlite")).unwrap(),
            b"old product"
        );
        assert_eq!(
            std::fs::read(archive.join("migrations/session-migration.json")).unwrap(),
            b"old report"
        );
        finalize(&paths).await.unwrap();
        assert!(!resume(&paths).await.unwrap());
        assert_eq!(read_notice(&paths).await.unwrap().unwrap().archive, archive);
    }

    #[tokio::test]
    async fn archive_collision_preserves_both_sides_and_blocks_fresh_start() {
        let (_root, paths, archive, marker) = fixture();
        std::fs::write(paths.config_file(), b"live").unwrap();
        std::fs::write(archive.join("config.toml"), b"archived").unwrap();
        store(&marker_path(&paths), &marker).await.unwrap();

        assert!(resume(&paths).await.is_err());
        assert_eq!(std::fs::read(paths.config_file()).unwrap(), b"live");
        assert_eq!(
            std::fs::read(archive.join("config.toml")).unwrap(),
            b"archived"
        );
        assert_eq!(
            load(&marker_path(&paths)).await.unwrap().unwrap().phase,
            Phase::Archiving
        );
    }

    #[tokio::test]
    async fn interrupted_fresh_initialization_is_preserved_before_retry() {
        let (_root, paths, archive, mut marker) = fixture();
        marker.phase = Phase::Archived;
        std::fs::write(paths.catalog_file(), b"partial fresh catalog").unwrap();
        store(&marker_path(&paths), &marker).await.unwrap();

        assert!(resume(&paths).await.unwrap());
        assert!(!paths.catalog_file().exists());
        assert_eq!(
            std::fs::read(archive.join("interrupted-fresh-0/catalog.toml")).unwrap(),
            b"partial fresh catalog"
        );
        assert_eq!(
            load(&marker_path(&paths))
                .await
                .unwrap()
                .unwrap()
                .fresh_attempt,
            1
        );
    }
}
