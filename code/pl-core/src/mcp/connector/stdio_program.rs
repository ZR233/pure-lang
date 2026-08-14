use std::io;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;

/// Resolve a model-visible stdio command to a target accepted by CreateProcess.
///
/// Config stays platform-neutral (`npx`, not `npx.cmd`). On Windows, bare names
/// are resolved through PATH/PATHEXT while extensionless shell scripts such as
/// npm's POSIX `npx` shim are skipped.
#[cfg(windows)]
pub(super) fn resolve(command: &str) -> io::Result<PathBuf> {
    resolve_windows(command)
}

#[cfg(not(windows))]
pub(super) fn resolve(command: &str) -> io::Result<PathBuf> {
    Ok(PathBuf::from(command))
}

#[cfg(windows)]
fn resolve_windows(command: &str) -> io::Result<PathBuf> {
    let path = Path::new(command);
    if has_explicit_path(path) || path.extension().is_some() {
        return Ok(path.to_path_buf());
    }

    which::which_all(command)
        .map_err(io::Error::other)?
        .find(|candidate| is_create_process_target(candidate))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("stdio command was not found in PATH/PATHEXT: {command}"),
            )
        })
}

#[cfg(windows)]
fn has_explicit_path(path: &Path) -> bool {
    path.is_absolute()
        || path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty())
}

#[cfg(windows)]
fn is_create_process_target(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["com", "exe", "bat", "cmd"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn create_process_targets_exclude_extensionless_and_powershell_shims() {
        assert!(is_create_process_target(Path::new("npx.cmd")));
        assert!(is_create_process_target(Path::new("tool.EXE")));
        assert!(!is_create_process_target(Path::new("npx")));
        assert!(!is_create_process_target(Path::new("npx.ps1")));
    }

    #[cfg(windows)]
    #[test]
    fn explicit_paths_do_not_require_path_lookup() {
        assert_eq!(
            resolve(r"C:\tools\server.cmd").unwrap(),
            PathBuf::from(r"C:\tools\server.cmd")
        );
        assert_eq!(
            resolve(r".\tools\server.exe").unwrap(),
            PathBuf::from(r".\tools\server.exe")
        );
    }
}
