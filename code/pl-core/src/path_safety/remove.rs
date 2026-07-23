use std::path::{Path, PathBuf};

use super::{
    PathSafetyError, is_link_or_reparse, metadata_if_real, metadata_if_real_async,
    validate_existing_path, validate_existing_path_async,
};

/// Recursively removes a real directory without following child link boundaries.
///
/// The target itself and its ancestors must be real entries below `root`. A
/// linked child is unlinked using the platform-appropriate operation, while its
/// target remains untouched.
pub fn remove_dir_all_no_follow(root: &Path, target: &Path) -> Result<(), PathSafetyError> {
    validate_existing_path(root, target)?;
    let metadata = metadata_if_real(target)?.ok_or_else(|| PathSafetyError::LinkOrReparse {
        path: target.to_path_buf(),
    })?;
    if !metadata.is_dir() {
        return Err(PathSafetyError::io(
            "remove directory tree",
            target,
            std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "target is not a directory",
            ),
        ));
    }

    let mut pending = vec![TraversalStep::Enter(target.to_path_buf())];
    while let Some(step) = pending.pop() {
        match step {
            TraversalStep::Enter(path) => {
                validate_removal_entry_parent(root, target, &path)?;
                let metadata = std::fs::symlink_metadata(&path)
                    .map_err(|error| PathSafetyError::io("inspect path", &path, error))?;
                if is_link_or_reparse(&metadata) {
                    remove_link_entry(&path, &metadata)?;
                } else if metadata.is_dir() {
                    pending.push(TraversalStep::Exit(path.clone()));
                    let entries = std::fs::read_dir(&path)
                        .map_err(|error| PathSafetyError::io("read directory", &path, error))?;
                    for entry in entries {
                        let entry = entry
                            .map_err(|error| PathSafetyError::io("read directory", &path, error))?;
                        pending.push(TraversalStep::Enter(entry.path()));
                    }
                } else {
                    std::fs::remove_file(&path)
                        .map_err(|error| PathSafetyError::io("remove file", &path, error))?;
                }
            }
            TraversalStep::Exit(path) => {
                validate_existing_path(root, &path)?;
                std::fs::remove_dir(&path)
                    .map_err(|error| PathSafetyError::io("remove directory", &path, error))?;
            }
        }
    }
    Ok(())
}

/// Asynchronously removes a real directory without following child link boundaries.
pub async fn remove_dir_all_no_follow_async(
    root: &Path,
    target: &Path,
) -> Result<(), PathSafetyError> {
    validate_existing_path_async(root, target).await?;
    let metadata =
        metadata_if_real_async(target)
            .await?
            .ok_or_else(|| PathSafetyError::LinkOrReparse {
                path: target.to_path_buf(),
            })?;
    if !metadata.is_dir() {
        return Err(PathSafetyError::io(
            "remove directory tree",
            target,
            std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "target is not a directory",
            ),
        ));
    }

    let mut pending = vec![TraversalStep::Enter(target.to_path_buf())];
    while let Some(step) = pending.pop() {
        match step {
            TraversalStep::Enter(path) => {
                validate_removal_entry_parent_async(root, target, &path).await?;
                let metadata = tokio::fs::symlink_metadata(&path)
                    .await
                    .map_err(|error| PathSafetyError::io("inspect path", &path, error))?;
                if is_link_or_reparse(&metadata) {
                    remove_link_entry_async(&path, &metadata).await?;
                } else if metadata.is_dir() {
                    pending.push(TraversalStep::Exit(path.clone()));
                    let mut entries = tokio::fs::read_dir(&path)
                        .await
                        .map_err(|error| PathSafetyError::io("read directory", &path, error))?;
                    while let Some(entry) = entries
                        .next_entry()
                        .await
                        .map_err(|error| PathSafetyError::io("read directory", &path, error))?
                    {
                        pending.push(TraversalStep::Enter(entry.path()));
                    }
                } else {
                    tokio::fs::remove_file(&path)
                        .await
                        .map_err(|error| PathSafetyError::io("remove file", &path, error))?;
                }
            }
            TraversalStep::Exit(path) => {
                validate_existing_path_async(root, &path).await?;
                tokio::fs::remove_dir(&path)
                    .await
                    .map_err(|error| PathSafetyError::io("remove directory", &path, error))?;
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
enum TraversalStep {
    Enter(PathBuf),
    Exit(PathBuf),
}

fn validate_removal_entry_parent(
    root: &Path,
    target: &Path,
    path: &Path,
) -> Result<(), PathSafetyError> {
    if path == target {
        validate_existing_path(root, path)
    } else {
        let parent = path.parent().ok_or_else(|| PathSafetyError::OutsideRoot {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
        })?;
        validate_existing_path(root, parent)
    }
}

async fn validate_removal_entry_parent_async(
    root: &Path,
    target: &Path,
    path: &Path,
) -> Result<(), PathSafetyError> {
    if path == target {
        validate_existing_path_async(root, path).await
    } else {
        let parent = path.parent().ok_or_else(|| PathSafetyError::OutsideRoot {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
        })?;
        validate_existing_path_async(root, parent).await
    }
}

#[cfg(windows)]
fn remove_link_entry(path: &Path, metadata: &std::fs::Metadata) -> Result<(), PathSafetyError> {
    use std::os::windows::fs::MetadataExt;

    if metadata.file_attributes() & super::windows::FILE_ATTRIBUTE_DIRECTORY != 0 {
        std::fs::remove_dir(path)
            .map_err(|error| PathSafetyError::io("remove linked directory", path, error))
    } else {
        std::fs::remove_file(path)
            .map_err(|error| PathSafetyError::io("remove linked file", path, error))
    }
}

#[cfg(not(windows))]
fn remove_link_entry(path: &Path, _metadata: &std::fs::Metadata) -> Result<(), PathSafetyError> {
    std::fs::remove_file(path)
        .map_err(|error| PathSafetyError::io("remove symbolic link", path, error))
}

#[cfg(windows)]
async fn remove_link_entry_async(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), PathSafetyError> {
    use std::os::windows::fs::MetadataExt;

    if metadata.file_attributes() & super::windows::FILE_ATTRIBUTE_DIRECTORY != 0 {
        tokio::fs::remove_dir(path)
            .await
            .map_err(|error| PathSafetyError::io("remove linked directory", path, error))
    } else {
        tokio::fs::remove_file(path)
            .await
            .map_err(|error| PathSafetyError::io("remove linked file", path, error))
    }
}

#[cfg(not(windows))]
async fn remove_link_entry_async(
    path: &Path,
    _metadata: &std::fs::Metadata,
) -> Result<(), PathSafetyError> {
    tokio::fs::remove_file(path)
        .await
        .map_err(|error| PathSafetyError::io("remove symbolic link", path, error))
}
