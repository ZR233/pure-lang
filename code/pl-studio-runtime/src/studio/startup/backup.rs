use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::StudioStartupRecovery;

const MEMBERS: [&str; 4] = ["config.toml", "agents", "v2", "studio/v2"];
const JOURNAL: &str = "startup-recovery.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Moving,
    BackedUp,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MoveState {
    Absent,
    Pending,
    Moved,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchivePass {
    directory: String,
    members: Vec<MoveState>,
    directories: Vec<bool>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: u32,
    id: String,
    reason: String,
    created_at: i64,
    phase: Phase,
    original: ArchivePass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interrupted: Option<ArchivePass>,
}

pub(in crate::studio) struct StartupBackup {
    home: PathBuf,
    record: Record,
}

impl StartupBackup {
    pub(in crate::studio) fn resume(home: &Path) -> Result<Option<Self>> {
        let path = home.join(JOURNAL);
        check_ancestors(home, &path)?;
        metadata(&path)?;
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let record: Record = toml::from_str(&content)
            .context("invalid startup recovery journal; preserving recovery state")?;
        ensure!(
            record.version == 1 && valid_id(&record.id),
            "unsupported startup recovery journal"
        );
        validate_pass(&record.original)?;
        ensure!(
            record.original.directory == "original",
            "invalid original backup directory"
        );
        if let Some(pass) = &record.interrupted {
            validate_pass(pass)?;
        }
        if record.phase == Phase::Completed {
            std::fs::remove_file(path)?;
            return Ok(None);
        }
        let mut backup = Self {
            home: home.to_owned(),
            record,
        };
        check_ancestors(home, &backup.root().join("recovery.toml"))?;
        ensure!(
            metadata(&backup.root())?.is_some_and(|meta| meta.is_dir()),
            "startup backup directory is missing"
        );
        backup.move_original()?;
        backup.verify_original()?;
        // A completed backup can be followed by an interrupted, unpublished default attempt.
        // Preserve that attempt separately; never treat its bytes as the original installation.
        if backup.record.interrupted.is_none() {
            backup.record.interrupted =
                Some(inventory(home, crate::studio::ids::new_id("interrupted"))?);
            backup.persist()?;
        }
        backup.move_interrupted()?;
        Ok(Some(backup))
    }

    pub(in crate::studio) fn begin(home: &Path, reason: String) -> Result<Self> {
        check_ancestors(home, &home.join(JOURNAL))?;
        ensure!(
            !home.join(JOURNAL).try_exists()?,
            "startup recovery journal already exists"
        );
        let record = Record {
            version: 1,
            id: crate::studio::ids::new_id("recovery"),
            reason,
            created_at: crate::studio::unix_seconds(),
            phase: Phase::Moving,
            original: inventory(home, "original".into())?,
            interrupted: None,
        };
        let mut backup = Self {
            home: home.to_owned(),
            record,
        };
        check_ancestors(home, &backup.root().join("recovery.toml"))?;
        std::fs::create_dir_all(home.join("startup-backups"))?;
        std::fs::create_dir(backup.root())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(backup.root(), std::fs::Permissions::from_mode(0o700))?;
        }
        backup.persist()?;
        backup.move_original()?;
        Ok(backup)
    }

    pub(in crate::studio) fn report(&self) -> StudioStartupRecovery {
        StudioStartupRecovery {
            backup_path: self.root().to_string_lossy().into_owned(),
            reason: self.record.reason.clone(),
            created_at: self.record.created_at,
        }
    }

    pub(in crate::studio) fn complete(&mut self) -> Result<()> {
        self.record.phase = Phase::Completed;
        self.persist()?;
        // The completed pointer is harmless if removal fails; the next launch removes it.
        if let Err(error) = std::fs::remove_file(self.home.join(JOURNAL)) {
            tracing::warn!(%error, "completed startup recovery journal retained");
        }
        Ok(())
    }

    fn root(&self) -> PathBuf {
        self.home.join("startup-backups").join(&self.record.id)
    }

    fn persist(&self) -> Result<()> {
        for path in [self.home.join(JOURNAL), self.root().join("recovery.toml")] {
            check_ancestors(&self.home, &path)?;
            metadata(&path)?;
        }
        let content = toml::to_string_pretty(&self.record)?;
        pl_tool::workspace::write_file_atomically(&self.home.join(JOURNAL), content.as_bytes())?;
        pl_tool::workspace::write_file_atomically(
            &self.root().join("recovery.toml"),
            content.as_bytes(),
        )?;
        sync_ancestors(&self.home, &self.root().join("recovery.toml"))?;
        Ok(())
    }

    fn move_original(&mut self) -> Result<()> {
        if self.record.phase != Phase::Moving {
            return Ok(());
        }
        for index in 0..MEMBERS.len() {
            move_member(&self.home, &self.root(), &mut self.record.original, index)?;
            self.persist()?;
        }
        self.record.phase = Phase::BackedUp;
        self.persist()
    }

    fn move_interrupted(&mut self) -> Result<()> {
        let root = self.root();
        for index in 0..MEMBERS.len() {
            if let Some(pass) = &mut self.record.interrupted {
                move_member(&self.home, &root, pass, index)?;
            }
            self.persist()?;
        }
        self.record.interrupted = None;
        self.persist()
    }

    fn verify_original(&self) -> Result<()> {
        for (index, member) in MEMBERS.iter().enumerate() {
            let target = self.root().join("original").join(member);
            check_ancestors(&self.home, &target)?;
            let meta = metadata(&target)?;
            match self.record.original.members[index] {
                MoveState::Absent => ensure!(meta.is_none(), "unexpected backup member"),
                MoveState::Moved => ensure!(
                    meta.is_some_and(
                        |meta| meta.is_dir() == self.record.original.directories[index]
                    ),
                    "backup member is missing or changed"
                ),
                MoveState::Pending => anyhow::bail!("backup is incomplete"),
            }
        }
        Ok(())
    }
}

