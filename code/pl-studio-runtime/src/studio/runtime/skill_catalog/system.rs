use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use pl_core::config::SkillsConfig;
use pl_core::path_safety::{metadata_if_real, remove_dir_all_no_follow, validate_existing_path};
use rust_embed::Embed;

const LEGACY_SYSTEM_MARKER_FILE_NAME: &str = ".pl-system-skills.marker";
const EXPECTED_SYSTEM_SKILLS: [&str; 9] = [
    "canvas-design",
    "docx",
    "frontend-design",
    "pdf",
    "powerpoint",
    "skill-creator",
    "studio-config",
    "subagent-workflow",
    "xlsx",
];

#[derive(Embed)]
#[folder = "assets/skills/"]
#[compression = "zstd"]
struct BundledSystemSkills;

#[derive(Debug, Clone, PartialEq, Eq)]
struct BundledAsset {
    path: PathBuf,
    contents: Vec<u8>,
}

pub(super) fn refresh_system_skills(system_dir: &Path, config: &SkillsConfig) -> Result<()> {
    let assets = validated_bundled_assets()?;
    replace_system_skills_dir(system_dir, &assets)?;
    if let Err(error) = clean_legacy_system_skills(system_dir, config) {
        tracing::warn!(%error, "failed to clean legacy system Skills cache");
    }
    Ok(())
}

fn validated_bundled_assets() -> Result<Vec<BundledAsset>> {
    let mut assets = Vec::new();
    let mut skill_documents = BTreeSet::new();
    for embedded_path in BundledSystemSkills::iter() {
        let path = validate_bundled_path(&embedded_path)?;
        let asset = BundledSystemSkills::get(&embedded_path)
            .with_context(|| format!("bundled Skill asset disappeared: {embedded_path}"))?;
        if is_main_skill_document(&path) {
            let skill_name = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .context("bundled Skill directory name must be valid UTF-8")?;
            let content = std::str::from_utf8(asset.data.as_ref())
                .with_context(|| format!("bundled Skill document is not UTF-8: {embedded_path}"))?;
            pl_core::skill::validate_skill_document(content, Some(skill_name))
                .with_context(|| format!("invalid bundled Skill document: {embedded_path}"))?;
            skill_documents.insert(skill_name.to_string());
        }
        assets.push(BundledAsset {
            path,
            contents: asset.data.into_owned(),
        });
    }
    assets.sort_unstable_by(|left, right| left.path.cmp(&right.path));

    let expected = EXPECTED_SYSTEM_SKILLS
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    if skill_documents != expected {
        bail!("bundled system Skills must contain exactly {expected:?}, found {skill_documents:?}");
    }
    Ok(assets)
}

fn validate_bundled_path(raw: &str) -> Result<PathBuf> {
    let path = Path::new(raw);
    let mut normalized = PathBuf::new();
    let mut components = path.components();
    let Some(Component::Normal(skill_name)) = components.next() else {
        bail!("bundled Skill path must start with a Skill directory: {raw}");
    };
    let skill_name = skill_name
        .to_str()
        .context("bundled Skill path must be valid UTF-8")?;
    if !EXPECTED_SYSTEM_SKILLS.contains(&skill_name) {
        bail!("unexpected bundled system Skill directory: {skill_name}");
    }
    normalized.push(skill_name);
    for component in components {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                bail!("bundled Skill path must stay relative and normalized: {raw}");
            }
        }
    }
    if normalized.components().count() < 2 {
        bail!("bundled Skill asset must be a file below its Skill directory: {raw}");
    }
    Ok(normalized)
}

fn is_main_skill_document(path: &Path) -> bool {
    matches!(path.components().count(), 2 | 3)
        && path.file_name().is_some_and(|name| name == "SKILL.md")
}

fn replace_system_skills_dir(system_dir: &Path, assets: &[BundledAsset]) -> Result<()> {
    let skills_dir = prepare_skills_parent(system_dir)?;
    remove_current_system_dir(&skills_dir, system_dir)?;
    let staging = tempfile::Builder::new()
        .prefix(".system-staging-")
        .tempdir_in(&skills_dir)
        .with_context(|| {
            format!(
                "failed to create system Skills staging directory in '{}'",
                skills_dir.display()
            )
        })?;
    write_assets(staging.path(), assets)?;
    fs::rename(staging.path(), system_dir).with_context(|| {
        format!(
            "failed to publish system Skills staging directory '{}' as '{}'",
            staging.path().display(),
            system_dir.display()
        )
    })?;
    Ok(())
}

fn prepare_skills_parent(system_dir: &Path) -> Result<PathBuf> {
    let skills_dir = system_dir
        .parent()
        .context("system Skills directory must have a parent")?;
    let data_dir = skills_dir
        .parent()
        .context("system Skills parent must be inside the Studio data directory")?;
    let studio_home = data_dir
        .parent()
        .context("Studio data directory must be inside Studio home")?;
    ensure_real_directory(studio_home, data_dir, "Studio data directory")?;
    ensure_real_directory(data_dir, skills_dir, "system Skills parent")?;
    Ok(skills_dir.to_path_buf())
}

