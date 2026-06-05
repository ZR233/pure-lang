use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use include_dir::Dir;
use pl_protocol::Result;

use super::SYSTEM_MARKER_FILE_NAME;
use super::util::expand_home;
use crate::config::SkillsConfig;

pub(super) const SYSTEM_SKILLS_DIR: Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/src/skill/system_assets");

pub fn install_system_skills(config: &SkillsConfig) -> Result<PathBuf> {
    let system_dir = system_skills_dir(config)?;
    install_system_skills_to_dir(&system_dir)?;
    Ok(system_dir)
}

pub(super) fn system_skills_dir(config: &SkillsConfig) -> Result<PathBuf> {
    Ok(expand_home(&config.user_dir)?.join(".system"))
}

fn install_system_skills_to_dir(system_dir: &Path) -> Result<()> {
    let marker_path = system_dir.join(SYSTEM_MARKER_FILE_NAME);
    let expected = embedded_system_skills_fingerprint();
    if system_dir.is_dir()
        && fs::read_to_string(&marker_path).is_ok_and(|marker| marker.trim() == expected)
    {
        return Ok(());
    }

    if system_dir.exists() {
        fs::remove_dir_all(system_dir)?;
    }
    write_embedded_dir(&SYSTEM_SKILLS_DIR, system_dir)?;
    fs::write(marker_path, format!("{expected}\n"))?;
    Ok(())
}

fn embedded_system_skills_fingerprint() -> String {
    let mut items = Vec::new();
    collect_fingerprint_items(&SYSTEM_SKILLS_DIR, &mut items);
    items.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));

    let mut hasher = DefaultHasher::new();
    super::SYSTEM_MARKER_SALT.hash(&mut hasher);
    for (path, contents_hash) in items {
        path.hash(&mut hasher);
        contents_hash.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

fn collect_fingerprint_items(dir: &Dir<'_>, items: &mut Vec<(String, Option<u64>)>) {
    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(subdir) => {
                items.push((subdir.path().to_string_lossy().to_string(), None));
                collect_fingerprint_items(subdir, items);
            }
            include_dir::DirEntry::File(file) => {
                let mut hasher = DefaultHasher::new();
                file.contents().hash(&mut hasher);
                items.push((
                    file.path().to_string_lossy().to_string(),
                    Some(hasher.finish()),
                ));
            }
        }
    }
}

fn write_embedded_dir(dir: &Dir<'_>, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(subdir) => {
                fs::create_dir_all(dest.join(subdir.path()))?;
                write_embedded_dir(subdir, dest)?;
            }
            include_dir::DirEntry::File(file) => {
                let path = dest.join(file.path());
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, file.contents())?;
            }
        }
    }
    Ok(())
}

use std::path::PathBuf;
