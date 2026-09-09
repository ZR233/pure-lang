//! Account lookup and shell-specific startup syntax.

use std::ffi::{CStr, OsStr, OsString};
use std::io;
use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
use std::path::{Path, PathBuf};

use super::EnvironmentError;

#[derive(Debug)]
enum Dialect {
    Bash,
    Zsh,
    Fish,
    Sh,
}

#[derive(Debug)]
pub(super) struct UserShell {
    path: PathBuf,
    dialect: Dialect,
}

pub(super) fn detect() -> Result<UserShell, EnvironmentError> {
    let account = account_shell()?;
    let path = account
        .filter(|path| executable(path))
        .or_else(|| {
            std::env::var_os("SHELL")
                .map(PathBuf::from)
                .filter(|path| executable(path))
        })
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    UserShell::new(path)
}

impl UserShell {
    fn new(path: PathBuf) -> Result<Self, EnvironmentError> {
        let dialect = match path.file_name().and_then(OsStr::to_str) {
            Some("bash") => Dialect::Bash,
            Some("zsh") => Dialect::Zsh,
            Some("fish") => Dialect::Fish,
            Some("sh" | "dash" | "ash") => Dialect::Sh,
            _ => {
                return Err(EnvironmentError::UnsupportedShell(
                    path.display().to_string(),
                ));
            }
        };
        Ok(Self { path, dialect })
    }

    pub(super) fn collect_command(
        &self,
        executable: &Path,
        socket: &Path,
    ) -> Result<String, EnvironmentError> {
        let quote = |path: &Path| -> Result<String, EnvironmentError> {
            let value = path.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "environment bootstrap path is not UTF-8",
                )
            })?;
            Ok(match self.dialect {
                Dialect::Fish => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
                Dialect::Bash | Dialect::Zsh | Dialect::Sh => quote_posix(value),
            })
        };
        let capture = format!(
            "exec {} --emit-environment {}",
            quote(executable)?,
            quote(socket)?
        );
        let flags = match self.dialect {
            Dialect::Bash => "-ic",
            Dialect::Zsh | Dialect::Fish => "-lic",
            Dialect::Sh => "-lc",
        };
        let path = self.path.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "user shell path is not UTF-8")
        })?;
        Ok(format!(
            "exec {} {flags} {} </dev/null",
            quote_posix(path),
            quote_posix(&capture)
        ))
    }
}

fn quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn executable(path: &Path) -> bool {
    path.is_absolute()
        && std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn account_shell() -> io::Result<Option<PathBuf>> {
    let mut capacity = 4096;
    loop {
        let mut buffer = vec![0u8; capacity];
        let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        // SAFETY: getuid takes no arguments. getpwuid_r writes to the exclusive passwd,
        // result slot and live byte buffer. Only success with a non-null result permits reads.
        let status = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                entry.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE && capacity < 1024 * 1024 {
            capacity *= 2;
            continue;
        }
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status));
        }
        if result.is_null() {
            return Ok(None);
        }
        // SAFETY: successful getpwuid_r initialized entry; pw_shell refers to a NUL-terminated
        // string in buffer, which remains alive until the path bytes have been copied.
        let shell = unsafe {
            let entry = entry.assume_init();
            if entry.pw_shell.is_null() {
                return Ok(None);
            }
            CStr::from_ptr(entry.pw_shell).to_bytes().to_vec()
        };
        return Ok((!shell.is_empty()).then(|| PathBuf::from(OsString::from_vec(shell))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_selection_is_explicit_and_quoting_preserves_arguments() {
        for (name, flags) in [
            ("bash", "-ic"),
            ("zsh", "-lic"),
            ("fish", "-lic"),
            ("sh", "-lc"),
            ("dash", "-lc"),
            ("ash", "-lc"),
        ] {
            let shell = UserShell::new(PathBuf::from(format!("/bin/{name}"))).unwrap();
            let command = shell
                .collect_command(Path::new("/tmp/helper's file"), Path::new("/tmp/socket"))
                .unwrap();
            assert!(command.contains(flags));
            assert!(command.ends_with("</dev/null"));
        }
        assert!(matches!(
            UserShell::new("/bin/csh".into()),
            Err(EnvironmentError::UnsupportedShell(_))
        ));
    }
    #[test]
    fn bash_and_sh_load_their_own_startup_files() {
        startup_fixture(
            Path::new("/bin/bash"),
            ".bashrc",
            "case $- in *i*) ;; *) return;; esac\nexport PURE_SHELL_FIXTURE='configured value'\n",
        );
        startup_fixture(
            Path::new("/bin/dash"),
            ".profile",
            "export PURE_SHELL_FIXTURE='configured value'\n",
        );
    }

    #[test]
    #[ignore = "requires Zsh and Fish; PURE_SSH_SHELL_BIN may name an isolated installation"]
    fn zsh_and_fish_load_their_own_startup_files() {
        let bin = std::env::var_os("PURE_SSH_SHELL_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/bin"));
        startup_fixture(
            &bin.join("zsh"),
            ".zshrc",
            "export PURE_SHELL_FIXTURE='configured value'\n",
        );
        startup_fixture(
            &bin.join("fish"),
            ".config/fish/config.fish",
            "if status is-interactive\nset -gx PURE_SHELL_FIXTURE 'configured value'\nend\n",
        );
    }

    fn startup_fixture(shell: &Path, filename: &str, content: &str) {
        let home = tempfile::tempdir().unwrap();
        let rc = home.path().join(filename);
        std::fs::create_dir_all(rc.parent().unwrap()).unwrap();
        std::fs::write(rc, content).unwrap();
        // The stand-in emitter observes the child's environment and also exercises quoting.
        let emitter = home.path().join("emitter's file");
        std::fs::write(&emitter, "#!/bin/sh\nprintf '%s' \"$PURE_SHELL_FIXTURE\"\n").unwrap();
        std::fs::set_permissions(&emitter, std::fs::Permissions::from_mode(0o700)).unwrap();
        let script = UserShell::new(shell.into())
            .unwrap()
            .collect_command(&emitter, &home.path().join("socket"))
            .unwrap();
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .env("HOME", home.path())
            .env("ZDOTDIR", home.path())
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env_remove("PURE_SHELL_FIXTURE")
            .env_remove("BASH_ENV")
            .env_remove("ENV")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "shell startup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).ends_with("configured value"),
            "shell did not export its configuration"
        );
    }
}