fn inventory(home: &Path, directory: String) -> Result<ArchivePass> {
    let mut members = Vec::new();
    let mut directories = Vec::new();
    for member in MEMBERS {
        let path = home.join(member);
        check_ancestors(home, &path)?;
        let meta = metadata(&path)?;
        directories.push(meta.as_ref().is_some_and(|meta| meta.is_dir()));
        members.push(if meta.is_some() {
            MoveState::Pending
        } else {
            MoveState::Absent
        });
    }
    Ok(ArchivePass {
        directory,
        members,
        directories,
    })
}

fn validate_pass(pass: &ArchivePass) -> Result<()> {
    ensure!(
        pass.members.len() == MEMBERS.len() && pass.directories.len() == MEMBERS.len(),
        "invalid backup member count"
    );
    ensure!(
        pass.directory == "original"
            || (pass.directory.starts_with("interrupted-") && valid_id(&pass.directory)),
        "invalid backup relative directory"
    );
    Ok(())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn metadata(path: &Path) -> Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(
                !meta.file_type().is_symlink(),
                "startup recovery refuses symbolic links: {}",
                path.display()
            );
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                ensure!(
                    meta.file_attributes() & 0x400 == 0,
                    "startup recovery refuses reparse points: {}",
                    path.display()
                );
            }
            Ok(Some(meta))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn move_member(home: &Path, root: &Path, pass: &mut ArchivePass, index: usize) -> Result<()> {
    if pass.members[index] == MoveState::Absent {
        return Ok(());
    }
    let source = home.join(MEMBERS[index]);
    let target = root.join(&pass.directory).join(MEMBERS[index]);
    check_ancestors(home, &source)?;
    check_ancestors(home, &target)?;
    let source_exists = metadata(&source)?.is_some();
    let target_exists = metadata(&target)?.is_some();
    if pass.members[index] == MoveState::Moved {
        ensure!(
            !source_exists && target_exists,
            "backup member changed after move: {}",
            source.display()
        );
        return Ok(());
    }
    match (source_exists, target_exists) {
        (true, false) => {
            let parent = target.parent().context("backup member has no parent")?;
            std::fs::create_dir_all(parent)?;
            check_ancestors(home, &target)?;
            sync_ancestors(home, &target)?;
            pl_tool::workspace::move_owned_path(&source, &target)
                .with_context(|| format!("backup move failed: {}", source.display()))?;
        }
        (false, true) => {} // Rename completed before the journal update.
        _ => anyhow::bail!(
            "backup member is missing or conflicts: {}",
            source.display()
        ),
    }
    sync_ancestors(home, &source)?;
    sync_ancestors(home, &target)?;
    ensure!(
        metadata(&target)?.is_some_and(|meta| meta.is_dir() == pass.directories[index]),
        "backup member type changed"
    );
    pass.members[index] = MoveState::Moved;
    Ok(())
}

fn check_ancestors(home: &Path, path: &Path) -> Result<()> {
    let relative = path
        .strip_prefix(home)
        .context("backup path escaped data root")?;
    let mut directory = home.to_owned();
    ensure!(
        metadata(&directory)?.is_none_or(|meta| meta.is_dir()),
        "data root is not a directory"
    );
    let components = relative.components().collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "invalid backup path component"
        );
        directory.push(component);
        ensure!(
            metadata(&directory)?.is_none_or(|meta| meta.is_dir()),
            "backup ancestor is not a directory"
        );
    }
    Ok(())
}