fn ensure_real_directory(root: &Path, directory: &Path, label: &str) -> Result<()> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "{label} is a symbolic link or reparse point: '{}'",
                directory.display()
            );
        }
        Ok(metadata) if !metadata.is_dir() => {
            bail!("{label} is not a directory: '{}'", directory.display());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(directory)
                .with_context(|| format!("failed to create {label} '{}'", directory.display()))?;
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect {label} '{}'", directory.display()));
        }
    }
    validate_existing_path(root, directory)
        .with_context(|| format!("unsafe {label} '{}'", directory.display()))
}

fn remove_current_system_dir(skills_dir: &Path, system_dir: &Path) -> Result<()> {
    match fs::symlink_metadata(system_dir) {
        Ok(_) => remove_dir_all_no_follow(skills_dir, system_dir)
            .with_context(|| format!("failed to remove system Skills '{}'", system_dir.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect system Skills '{}'", system_dir.display())),
    }
}

fn write_assets(staging_dir: &Path, assets: &[BundledAsset]) -> Result<()> {
    for asset in assets {
        let path = staging_dir.join(&asset.path);
        let parent = path
            .parent()
            .context("bundled Skill asset must have a parent")?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create bundled Skill directory '{}'",
                parent.display()
            )
        })?;
        fs::write(&path, &asset.contents)
            .with_context(|| format!("failed to write bundled Skill asset '{}'", path.display()))?;
    }
    Ok(())
}

