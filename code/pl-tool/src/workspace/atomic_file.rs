//! 同目录临时文件与原子替换。

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Filesystem write intent shared by tool schemas and physical file backends.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum WriteMode {
    Create,
    Overwrite,
    Append,
}

/// Writes bytes with explicit creation, replacement or append semantics.
///
/// # Errors
/// Returns path, exclusive-create, write or synchronization errors without claiming rollback.
pub fn write_file_with_mode(path: &Path, content: &[u8], mode: WriteMode) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("write target has no parent"))?;
    fs::create_dir_all(directory)?;
    let mut file = match mode {
        WriteMode::Overwrite => return atomic_write(path, content, AtomicPermissions::Workspace),
        WriteMode::Create => fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?,
        WriteMode::Append => fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?,
    };
    file.write_all(content)?;
    file.sync_all()?;
    sync_directory(directory)
}

/// 将完整内容写入同目录临时文件，并原子替换目标文件。
///
/// # Errors
///
/// 创建、写入、同步临时文件或提交替换失败时返回底层 I/O 错误。
pub fn write_file_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    atomic_write(path, content, AtomicPermissions::Private)
}

#[derive(Clone, Copy)]
enum AtomicPermissions {
    Private,
    Workspace,
}

fn atomic_write(path: &Path, content: &[u8], permissions: AtomicPermissions) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("atomic write target has no parent directory"))?;
    fs::create_dir_all(directory)?;
    let existing = match permissions {
        AtomicPermissions::Private => None,
        AtomicPermissions::Workspace => match fs::metadata(path) {
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        },
    };
    let mut builder = tempfile::Builder::new();
    builder.prefix(".pure-write-");
    #[cfg(unix)]
    if matches!(permissions, AtomicPermissions::Workspace) {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut temporary = builder.tempfile_in(directory)?;
    temporary.write_all(content)?;
    if let Some(permissions) = existing {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file().sync_all()?;
    let (file, temporary_path) = temporary.keep().map_err(|error| error.error)?;
    drop(file);
    let result = replace_file(&temporary_path, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result?;
    sync_directory(directory)
}

fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        let source = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let target = target
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: 两个路径缓冲区均以 NUL 结尾，并在调用期间保持有效。
        let replaced = unsafe {
            MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if replaced == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        fs::rename(source, target)
    }
}

fn sync_directory(directory: &Path) -> io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::File::open(directory)?.sync_all()
    }

    #[cfg(windows)]
    {
        let _ = directory;
        Ok(())
    }
}
