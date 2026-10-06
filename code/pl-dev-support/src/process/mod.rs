//! Shared command execution for the engineering tools.
//!
//! Foreground helpers stream the child's output to the caller's terminal and
//! only summarize failures; resident helpers add background-console handling
//! and platform process-tree ownership. Arguments are always passed as
//! `OsString` values and never through a shell string.

use anyhow::{Context, bail};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

pub use anyhow::Result;

#[cfg(target_os = "linux")]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// A resident command's real termination, observed by its direct parent.
///
/// Unlike the platform launch tools, the engineering entrypoint waits on the
/// process it started itself, so [`status`](Self::status) is the genuine
/// `waitpid` status of that process rather than a launcher's exit code.
/// After the root is reaped the harness first gives the started process's own
/// supervisor a bounded window to reclaim its subtree before this harness
/// escalates, so a real product supervision failure is distinguished from one
/// this harness caused itself.
///
/// * `descendants_at_root_exit` is the descendant set still present when the
///   root was reaped (the subtree the started process was responsible for).
/// * `reclaimed_descendants` is the subset that was *still* alive after the
///   natural reclamation window and therefore had to be force-killed by this
///   harness. Empty means the started process supervised its own subtree; a
///   non-empty list means it leaked and this harness reclaimed it.
#[derive(Debug)]
pub struct ResidentExit {
    /// The direct child's OS process id, captured before it exited.
    pub pid: u32,
    /// The direct child's real exit status from `waitpid`.
    pub status: ExitStatus,
    /// Descendants still present when the root was reaped, before the natural
    /// reclamation window. Non-empty means the root had a live subtree to
    /// supervise; the product supervisor is expected to reclaim it.
    pub descendants_at_root_exit: Vec<u32>,
    /// Descendants still alive after the natural reclamation window and thus
    /// force-killed by this harness. Empty means the started process supervised
    /// its own subtree; a non-empty list means it leaked.
    pub reclaimed_descendants: Vec<u32>,
    /// Whether the resident run was interrupted by the cancellation handler.
    pub cancelled: bool,
}

/// 为需要驻留和进程树托管的子进程应用后台配置。
///
/// Windows 上设置 `CREATE_NO_WINDOW`：从非控制台环境（IDE Run 按钮、
/// 快捷方式、任务计划程序）启动驻留 GUI 时，`cmd /c flutter ...` 等控制台
/// 子进程不得弹出新的命令行窗口。同步构建等前台命令不使用本配置，确保它们
/// 继承当前终端并实时显示输出。
///
/// Interactive tools that need the caller's console (an acceptance journey
/// driving stdin, for example) must not use this configuration.
pub fn configure_background_command(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// Places the current process inside a kill-on-close Windows Job Object.
///
/// Ordinary child processes started afterwards join the job, so the whole tree is
/// terminated when this process exits for any reason (including a closed
/// console window). The job is created once per process and is deliberately
/// never released early: it is the last-resort backstop, while graceful
/// cancellation is each coordinator's own responsibility. On non-Windows
/// platforms this is a no-op and process trees are reaped per child.
/// Explicit installer/replacement handoffs may use the shared handoff factory's
/// breakaway flag; silent breakaway is never enabled for ordinary children.
///
/// # Errors
/// Returns an error when the Job Object cannot be created, configured with
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, or assigned to the current process.
pub fn own_current_process_tree() -> Result<()> {
    #[cfg(windows)]
    {
        windows::own_current_process_tree()
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

/// Runs [command] to completion with inherited stdio.
///
/// Output streams live to the caller's terminal; a failure adds a context
/// summary and the original output stays above it.
///
/// # Errors
/// Returns an error when the command cannot be started from `PATH` or exits
/// non-zero (signals are reported through the exit status text).
pub fn run_checked(command: &mut Command, display: &str) -> Result<()> {
    print_command_context(command, display);
    let status = command
        .status()
        .with_context(|| format!("failed to start command from PATH: {display}"))?;
    ensure_success(status, display)
}

/// Runs a resident [command] to completion with background-console handling.
///
/// The command's stdin is kept open (piped and held by this function) for the
/// child's whole lifetime, and on Windows the current process is first placed
/// in a kill-on-close Job Object so the resident tree cannot outlive it.
/// The caller's terminal stays attached for stdout/stderr.
/// Linux engineering entrypoints exclusively create and wait for children during this call;
/// existing children are rejected before enabling descendant adoption. Resident runs are serialized.
///
/// # Errors
/// Returns an error when the process tree cannot be owned (Windows), the
/// command cannot be started, or the wait fails; a non-zero exit is reported
/// through the shared failure summary like in [`run_checked`].
pub fn run_resident_checked(command: &mut Command, display: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        unix::run_resident(command, display)
    }
    #[cfg(not(target_os = "linux"))]
    {
        configure_background_command(command);
        print_command_context(command, display);
        own_current_process_tree()
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
}

/// Runs a resident [command] and returns its real termination without judging it.
///
/// Used by acceptance launchers that must observe the started process's own
/// exit status (and any child tree it left behind) instead of a launcher exit
/// code. The command's stdin is kept open for its whole lifetime and descendant
/// adoption is enabled so rerouting and process-group changes cannot hide a
/// leaked grandchild. A non-zero exit status is returned to the caller, never
/// turned into a harness failure here.
///
/// # Errors
/// Returns an error when descendant adoption or the command start fails, or
/// when the reaping loop fails; the returned status itself is never an error.
pub fn run_resident_reporting(command: &mut Command, display: &str) -> Result<ResidentExit> {
    #[cfg(target_os = "linux")]
    {
        unix::run_resident_reporting(command, display)
    }
    #[cfg(not(target_os = "linux"))]
    {
        configure_background_command(command);
        print_command_context(command, display);
        own_current_process_tree()
            .with_context(|| format!("failed to own resident command process tree: {display}"))?;
        command.stdin(Stdio::piped());
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start command from PATH: {display}"))?;
        let pid = child.id();
        eprintln!("resident command started: pid={pid}, command={display}");
        let status = child
            .wait()
            .with_context(|| format!("failed to wait for resident command: {display}"))?;
        eprintln!("resident command exited: pid={pid}, status={status}");
        Ok(ResidentExit {
            pid,
            status,
            descendants_at_root_exit: Vec::new(),
            reclaimed_descendants: Vec::new(),
            cancelled: false,
        })
    }
}

/// Runs [command] to completion, feeding [input] to its stdin first.
///
/// The child's stdout/stderr stay inherited; stdin is closed after the input
/// is written so the child observes EOF.
///
/// # Errors
/// Returns an error when the command cannot be started, the input cannot be
/// written, or the wait fails; a non-zero exit is reported through
/// the shared failure summary like in [`run_checked`].
pub fn run_checked_with_stdin(command: &mut Command, display: &str, input: &[u8]) -> Result<()> {
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
        bail!(
            "command failed with exit code {code}: {display}; original stdout/stderr was streamed above"
        );
    }
    Ok(())
}

/// Builds a `PATH`-resolved invocation of [program] with [args].
///
/// On Windows, `flutter` and `dart` resolve to `.bat` launchers, so they are
/// invoked through `cmd /c`; every other program is executed directly. The
/// arguments are appended as values and are never joined into a shell string.
pub fn path_command(program: &'static str, args: &[OsString]) -> Command {
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

/// Renders [program] and [args] as a single quoted display line for logs.
pub fn display_command(program: &str, args: &[OsString]) -> String {
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
