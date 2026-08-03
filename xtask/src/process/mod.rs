use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

#[cfg(windows)]
mod windows;

pub(crate) fn run_checked(command: &mut Command, display: &str) -> Result<()> {
    print_command_context(command, display);
    let status = command
        .status()
        .with_context(|| format!("failed to start command from PATH: {display}"))?;
    ensure_success(status, display)
}

pub(crate) fn run_resident_checked(command: &mut Command, display: &str) -> Result<()> {
    print_command_context(command, display);
    #[cfg(windows)]
    windows::own_current_process_tree()
        .with_context(|| format!("failed to own resident command process tree: {display}"))?;

    command.stdin(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start command from PATH: {display}"))?;
    eprintln!(
        "resident command started: pid={}, command={display}",
        child.id()
    );
    let control = child
        .stdin
        .take()
        .with_context(|| format!("failed to keep resident command stdin open: {display}"))?;
    let status = child
        .wait()
        .with_context(|| format!("failed to wait for resident command: {display}"))?;
    drop(control);
    eprintln!(
        "resident command exited: pid={}, status={status}",
        child.id()
    );
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn resident_command_keeps_control_pipe_open_while_child_runs() {
        let mut command =
            Command::new(std::env::current_exe().expect("test executable must be available"));
        command.args([
            "--exact",
            "process::tests::resident_child_probe",
            "--nocapture",
        ]);
        command.env("PURE_XTASK_RESIDENT_CHILD_PROBE", "1");

        run_resident_checked(&mut command, "xtask resident stdin probe")
            .expect("resident command must not observe stdin EOF");
    }

    #[test]
    fn resident_child_probe() {
        if std::env::var_os("PURE_XTASK_RESIDENT_CHILD_PROBE").is_none() {
            return;
        }

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut input = String::new();
            let result = std::io::stdin().read_line(&mut input);
            let _ = sender.send(result);
        });

        match receiver.recv_timeout(Duration::from_millis(250)) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("stdin probe disconnected before reporting a result")
            }
            Ok(Ok(0)) => panic!("resident child observed stdin EOF"),
            Ok(Ok(_)) => panic!("resident child received unexpected stdin input"),
            Ok(Err(error)) => panic!("resident child failed to read stdin: {error}"),
        }
    }
}
