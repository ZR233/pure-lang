use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

pub(crate) fn run_checked(command: &mut Command, display: &str) -> Result<()> {
    let cwd = command
        .get_current_dir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    println!("==> ({}) {display}", cwd.display());
    let status = command
        .status()
        .with_context(|| format!("failed to start command from PATH: {display}"))?;
    if !status.success() {
        let code = status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated by signal".to_owned());
        bail!("command failed with exit code {code}: {display}");
    }
    Ok(())
}

pub(crate) fn path_command(program: &'static str, args: &[OsString]) -> Command {
    if cfg!(windows) && program == "flutter" {
        let mut command = Command::new("cmd");
        command.arg("/c").arg(program);
        command.args(args);
        command
    } else {
        let mut command = Command::new(program);
        command.args(args);
        command
    }
}

pub(crate) fn display_command(program: &str, args: &[OsString]) -> String {
    std::iter::once(OsStr::new(program))
        .chain(args.iter().map(OsString::as_os_str))
        .map(display_arg)
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_arg(arg: &OsStr) -> String {
    let value = arg.to_string_lossy();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.into_owned()
    }
}
