//! Runtime facts describing where and how Pure executes shell commands.

use std::path::{Path, PathBuf};

/// Whether commands execute on this process or through an SSH helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionTransport {
    Local,
    Ssh,
}

impl ExecutionTransport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Ssh => "ssh",
        }
    }
}

/// Operating-system family of the workspace that executes commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOs {
    Windows,
    Linux,
    Macos,
    Other(String),
}

impl ExecutionOs {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Other(value) => value.as_str(),
        }
    }

    pub fn is_windows(&self) -> bool {
        matches!(self, Self::Windows)
    }
}

/// The shell dialect used to start a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellDialect {
    Bash,
    Sh,
    Pwsh,
    PowerShell,
    Cmd,
}

impl ShellDialect {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Sh => "sh",
            Self::Pwsh => "pwsh",
            Self::PowerShell => "powershell",
            Self::Cmd => "cmd",
        }
    }
}

/// Verified runtime execution facts shared by command execution and prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionEnvironment {
    pub transport: ExecutionTransport,
    pub os: ExecutionOs,
    pub shell: ShellDialect,
    pub shell_path: PathBuf,
}

impl ExecutionEnvironment {
    /// Resolve the shell Pure will use for commands launched by this process.
    pub fn detect_local() -> Self {
        let os = local_os();
        let (shell, shell_path) = resolve_local_shell(&os);
        Self {
            transport: ExecutionTransport::Local,
            os,
            shell,
            shell_path,
        }
    }

    pub fn for_ssh(os: ExecutionOs, shell: ShellDialect, shell_path: impl Into<PathBuf>) -> Self {
        Self {
            transport: ExecutionTransport::Ssh,
            os,
            shell,
            shell_path: shell_path.into(),
        }
    }

    pub fn shell_path_display(&self) -> String {
        self.shell_path.to_string_lossy().into_owned()
    }
}

/// Resolve a local shell without consulting login-shell environment variables.
pub fn resolve_local_shell(os: &ExecutionOs) -> (ShellDialect, PathBuf) {
    if os.is_windows() {
        resolve_windows_shell()
    } else {
        resolve_unix_shell()
    }
}

fn local_os() -> ExecutionOs {
    match std::env::consts::OS {
        "windows" => ExecutionOs::Windows,
        "linux" => ExecutionOs::Linux,
        "macos" => ExecutionOs::Macos,
        value => ExecutionOs::Other(value.to_string()),
    }
}

fn resolve_unix_shell() -> (ShellDialect, PathBuf) {
    let bash = [Path::new("/bin/bash")]
        .into_iter()
        .find(|candidate| is_executable(candidate))
        .map(Path::to_path_buf)
        .or_else(|| which::which("bash").ok());
    let sh = [Path::new("/bin/sh")]
        .into_iter()
        .find(|candidate| is_executable(candidate))
        .map(Path::to_path_buf)
        .or_else(|| which::which("sh").ok());
    select_unix_shell(bash, sh)
}

fn select_unix_shell(bash: Option<PathBuf>, sh: Option<PathBuf>) -> (ShellDialect, PathBuf) {
    if let Some(path) = bash {
        return (ShellDialect::Bash, path);
    }
    if let Some(path) = sh {
        return (ShellDialect::Sh, path);
    }
    // Keep command construction deterministic on unusual minimal systems. The
    // path is still surfaced so a spawn failure identifies the attempted shell.
    (ShellDialect::Sh, PathBuf::from("sh"))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(windows)]
fn resolve_windows_shell() -> (ShellDialect, PathBuf) {
    select_windows_shell(
        find_powershell_executable("pwsh.exe", &[r"C:\Program Files\PowerShell\7\pwsh.exe"]),
        find_powershell_executable(
            "powershell.exe",
            &[r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"],
        ),
    )
}

#[cfg(not(windows))]
fn resolve_windows_shell() -> (ShellDialect, PathBuf) {
    select_windows_shell(None, None)
}

fn select_windows_shell(
    pwsh: Option<PathBuf>,
    powershell: Option<PathBuf>,
) -> (ShellDialect, PathBuf) {
    if let Some(path) = pwsh {
        return (ShellDialect::Pwsh, path);
    }
    if let Some(path) = powershell {
        return (ShellDialect::PowerShell, path);
    }
    (ShellDialect::Cmd, PathBuf::from("cmd.exe"))
}

#[cfg(windows)]
fn find_powershell_executable(name: &str, fallbacks: &[&str]) -> Option<PathBuf> {
    let candidates = which::which(name)
        .ok()
        .into_iter()
        .chain(fallbacks.iter().map(PathBuf::from));
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let mut command = std::process::Command::new(&path);
        command.args(["-NoLogo", "-NoProfile", "-Command", "Write-Output ok"]);
        crate::process::configure_background_std_command(&mut command);
        if command
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_environment_has_explicit_facts() {
        let environment = ExecutionEnvironment::detect_local();
        assert_eq!(environment.transport, ExecutionTransport::Local);
        assert!(!environment.os.as_str().is_empty());
        assert!(!environment.shell.as_str().is_empty());
        assert!(!environment.shell_path.as_os_str().is_empty());
    }

    #[test]
    fn shell_dialects_are_stable() {
        assert_eq!(ShellDialect::Bash.as_str(), "bash");
        assert_eq!(ShellDialect::PowerShell.as_str(), "powershell");
    }

    #[test]
    fn unix_resolver_prefers_bash_and_falls_back_to_sh() {
        assert_eq!(
            select_unix_shell(
                Some(PathBuf::from("/custom/bash")),
                Some(PathBuf::from("/bin/sh"))
            ),
            (ShellDialect::Bash, PathBuf::from("/custom/bash"))
        );
        assert_eq!(
            select_unix_shell(None, Some(PathBuf::from("/custom/sh"))),
            (ShellDialect::Sh, PathBuf::from("/custom/sh"))
        );
    }

    #[test]
    fn windows_resolver_preserves_pwsh_powershell_cmd_precedence() {
        assert_eq!(
            select_windows_shell(
                Some(PathBuf::from("pwsh.exe")),
                Some(PathBuf::from("powershell.exe"))
            ),
            (ShellDialect::Pwsh, PathBuf::from("pwsh.exe"))
        );
        assert_eq!(
            select_windows_shell(None, Some(PathBuf::from("powershell.exe"))),
            (ShellDialect::PowerShell, PathBuf::from("powershell.exe"))
        );
        assert_eq!(
            select_windows_shell(None, None),
            (ShellDialect::Cmd, PathBuf::from("cmd.exe"))
        );
    }
}
