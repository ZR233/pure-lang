#[cfg(not(target_os = "linux"))]
use tokio::process::Command;

use crate::environment::{ExecutionEnvironment, ShellDialect};

const POWERSHELL_UTF8_OUTPUT_PREFIX: &str =
    "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;\n";

#[cfg(not(target_os = "linux"))]
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
