//! 同目录临时文件与原子替换。

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// 将完整内容写入同目录临时文件，并原子替换目标文件。
///
/// # Errors
///
/// 创建、写入、同步临时文件或提交替换失败时返回底层 I/O 错误。
pub fn write_file_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("atomic write target has no parent directory"))?;
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".pure-write-")
        .tempfile_in(directory)?;
    temporary.write_all(content)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("state.json");
        fs::write(&target, "old").unwrap();

        write_file_atomically(&target, b"new").unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "new");
    }
}
