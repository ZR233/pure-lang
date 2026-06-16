use std::path::PathBuf;
use std::sync::OnceLock;

#[cfg(windows)]
use std::path::Path;
use tokio::process::Command;

#[cfg(windows)]
const POWERSHELL_UTF8_OUTPUT_PREFIX: &str =
    "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellKind {
    #[cfg(windows)]
    PowerShell,
    #[cfg(windows)]
    Cmd,
    #[cfg(not(windows))]
    Sh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedShell {
    kind: ShellKind,
    path: PathBuf,
}

pub(crate) fn shell_command(command: &str) -> Command {
    let shell = resolve_default_shell();
    command_for_shell(&shell, command)
}

fn command_for_shell(shell: &ResolvedShell, script: &str) -> Command {
    let argv = argv_for_shell(shell, script);
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command
}

fn argv_for_shell(shell: &ResolvedShell, script: &str) -> Vec<String> {
    match shell.kind {
        #[cfg(windows)]
        ShellKind::PowerShell => vec![
            shell.path.display().to_string(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            powershell_script(script),
        ],
        #[cfg(windows)]
        ShellKind::Cmd => vec![
            shell.path.display().to_string(),
            "/C".to_string(),
            script.to_string(),
        ],
        #[cfg(not(windows))]
        ShellKind::Sh => vec![
            shell.path.display().to_string(),
            "-c".to_string(),
            script.to_string(),
        ],
    }
}

#[cfg(windows)]
fn powershell_script(script: &str) -> String {
    if script
        .trim_start()
        .starts_with(POWERSHELL_UTF8_OUTPUT_PREFIX)
    {
        script.to_string()
    } else {
        format!("{POWERSHELL_UTF8_OUTPUT_PREFIX}{script}")
    }
}

fn resolve_default_shell() -> ResolvedShell {
    static SHELL: OnceLock<ResolvedShell> = OnceLock::new();
    SHELL.get_or_init(resolve_default_shell_uncached).clone()
}

#[cfg(windows)]
fn resolve_default_shell_uncached() -> ResolvedShell {
    if let Some(path) = try_find_pwsh_executable() {
        return ResolvedShell {
            kind: ShellKind::PowerShell,
            path,
        };
    }
    if let Some(path) = try_find_powershell_executable() {
        return ResolvedShell {
            kind: ShellKind::PowerShell,
            path,
        };
    }
    ResolvedShell {
        kind: ShellKind::Cmd,
        path: PathBuf::from("cmd.exe"),
    }
}

#[cfg(windows)]
fn try_find_pwsh_executable() -> Option<PathBuf> {
    if let Some(ps_home) = command_output("cmd")
        .args(["/C", "pwsh", "-NoProfile", "-Command", "$PSHOME"])
        .output()
        .ok()
        .and_then(|out| {
            if !out.status.success() {
                return None;
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            let trimmed = stdout.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
    {
        let candidate = PathBuf::from(ps_home).join("pwsh.exe");
        if is_powershellish_executable_available(&candidate) {
            return Some(candidate);
        }
    }

    find_powershellish_executable_in_path(&["pwsh.exe"]).or_else(|| {
        find_existing_powershellish_fallback(&[r"C:\Program Files\PowerShell\7\pwsh.exe"])
    })
}

#[cfg(windows)]
fn try_find_powershell_executable() -> Option<PathBuf> {
    find_powershellish_executable_in_path(&["powershell.exe"]).or_else(|| {
        find_existing_powershellish_fallback(&[
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
        ])
    })
}

#[cfg(windows)]
fn find_powershellish_executable_in_path(candidates: &[&str]) -> Option<PathBuf> {
    for candidate in candidates {
        let Ok(path) = which::which(candidate) else {
            continue;
        };
        if is_powershellish_executable_available(&path) {
            return Some(path);
        }
    }
    None
}

#[cfg(windows)]
fn find_existing_powershellish_fallback(paths: &[&str]) -> Option<PathBuf> {
    for path in paths {
        let candidate = PathBuf::from(*path);
        if candidate.exists() && is_powershellish_executable_available(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(windows)]
fn is_powershellish_executable_available(path: &Path) -> bool {
    command_output(path)
        .args(["-NoLogo", "-NoProfile", "-Command", "Write-Output ok"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn command_output(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    crate::process::configure_background_std_command(&mut command);
    command
}

#[cfg(not(windows))]
fn resolve_default_shell_uncached() -> ResolvedShell {
    ResolvedShell {
        kind: ShellKind::Sh,
        path: PathBuf::from("sh"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[cfg(windows)]
    #[test]
    fn powershell_argv_uses_no_profile_command_and_utf8_prefix() {
        let shell = ResolvedShell {
            kind: ShellKind::PowerShell,
            path: PathBuf::from("pwsh.exe"),
        };

        let argv = argv_for_shell(&shell, "Write-Output '你好'");

        assert_eq!(argv[0], "pwsh.exe");
        assert_eq!(argv[1], "-NoProfile");
        assert_eq!(argv[2], "-Command");
        assert!(argv[3].starts_with(POWERSHELL_UTF8_OUTPUT_PREFIX));
        assert!(argv[3].contains("Write-Output '你好'"));
    }

    #[cfg(windows)]
    #[test]
    fn powershell_argv_does_not_duplicate_utf8_prefix() {
        let shell = ResolvedShell {
            kind: ShellKind::PowerShell,
            path: PathBuf::from("pwsh.exe"),
        };
        let script = format!("{POWERSHELL_UTF8_OUTPUT_PREFIX}Write-Output 'ok'");

        let argv = argv_for_shell(&shell, &script);

        assert_eq!(argv[3], script);
    }

    #[cfg(windows)]
    #[test]
    fn cmd_fallback_argv_uses_cmd_c() {
        let shell = ResolvedShell {
            kind: ShellKind::Cmd,
            path: PathBuf::from("cmd.exe"),
        };

        let argv = argv_for_shell(&shell, "echo hello");

        assert_eq!(
            argv,
            vec![
                "cmd.exe".to_string(),
                "/C".to_string(),
                "echo hello".to_string()
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn sh_argv_uses_sh_c() {
        let shell = ResolvedShell {
            kind: ShellKind::Sh,
            path: PathBuf::from("sh"),
        };

        let argv = argv_for_shell(&shell, "echo hello");

        assert_eq!(
            argv,
            vec!["sh".to_string(), "-c".to_string(), "echo hello".to_string()]
        );
    }
}
