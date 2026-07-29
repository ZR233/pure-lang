use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

pub(crate) fn run_checked(command: &mut Command, display: &str) -> Result<()> {
    print_command_context(command, display);
    let status = command
        .status()
        .with_context(|| format!("failed to start command from PATH: {display}"))?;
    ensure_success(status, display)
}

pub(crate) fn run_checked_with_stdin(
    command: &mut Command,
    display: &str,
    input: &[u8],
) -> Result<()> {
    print_command_context(command, display);
    command.stdin(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start command from PATH: {display}"))?;
    let mut stdin = child
        .stdin
        .take()
        .with_context(|| format!("failed to open command stdin: {display}"))?;
    stdin
        .write_all(input)
        .with_context(|| format!("failed to write command stdin: {display}"))?;
    drop(stdin);
    let status = child
        .wait()
        .with_context(|| format!("failed to wait for command: {display}"))?;
    ensure_success(status, display)
}

fn print_command_context(command: &Command, display: &str) {
    let cwd = command
        .get_current_dir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    println!("==> ({}) {display}", cwd.display());
}

fn ensure_success(status: ExitStatus, display: &str) -> Result<()> {
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
    if cfg!(windows) && matches!(program, "flutter" | "dart") {
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