fn clean_legacy_system_skills(system_dir: &Path, config: &SkillsConfig) -> Result<()> {
    let user_dir = pl_core::skill::resolve_user_skills_dir(config)?;
    let legacy_dir = user_dir.join(".system");
    if same_existing_path(&legacy_dir, system_dir) || !has_legacy_marker(&legacy_dir)? {
        return Ok(());
    }
    remove_dir_all_no_follow(&user_dir, &legacy_dir).with_context(|| {
        format!(
            "failed to remove legacy system Skills cache '{}'",
            legacy_dir.display()
        )
    })
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn has_legacy_marker(legacy_dir: &Path) -> Result<bool> {
    let marker = legacy_dir.join(LEGACY_SYSTEM_MARKER_FILE_NAME);
    let Some(metadata) = metadata_if_real(&marker)
        .with_context(|| format!("failed to inspect legacy marker '{}'", marker.display()))?
    else {
        return Ok(false);
    };
    if !metadata.is_file() {
        return Ok(false);
    }
    let value = fs::read_to_string(&marker)
        .with_context(|| format!("failed to read legacy marker '{}'", marker.display()))?;
    let value = value.trim();
    Ok(
        !value.is_empty()
            && value.len() <= 16
            && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn target(root: &Path) -> PathBuf {
        root.join("studio").join("skills").join(".system")
    }

    fn materialized_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(current).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }

        let mut files = BTreeMap::new();
        visit(root, root, &mut files);
        files
    }

    #[test]
    fn embedded_bundle_contains_exactly_valid_expected_skills() {
        let assets = validated_bundled_assets().unwrap();
        let documents = assets
            .iter()
            .filter(|asset| is_main_skill_document(&asset.path))
            .map(|asset| {
                asset
                    .path
                    .parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(
            documents,
            EXPECTED_SYSTEM_SKILLS
                .into_iter()
                .map(ToOwned::to_owned)
                .collect()
        );
    }

    #[test]
    fn every_embedded_asset_is_zstd_compressed_and_round_trips() {
        for path in BundledSystemSkills::iter() {
            let compressed = BundledSystemSkills::compressed(&path).unwrap();
            let restored = BundledSystemSkills::get(&path).unwrap();

            assert_eq!(compressed.content_encoding(), "zstd");
            assert_eq!(compressed.data.decoded(), restored.data.as_ref());
        }
    }

    #[test]
    fn refresh_rebuilds_every_time_and_restores_all_embedded_bytes() {
        let home = tempfile::tempdir().unwrap();
        let system_dir = target(home.path());
        let config = SkillsConfig::default();

        refresh_system_skills(&system_dir, &config).unwrap();
        let first = materialized_files(&system_dir);
        fs::write(system_dir.join("stale.txt"), b"stale").unwrap();
        fs::remove_file(system_dir.join("skill-creator").join("SKILL.md")).unwrap();

        refresh_system_skills(&system_dir, &config).unwrap();

        assert_eq!(materialized_files(&system_dir), first);
        let expected = validated_bundled_assets()
            .unwrap()
            .into_iter()
            .map(|asset| (asset.path, asset.contents))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(first, expected);
    }

    #[test]
    fn refresh_is_independent_from_system_discovery_setting() {
        let home = tempfile::tempdir().unwrap();
        let system_dir = target(home.path());
        let mut config = SkillsConfig::default();
        config.system.enabled = false;

        refresh_system_skills(&system_dir, &config).unwrap();

        for skill in EXPECTED_SYSTEM_SKILLS {
            assert!(system_dir.join(skill).join("SKILL.md").is_file());
        }
    }

    #[test]
    fn target_file_is_rejected_without_deleting_it() {
        let home = tempfile::tempdir().unwrap();
        let system_dir = target(home.path());
        fs::create_dir_all(system_dir.parent().unwrap()).unwrap();
        fs::write(&system_dir, "owned file").unwrap();

        let error = refresh_system_skills(&system_dir, &SkillsConfig::default())
            .unwrap_err()
            .to_string();

        assert!(error.contains("failed to remove system Skills"));
        assert_eq!(fs::read_to_string(system_dir).unwrap(), "owned file");
    }

    #[cfg(unix)]
    #[test]
    fn target_symlink_is_rejected_without_touching_its_destination() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let system_dir = target(home.path());
        fs::create_dir_all(system_dir.parent().unwrap()).unwrap();
        fs::write(destination.path().join("sentinel"), "keep").unwrap();
        symlink(destination.path(), &system_dir).unwrap();

        assert!(refresh_system_skills(&system_dir, &SkillsConfig::default()).is_err());
        assert_eq!(
            fs::read_to_string(destination.path().join("sentinel")).unwrap(),
            "keep"
        );
        assert!(
            fs::symlink_metadata(system_dir)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn linked_studio_data_directory_is_rejected_before_writing_outside_home() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        symlink(destination.path(), home.path().join("studio")).unwrap();

        assert!(refresh_system_skills(&target(home.path()), &SkillsConfig::default()).is_err());
        assert!(!destination.path().join("skills").exists());
    }

    #[test]
    fn write_failure_does_not_publish_a_partial_directory() {
        let home = tempfile::tempdir().unwrap();
        let system_dir = target(home.path());
        fs::create_dir_all(&system_dir).unwrap();
        fs::write(system_dir.join("stale"), "remove").unwrap();
        let assets = vec![
            BundledAsset {
                path: PathBuf::from("conflict"),
                contents: b"file".to_vec(),
            },
            BundledAsset {
                path: PathBuf::from("conflict/child"),
                contents: b"child".to_vec(),
            },
        ];

        assert!(replace_system_skills_dir(&system_dir, &assets).is_err());
        assert!(!system_dir.exists());
        assert!(
            fs::read_dir(system_dir.parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".system-staging-"))
        );
    }

    #[test]
    fn staging_parent_failure_cannot_publish_a_target() {
        let home = tempfile::tempdir().unwrap();
        let skills_dir = home.path().join("studio").join("skills");
        fs::create_dir_all(skills_dir.parent().unwrap()).unwrap();
        fs::write(&skills_dir, "not a directory").unwrap();
        let system_dir = skills_dir.join(".system");

        assert!(refresh_system_skills(&system_dir, &SkillsConfig::default()).is_err());
        assert_eq!(fs::read_to_string(skills_dir).unwrap(), "not a directory");
        assert!(!system_dir.exists());
    }

    #[test]
    fn invalid_embedded_paths_are_rejected() {
        for path in [
            "../escape",
            "/absolute",
            "skill-creator",
            "unknown/SKILL.md",
        ] {
            assert!(
                validate_bundled_path(path).is_err(),
                "path must fail: {path}"
            );
        }
    }

    #[test]
    fn legacy_cache_is_removed_only_with_old_pure_marker() {
        let home = tempfile::tempdir().unwrap();
        let system_dir = target(home.path());
        let user_dir = home.path().join("custom-user-skills");
        let legacy_dir = user_dir.join(".system");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(legacy_dir.join("stale"), "stale").unwrap();
        let config = SkillsConfig {
            user_dir: user_dir.to_string_lossy().into_owned(),
            ..SkillsConfig::default()
        };

        clean_legacy_system_skills(&system_dir, &config).unwrap();
        assert!(legacy_dir.exists());

        fs::write(
            legacy_dir.join(LEGACY_SYSTEM_MARKER_FILE_NAME),
            "a1b2c3d4\n",
        )
        .unwrap();
        clean_legacy_system_skills(&system_dir, &config).unwrap();
        assert!(!legacy_dir.exists());
    }

    #[test]
    fn legacy_cleanup_never_removes_the_new_system_directory() {
        let home = tempfile::tempdir().unwrap();
        let user_dir = home.path().join("studio").join("skills");
        let system_dir = user_dir.join(".system");
        fs::create_dir_all(&system_dir).unwrap();
        fs::write(
            system_dir.join(LEGACY_SYSTEM_MARKER_FILE_NAME),
            "a1b2c3d4\n",
        )
        .unwrap();
        let config = SkillsConfig {
            user_dir: user_dir.to_string_lossy().into_owned(),
            ..SkillsConfig::default()
        };

        clean_legacy_system_skills(&system_dir, &config).unwrap();

        assert!(system_dir.exists());
    }
}