fn sync_ancestors(home: &Path, path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut directory = path.parent().context("backup path has no parent")?;
        loop {
            std::fs::File::open(directory)?.sync_all()?;
            if directory == home {
                break;
            }
            directory = directory
                .parent()
                .context("backup parent escaped data root")?;
        }
    }
    #[cfg(not(unix))]
    let _ = (home, path);
    Ok(())
}

#[cfg(test)]
mod migration_fault_tests {
    use super::*;

    fn pending(home: &Path) -> Result<StartupBackup> {
        std::fs::create_dir_all(home.join("studio/v2"))?;
        std::fs::create_dir_all(home.join("v2/calls"))?;
        std::fs::write(home.join("config.toml"), b"original config")?;
        for suffix in ["", "-wal", "-shm"] {
            std::fs::write(
                home.join(format!("studio/v2/studio.sqlite{suffix}")),
                suffix.as_bytes(),
            )?;
        }
        std::fs::write(home.join("v2/calls/calls.sqlite"), b"calls bytes")?;
        let backup = StartupBackup {
            home: home.to_owned(),
            record: Record {
                version: 1,
                id: crate::studio::ids::new_id("recovery"),
                reason: "safe reason".into(),
                created_at: crate::studio::unix_seconds(),
                phase: Phase::Moving,
                original: inventory(home, "original".into())?,
                interrupted: None,
            },
        };
        std::fs::create_dir_all(backup.root())?;
        backup.persist()?;
        Ok(backup)
    }

    #[test]
    fn resumes_rename_before_journal_and_archives_interrupted_defaults() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut backup = pending(temp.path())?;
        let root = backup.root();
        move_member(temp.path(), &root, &mut backup.record.original, 0)?;
        // The on-disk journal still says Pending, as if the process exited just after rename.
        drop(backup);
        let resumed = StartupBackup::resume(temp.path())?.expect("resumed backup");
        assert_eq!(resumed.root(), root);
        for suffix in ["", "-wal", "-shm"] {
            assert_eq!(
                std::fs::read(root.join(format!("original/studio/v2/studio.sqlite{suffix}")))?,
                suffix.as_bytes()
            );
        }
        std::fs::write(temp.path().join("config.toml"), b"interrupted defaults")?;
        drop(resumed);
        let mut resumed = StartupBackup::resume(temp.path())?.expect("same recovery");
        let interrupted = std::fs::read_dir(&root)?
            .filter_map(|entry| entry.ok())
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("interrupted-")
            })
            .expect("interrupted attempt saved");
        assert_eq!(
            std::fs::read(interrupted.path().join("config.toml"))?,
            b"interrupted defaults"
        );
        assert_eq!(
            std::fs::read(root.join("original/config.toml"))?,
            b"original config"
        );
        resumed.complete()?;
        assert!(StartupBackup::resume(temp.path())?.is_none());
        assert_eq!(
            std::fs::read_dir(temp.path().join("startup-backups"))?.count(),
            1
        );
        Ok(())
    }

    #[test]
    fn conflicting_or_missing_backup_stops_without_overwriting() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let backup = pending(temp.path())?;
        let target = backup.root().join("original/config.toml");
        std::fs::create_dir_all(target.parent().expect("parent"))?;
        std::fs::write(&target, b"existing backup")?;
        assert!(StartupBackup::resume(temp.path()).is_err());
        assert_eq!(std::fs::read(&target)?, b"existing backup");
        assert_eq!(
            std::fs::read(temp.path().join("config.toml"))?,
            b"original config"
        );
        std::fs::remove_file(&target)?;
        let backup = StartupBackup::resume(temp.path())?.expect("resume after conflict fixed");
        std::fs::remove_file(backup.root().join("original/config.toml"))?;
        std::fs::write(temp.path().join("config.toml"), b"new attempt")?;
        assert!(StartupBackup::resume(temp.path()).is_err());
        assert_eq!(
            std::fs::read(temp.path().join("config.toml"))?,
            b"new attempt"
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn occupied_source_preserves_journal_and_resumes_same_backup() -> Result<()> {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir()?;
        let backup = pending(temp.path())?;
        let root = backup.root();
        let occupied = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(temp.path().join("config.toml"))?;
        assert!(StartupBackup::resume(temp.path()).is_err());
        assert_eq!(
            std::fs::read_dir(temp.path().join("startup-backups"))?.count(),
            1
        );
        drop(occupied);
        let resumed = StartupBackup::resume(temp.path())?.expect("continued recovery");
        assert_eq!(resumed.root(), root);
        assert_eq!(
            std::fs::read(root.join("original/config.toml"))?,
            b"original config"
        );
        Ok(())
    }
}
