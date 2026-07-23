//! Host filesystem path safety primitives.
//!
//! The workspace root chosen by the host is trusted. Existing descendants are
//! inspected without following links so Unix symbolic links and every Windows
//! reparse point remain explicit filesystem boundaries.

use std::fmt;
use std::path::{Component, Path, PathBuf};

mod remove;
pub use remove::{remove_dir_all_no_follow, remove_dir_all_no_follow_async};

/// A failure while validating or safely traversing a host filesystem path.
#[derive(Debug)]
pub enum PathSafetyError {
    /// The candidate is not lexically contained by the trusted root.
    OutsideRoot { root: PathBuf, path: PathBuf },
    /// An existing path component is a symbolic link or Windows reparse point.
    LinkOrReparse { path: PathBuf },
    /// A filesystem operation failed for a specific path.
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

impl PathSafetyError {
    pub(super) fn io(operation: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl fmt::Display for PathSafetyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideRoot { root, path } => write!(
                formatter,
                "path '{}' is outside trusted root '{}'",
                path.display(),
                root.display()
            ),
            Self::LinkOrReparse { path } => write!(
                formatter,
                "path contains a symbolic link or Windows reparse point: '{}'",
                path.display()
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "failed to {operation} '{}': {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PathSafetyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::OutsideRoot { .. } | Self::LinkOrReparse { .. } => None,
        }
    }
}

/// Returns whether metadata obtained with `symlink_metadata` represents a link boundary.
#[cfg(windows)]
pub fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_type().is_symlink()
        || metadata.file_attributes() & windows::FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Returns whether metadata obtained with `symlink_metadata` represents a link boundary.
#[cfg(not(windows))]
pub fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Inspects one path entry without following it.
///
/// Missing entries and link boundaries return `None`; other inspection errors
/// remain explicit.
pub fn metadata_if_real(path: &Path) -> Result<Option<std::fs::Metadata>, PathSafetyError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PathSafetyError::io("inspect path", path, error)),
    };
    Ok((!is_link_or_reparse(&metadata)).then_some(metadata))
}

/// Asynchronously inspects one path entry without following it.
pub async fn metadata_if_real_async(
    path: &Path,
) -> Result<Option<std::fs::Metadata>, PathSafetyError> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PathSafetyError::io("inspect path", path, error)),
    };
    Ok((!is_link_or_reparse(&metadata)).then_some(metadata))
}

