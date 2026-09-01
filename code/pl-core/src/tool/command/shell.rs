use tokio::process::Command;

use crate::execution_environment::{ExecutionEnvironment, ShellDialect};

const POWERSHELL_UTF8_OUTPUT_PREFIX: &str =
    "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;\n";

pub(crate) fn command_for_environment(environment: &ExecutionEnvironment, script: &str) -> Command {
    let argv = argv_for_environment(environment, script);
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command
}

pub(crate) fn argv_for_environment(
    environment: &ExecutionEnvironment,
    script: &str,
) -> Vec<String> {
    let path = environment.shell_path.to_string_lossy().into_owned();
    match environment.shell {
        ShellDialect::Bash | ShellDialect::Sh => {
            vec![path, "-c".to_string(), script.to_string()]
        }
        ShellDialect::Pwsh | ShellDialect::PowerShell => vec![
            path,
            "-NoProfile".to_string(),
            "-Command".to_string(),
            powershell_script(script),
        ],
        ShellDialect::Cmd => vec![path, "/C".to_string(), script.to_string()],
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_environment::{ExecutionOs, ExecutionTransport};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn environment(shell: ShellDialect, path: &str) -> ExecutionEnvironment {
        ExecutionEnvironment {
            transport: ExecutionTransport::Local,
            os: ExecutionOs::Linux,
            shell,
            shell_path: PathBuf::from(path),
        }
    }

    #[test]
    fn posix_shells_use_dash_c() {
        for (dialect, path) in [
            (ShellDialect::Bash, "/bin/bash"),
            (ShellDialect::Sh, "/bin/sh"),
        ] {
            assert_eq!(
                argv_for_environment(&environment(dialect, path), "echo hello"),
                vec![path, "-c", "echo hello"]
            );
        }
    }

    #[test]
    fn powershell_argv_uses_no_profile_command_and_utf8_prefix() {
        let argv = argv_for_environment(
            &environment(ShellDialect::Pwsh, "pwsh.exe"),
            "Write-Output '你好'",
        );
        assert_eq!(argv[0], "pwsh.exe");
        assert_eq!(argv[1], "-NoProfile");
        assert_eq!(argv[2], "-Command");
        assert!(argv[3].starts_with(POWERSHELL_UTF8_OUTPUT_PREFIX));
    }

    #[test]
    fn powershell_argv_does_not_duplicate_utf8_prefix() {
        let script = format!("{POWERSHELL_UTF8_OUTPUT_PREFIX}Write-Output 'ok'");
        let argv = argv_for_environment(
            &environment(ShellDialect::PowerShell, "powershell.exe"),
            &script,
        );
        assert_eq!(argv[3], script);
    }

    #[test]
    fn cmd_fallback_argv_uses_cmd_c() {
        let argv = argv_for_environment(&environment(ShellDialect::Cmd, "cmd.exe"), "echo hello");
        assert_eq!(argv, vec!["cmd.exe", "/C", "echo hello"]);
    }
}
