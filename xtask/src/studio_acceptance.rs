//! Thin launcher for the standalone Studio GUI acceptance tool.
//!
//! `cargo xtask manual-gui <args...>` stays the canonical engineering entry,
//! but xtask does not link the acceptance tool or define a second copy of its
//! scenarios and CLI: it compiles `pl-studio-acceptance` with a normal
//! workspace cargo build (no xtask-specific `--target-dir`, so an explicit
//! `CARGO_TARGET_DIR` is honored) and forwards the arguments verbatim. Only
//! this command builds the tool; top-level `--help` and every other
//! subcommand never invoke cargo for it.
//!
//! Launch semantics per platform:
//!
//! * Unix: the launcher replaces itself with the tool via `exec`, so there is
//!   no intermediate cargo process holding locks or stealing terminal signals;
//!   the tool's own Ctrl-C handling, interactive stdin and exit status reach
//!   the caller unchanged.
//! * Windows: the tool is spawned directly (again not through `cargo run`)
//!   after the current process joins a kill-on-close Job Object, so a closed
//!   console still reaps the whole tree. A registered Ctrl-C handler keeps
//!   the launcher alive while the tool performs its graceful, evidence
//!   preserving cancellation; afterwards the tool's real exit code is
//!   propagated. The subscription is established synchronously before the
//!   child is spawned and a registration failure aborts the launch, so there
//!   is no window in which a Ctrl-C would terminate the launcher first and
//!   let the Job Object cut the tool's cleanup short.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use pl_dev_support::{paths, process};
use serde_json::Value as Json;

const ACCEPTANCE_PACKAGE: &str = "pl-studio-acceptance";

pub(crate) fn run(args: &[OsString]) -> Result<()> {
    let workspace = paths::workspace_root()?;
    let executable = build_acceptance_tool(&workspace)?;
    launch_acceptance_tool(&executable, args)
}

/// Builds the tool with readable streamed output, then reports its executable.
///
/// The second cargo invocation is a no-op build whose JSON artifact messages
/// locate the executable without recompiling, respecting `CARGO_TARGET_DIR`
/// and any configured build target.
fn build_acceptance_tool(workspace: &Path) -> Result<PathBuf> {
    let display = format!("cargo build -p {ACCEPTANCE_PACKAGE}");
    let mut build = Command::new("cargo");
    build
        .current_dir(workspace)
        .args(["build", "-p", ACCEPTANCE_PACKAGE]);
    process::run_checked(&mut build, &display)?;

    let discovery = Command::new("cargo")
        .current_dir(workspace)
        .args(["build", "-p", ACCEPTANCE_PACKAGE, "--message-format=json"])
        .output()
        .with_context(|| format!("failed to start command from PATH: {display}"))?;
    if !discovery.status.success() {
        bail!(
            "failed to locate the {ACCEPTANCE_PACKAGE} executable: {display}; stderr: {}",
            String::from_utf8_lossy(&discovery.stderr)
        );
    }
    parse_acceptance_executable(&discovery.stdout)
}

/// Extracts the tool's bin executable from cargo's JSON artifact messages.
fn parse_acceptance_executable(stdout: &[u8]) -> Result<PathBuf> {
    for line in String::from_utf8_lossy(stdout).lines() {
        let Ok(record) = serde_json::from_str::<Json>(line) else {
            continue;
        };
        let target = &record["target"];
        if record["reason"] == "compiler-artifact"
            && target["name"] == ACCEPTANCE_PACKAGE
            && target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            && let Some(executable) = record["executable"].as_str()
        {
            return Ok(PathBuf::from(executable));
        }
    }
    bail!("cargo reported no {ACCEPTANCE_PACKAGE} bin executable")
}

#[cfg(unix)]
fn launch_acceptance_tool(executable: &Path, args: &[OsString]) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(executable);
    command.args(args);
    // Replaces this process, inheriting stdin/stdout/stderr, the environment
    // and the working directory. On success `exec` never returns.
    let error = command.exec();
    Err(anyhow::Error::new(error).context(format!("failed to exec {}", executable.display())))
}

#[cfg(windows)]
fn launch_acceptance_tool(executable: &Path, args: &[OsString]) -> Result<()> {
    use std::process::Stdio;

    process::own_current_process_tree().context("failed to own acceptance tool process tree")?;
    let _ctrl_c_shield = shield_ctrl_c_while_tool_runs()?;
    let mut tool = Command::new(executable);
    tool.args(args);
    // The tool drives an interactive stdin loop, so it must inherit the
    // console instead of the resident background configuration.
    tool.stdin(Stdio::inherit());
    let mut child = tool
        .spawn()
        .with_context(|| format!("failed to start {}", executable.display()))?;
    let status = child
        .wait()
        .with_context(|| format!("failed to wait for {}", executable.display()))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(code) => std::process::exit(code),
        None => bail!("acceptance tool terminated without an exit code"),
    }
}

/// Subscription that keeps the launcher alive through a console Ctrl-C on
/// Windows.
///
/// The acceptance tool receives its own copy of the console event and handles
/// it with its graceful, evidence preserving cancellation. This subscription
/// only protects the launcher itself: tokio's console handler reports the
/// event as handled while this listener is alive, so the default termination
/// never runs and the launcher can still reap the tool and propagate its exit
/// code. The kill-on-close Job Object stays as the backstop for a closed
/// console or a crashed tool.
#[cfg(windows)]
struct CtrlCShield {
    /// The live listener is the shield; it is deliberately never polled.
    /// Registration happens synchronously when the listener is created
    /// (tokio installs its `SetConsoleCtrlHandler` routine at that moment
    /// and reports failures through the `io::Result`), and tokio's handler
    /// considers the event handled as long as this receiver is alive, with
    /// no runtime or poll required.
    _listener: tokio::signal::windows::CtrlC,
}

/// Establishes the launcher's Ctrl-C subscription before the tool is spawned.
///
/// # Errors
/// Returns the registration error when the Windows console handler cannot be
/// installed; the caller must not start the acceptance tool in that case,
/// because the launcher could no longer survive a Ctrl-C long enough to reap
/// the tool's graceful cancellation.
#[cfg(windows)]
fn shield_ctrl_c_while_tool_runs() -> Result<CtrlCShield> {
    let listener =
        tokio::signal::windows::ctrl_c().context("failed to register the Ctrl-C shield")?;
    Ok(CtrlCShield {
        _listener: listener,
    })
}