/// Returns the real children of a directory, omitting link and reparse entries.
pub fn real_directory_entries(directory: &Path) -> Result<Vec<PathBuf>, PathSafetyError> {
    require_real_directory(directory)?;
    let entries = std::fs::read_dir(directory)
        .map_err(|error| PathSafetyError::io("read directory", directory, error))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| PathSafetyError::io("read directory", directory, error))?;
        let path = entry.path();
        match metadata_if_real(&path) {
            Ok(Some(_)) => paths.push(path),
            Ok(None) => {}
            Err(PathSafetyError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(paths)
}

/// Asynchronously returns real directory children, omitting link and reparse entries.
pub async fn real_directory_entries_async(
    directory: &Path,
) -> Result<Vec<PathBuf>, PathSafetyError> {
    require_real_directory_async(directory).await?;
    let mut entries = tokio::fs::read_dir(directory)
        .await
        .map_err(|error| PathSafetyError::io("read directory", directory, error))?;
    let mut paths = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| PathSafetyError::io("read directory", directory, error))?
    {
        let path = entry.path();
        match metadata_if_real_async(&path).await {
            Ok(Some(_)) => paths.push(path),
            Ok(None) => {}
            Err(PathSafetyError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(paths)
}

/// Returns whether `candidate` is lexically contained by `root`.
pub fn is_lexically_within(root: &Path, candidate: &Path) -> bool {
    descendant_components(root, candidate).is_ok()
}

/// Validates that an existing candidate and all descendants below `root` are real entries.
pub fn validate_existing_path(root: &Path, candidate: &Path) -> Result<(), PathSafetyError> {
    validate_path(root, candidate, ExistingPathRequirement::TargetMustExist)
}

/// Validates a write target while permitting a missing suffix below its nearest real ancestor.
pub fn validate_path_for_write(root: &Path, candidate: &Path) -> Result<(), PathSafetyError> {
    validate_path(root, candidate, ExistingPathRequirement::AllowMissingSuffix)
}

/// Asynchronously validates an existing candidate and its descendants below `root`.
pub async fn validate_existing_path_async(
    root: &Path,
    candidate: &Path,
) -> Result<(), PathSafetyError> {
    validate_path_async(root, candidate, ExistingPathRequirement::TargetMustExist).await
}

/// Asynchronously validates a write target while permitting a missing suffix.
pub async fn validate_path_for_write_async(
    root: &Path,
    candidate: &Path,
) -> Result<(), PathSafetyError> {
    validate_path_async(root, candidate, ExistingPathRequirement::AllowMissingSuffix).await
}

#[derive(Debug, Clone, Copy)]
enum ExistingPathRequirement {
    TargetMustExist,
    AllowMissingSuffix,
}

fn require_real_directory(directory: &Path) -> Result<(), PathSafetyError> {
    let metadata = std::fs::symlink_metadata(directory)
        .map_err(|error| PathSafetyError::io("inspect directory", directory, error))?;
    validate_directory_metadata(directory, &metadata)
}

async fn require_real_directory_async(directory: &Path) -> Result<(), PathSafetyError> {
    let metadata = tokio::fs::symlink_metadata(directory)
        .await
        .map_err(|error| PathSafetyError::io("inspect directory", directory, error))?;
    validate_directory_metadata(directory, &metadata)
}

fn validate_directory_metadata(
    directory: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), PathSafetyError> {
    if is_link_or_reparse(metadata) {
        return Err(PathSafetyError::LinkOrReparse {
            path: directory.to_path_buf(),
        });
    }
    if !metadata.is_dir() {
        return Err(PathSafetyError::io(
            "read directory",
            directory,
            std::io::Error::new(std::io::ErrorKind::NotADirectory, "path is not a directory"),
        ));
    }
    Ok(())
}

fn validate_path(
    root: &Path,
    candidate: &Path,
    requirement: ExistingPathRequirement,
) -> Result<(), PathSafetyError> {
    let components = descendant_components(root, candidate)?;
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_or_reparse(&metadata) => {
                return Err(PathSafetyError::LinkOrReparse { path: current });
            }
            Ok(_) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && matches!(requirement, ExistingPathRequirement::AllowMissingSuffix) =>
            {
                return Ok(());
            }
            Err(error) => {
                return Err(PathSafetyError::io("inspect path", &current, error));
            }
        }
        if index + 1 == components.len() {
            return Ok(());
        }
    }

    if matches!(requirement, ExistingPathRequirement::TargetMustExist) && !candidate.exists() {
        return Err(PathSafetyError::io(
            "inspect path",
            candidate,
            std::io::Error::new(std::io::ErrorKind::NotFound, "path does not exist"),
        ));
    }
    Ok(())
}

async fn validate_path_async(
    root: &Path,
    candidate: &Path,
    requirement: ExistingPathRequirement,
) -> Result<(), PathSafetyError> {
    let components = descendant_components(root, candidate)?;
    let target_is_root = components.is_empty();
    let mut current = root.to_path_buf();
    for component in components {
        current.push(component);
        match tokio::fs::symlink_metadata(&current).await {
            Ok(metadata) if is_link_or_reparse(&metadata) => {
                return Err(PathSafetyError::LinkOrReparse { path: current });
            }
            Ok(_) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && matches!(requirement, ExistingPathRequirement::AllowMissingSuffix) =>
            {
                return Ok(());
            }
            Err(error) => {
                return Err(PathSafetyError::io("inspect path", &current, error));
            }
        }
    }
    if target_is_root && matches!(requirement, ExistingPathRequirement::TargetMustExist) {
        tokio::fs::metadata(candidate)
            .await
            .map_err(|error| PathSafetyError::io("inspect path", candidate, error))?;
    }
    Ok(())
}

fn descendant_components(root: &Path, candidate: &Path) -> Result<Vec<PathBuf>, PathSafetyError> {
    let comparison_root = lexical_comparison_path(root);
    let comparison_candidate = lexical_comparison_path(candidate);
    let relative = comparison_candidate
        .strip_prefix(&comparison_root)
        .map_err(|_| PathSafetyError::OutsideRoot {
            root: root.to_path_buf(),
            path: candidate.to_path_buf(),
        })?;
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => components.push(PathBuf::from(part)),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PathSafetyError::OutsideRoot {
                    root: root.to_path_buf(),
                    path: candidate.to_path_buf(),
                });
            }
        }
    }
    Ok(components)
}

