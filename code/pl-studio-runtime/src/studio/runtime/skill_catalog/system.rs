use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use pl_tool::skill::SkillsConfig;
use pl_tool::workspace::path_safety::{
    metadata_if_real, remove_dir_all_no_follow, validate_existing_path,
};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};

const BUNDLE_FINGERPRINT: &str =
    include_str!(concat!(env!("OUT_DIR"), "/system_skills_fingerprint"));

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
    prepare_skills_parent(system_dir)?;
    if cache_matches(system_dir)? {
        tracing::info!(cache_hit = true, "system Skills prepared");
        return Ok(());
    }
    tracing::info!(cache_hit = false, "system Skills prepared");
    let assets = validated_bundled_assets()?;
    replace_system_skills_dir(system_dir, &assets)?;
    write_cache_manifest(system_dir)?;
    if let Err(error) = clean_legacy_system_skills(system_dir, config) {
        tracing::warn!(%error, "failed to clean legacy system Skills cache");
    }
    Ok(())
}

// This derived cache is only an optimization. Bundle identity comes from the
// build input; a missing or changed file inventory conservatively rebuilds it.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct CacheManifest {
    version: u32,
    bundle: String,
    files: BTreeMap<PathBuf, FileStamp>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct FileStamp {
    length: u64,
    modified: std::time::SystemTime,
}

fn manifest_path(root: &Path) -> Result<PathBuf> {
    Ok(root
        .parent()
        .context("system Skills parent")?
        .join(".system-cache.json"))
}

fn file_inventory(root: &Path) -> Result<BTreeMap<PathBuf, FileStamp>> {
    fn collect(
        root: &Path,
        directory: &Path,
        files: &mut BTreeMap<PathBuf, FileStamp>,
    ) -> Result<()> {
        validate_existing_path(root, directory)?;
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            validate_existing_path(root, &path)?;
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                collect(root, &path, files)?;
            } else if metadata.is_file() {
                files.insert(
                    path.strip_prefix(root)?.to_path_buf(),
                    FileStamp {
                        length: metadata.len(),
                        modified: metadata.modified()?,
                    },
                );
            } else {
                bail!("unexpected system Skill filesystem entry");
            }
        }
        Ok(())
    }
    validate_existing_path(root.parent().context("system Skills parent")?, root)?;
    let mut files = BTreeMap::new();
    collect(root, root, &mut files)?;
    Ok(files)
}

fn cache_matches(root: &Path) -> Result<bool> {
    let path = manifest_path(root)?;
    if metadata_if_real(&path)?.is_none() || metadata_if_real(root)?.is_none() {
        return Ok(false);
    }
    let Ok(manifest) = serde_json::from_slice::<CacheManifest>(&fs::read(path)?) else {
        return Ok(false);
    };
    Ok(manifest.version == 1
        && manifest.bundle == BUNDLE_FINGERPRINT
        && manifest.files == file_inventory(root)?)
}

fn write_cache_manifest(root: &Path) -> Result<()> {
    use std::io::Write;
    let parent = root.parent().context("system Skills parent")?;
    let path = manifest_path(root)?;
    // Validate even on a cache miss before replacing a stale cache file.
    if fs::symlink_metadata(&path).is_ok() {
        validate_existing_path(parent, &path)?;
    }
    let manifest = CacheManifest {
        version: 1,
        bundle: BUNDLE_FINGERPRINT.to_owned(),
        files: file_inventory(root)?,
    };
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&serde_json::to_vec(&manifest)?)?;
    temporary.persist(path)?;
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
            pl_tool::skill::validate_skill_document(content, Some(skill_name))
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
    // Prepare the complete candidate before touching the currently usable bundle.
    let previous = tempfile::Builder::new()
        .prefix(".system-previous-")
        .tempdir_in(&skills_dir)?;
    let previous_path = previous.path().join("bundle");
    let had_previous = fs::symlink_metadata(system_dir).is_ok();
    if had_previous {
        validate_existing_path(&skills_dir, system_dir)?;
        anyhow::ensure!(
            system_dir.is_dir(),
            "failed to remove system Skills: target is not a directory"
        );
        fs::rename(system_dir, &previous_path)?;
    }
    if let Err(error) = fs::rename(staging.path(), system_dir) {
        if had_previous {
            fs::rename(&previous_path, system_dir).context("restore previous system Skills")?;
        }
        return Err(error).context("publish system Skills");
    }
    if had_previous {
        remove_current_system_dir(previous.path(), &previous_path)?;
    }
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
    let user_dir = pl_tool::skill::resolve_user_skills_dir(config)?;
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