#[cfg(not(windows))]
fn lexical_comparison_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(windows)]
fn lexical_comparison_path(path: &Path) -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    const VERBATIM_PREFIX: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const VERBATIM_UNC_PREFIX: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    const UNC_PREFIX: &[u16] = &[b'\\' as u16, b'\\' as u16];

    let encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let normalized = if let Some(suffix) = encoded.strip_prefix(VERBATIM_UNC_PREFIX) {
        UNC_PREFIX
            .iter()
            .copied()
            .chain(suffix.iter().copied())
            .collect()
    } else if let Some(suffix) = encoded.strip_prefix(VERBATIM_PREFIX) {
        suffix.to_vec()
    } else {
        return path.to_path_buf();
    };
    PathBuf::from(OsString::from_wide(&normalized))
}

#[cfg(windows)]
mod windows {
    pub(super) const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    pub(super) const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pl-path-safety-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn write_validation_allows_missing_suffix() {
        let root = temp_dir("missing");
        std::fs::create_dir_all(&root).unwrap();

        validate_path_for_write(&root, &root.join("new/child.txt")).unwrap();

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recursive_remove_preserves_link_target() {
        let root = temp_dir("remove-root");
        let outside = temp_dir("remove-outside");
        std::fs::create_dir_all(root.join("tree")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("kept.txt"), "kept").unwrap();
        create_directory_link(&outside, &root.join("tree/linked")).unwrap();

        remove_dir_all_no_follow(&root, &root.join("tree")).unwrap();

        assert!(!root.join("tree").exists());
        assert_eq!(
            std::fs::read_to_string(outside.join("kept.txt")).unwrap(),
            "kept"
        );
        let _ = std::fs::remove_dir_all(root);
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn existing_validation_rejects_link_ancestor() {
        let root = temp_dir("ancestor-root");
        let outside = temp_dir("ancestor-outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        create_directory_link(&outside, &root.join("linked")).unwrap();

        let error = validate_existing_path(&root, &root.join("linked/secret.txt")).unwrap_err();

        assert!(matches!(error, PathSafetyError::LinkOrReparse { .. }));
        let _ = std::fs::remove_dir_all(root);
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn directory_enumeration_omits_link_entries() {
        let root = temp_dir("entries-root");
        let outside = temp_dir("entries-outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("real.txt"), "real").unwrap();
        create_directory_link(&outside, &root.join("linked")).unwrap();

        let entries = real_directory_entries(&root).unwrap();

        assert_eq!(entries, vec![root.join("real.txt")]);
        let _ = std::fs::remove_dir_all(root);
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_junction_is_a_reparse_boundary() {
        let root = temp_dir("junction-root");
        let outside = temp_dir("junction-outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        let junction = root.join("junction");
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let metadata = std::fs::symlink_metadata(&junction).unwrap();
        assert!(is_link_or_reparse(&metadata));
        assert!(matches!(
            validate_existing_path(&root, &junction.join("secret.txt")),
            Err(PathSafetyError::LinkOrReparse { .. })
        ));

        std::fs::remove_dir(&junction).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_verbatim_root_accepts_equivalent_drive_path() {
        let root = temp_dir("verbatim-root");
        let child = root.join("child");
        std::fs::create_dir_all(&child).unwrap();
        let canonical_root = std::fs::canonicalize(&root).unwrap();

        validate_existing_path(&canonical_root, &child).unwrap();
        validate_path_for_write(&canonical_root, &root.join("missing/file.txt")).unwrap();

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }
}
