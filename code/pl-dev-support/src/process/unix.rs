//! Linux resident tree ownership, including descendants with independent process groups.

use std::collections::BTreeSet;
use std::io;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

static CANCELLED: AtomicBool = AtomicBool::new(false);
static SIGNALS: OnceLock<Result<(), String>> = OnceLock::new();
static RESIDENT_OWNER: Mutex<()> = Mutex::new(());

/// The engineering command is the exclusive child creator/reaper during a resident run.
/// Subreaping keeps descendants owned even if their parent exits or they change process groups.
struct ProcessTree {
    child: Child,
    root_status: Option<ExitStatus>,
    previous_subreaper: libc::c_int,
    closed: bool,
}

impl ProcessTree {
    fn start(command: &mut Command) -> Result<Self> {
        ensure!(
            children()?.is_empty(),
            "resident command requires exclusive child ownership"
        );
        let mut previous_subreaper = 0;
        // SAFETY: the output is exclusively borrowed initialized c_int storage. SET only uses
        // scalar unsigned-long arguments with the width expected by prctl's variadic ABI.
        // No child exists yet; the owner mutex serializes resident runs.
        unsafe {
            if libc::prctl(
                libc::PR_GET_CHILD_SUBREAPER,
                &mut previous_subreaper,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
            ) != 0
                || libc::prctl(
                    libc::PR_SET_CHILD_SUBREAPER,
                    1 as libc::c_ulong,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                ) != 0
            {
                return Err(io::Error::last_os_error()).context("own resident descendants");
            }
        }
        command.process_group(0).stdin(Stdio::piped());
        match command.spawn() {
            Ok(child) => Ok(Self {
                child,
                root_status: None,
                previous_subreaper,
                closed: false,
            }),
            Err(error) => {
                restore_subreaper(previous_subreaper)?;
                Err(error).context("start resident command")
            }
        }
    }

    fn reap(&mut self, signalled: &mut BTreeSet<libc::pid_t>) -> io::Result<bool> {
        // Bound each batch so cancellation/deadlines are checked even with continuous exits.
        for _ in 0..128 {
            let mut status = 0;
            // SAFETY: this command exclusively owns/waits for its children, including adopted
            // descendants. status is writable c_int storage; WNOHANG never blocks the loop.
            let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
            if pid > 0 {
                signalled.remove(&pid);
                if pid == self.child.id() as libc::pid_t
                    && self.root_status.is_none()
                    && (libc::WIFEXITED(status) || libc::WIFSIGNALED(status))
                {
                    self.root_status = Some(ExitStatus::from_raw(status));
                    self.child.stdin.take();
                }
            } else if pid == 0 {
                return Ok(false);
            } else {
                let error = io::Error::last_os_error();
                match error.raw_os_error() {
                    Some(libc::ECHILD) => return Ok(true),
                    Some(libc::EINTR) => continue,
                    _ => return Err(error),
                }
            }
        }
        Ok(false)
    }

    fn signal_children(&self, signal: libc::c_int, sent: &mut BTreeSet<libc::pid_t>) -> Result<()> {
        for pid in children()? {
            if !sent.insert(pid) {
                continue;
            }
            // SAFETY: these are direct unreaped children from our own /proc list. No other
            // waiter runs, so their positive PIDs cannot be reused before this call. Adopted
            // descendants are signalled individually regardless of process-group membership.
            if unsafe { libc::kill(pid, signal) } != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error).context("signal resident child");
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        restore_subreaper(self.previous_subreaper)?;
        self.closed = true;
        Ok(())
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        // Error/panic backstop: drain all adopted generations, never wait on a live root.
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let _ = self.signal_children(libc::SIGKILL, &mut BTreeSet::new());
            if self.reap(&mut BTreeSet::new()).unwrap_or(false) {
                let _ = self.finish();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        eprintln!("resident process-tree cleanup failed; unreaped children remain");
    }
}

pub(super) fn run_resident(command: &mut Command, display: &str) -> Result<()> {
    let _owner = RESIDENT_OWNER
        .lock()
        .map_err(|_| anyhow::anyhow!("resident owner poisoned"))?;
    SIGNALS
        .get_or_init(|| {
            ctrlc::set_handler(|| CANCELLED.store(true, Ordering::Release))
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    CANCELLED.store(false, Ordering::Release);
    super::print_command_context(command, display);
    let mut tree = ProcessTree::start(command)?;
    eprintln!(
        "resident command started: pid={}, command={display}",
        tree.child.id()
    );
    let mut closing = None;
    let mut term_sent = BTreeSet::new();
    loop {
        let empty = tree.reap(&mut term_sent)?;
        if empty {
            let status = tree.root_status.context("resident root was not reaped")?;
            tree.finish()?;
            eprintln!(
                "resident command exited: pid={}, status={status}",
                tree.child.id()
            );
            if CANCELLED.load(Ordering::Acquire) {
                bail!("resident command cancelled: {display}");
            }
            return super::ensure_success(status, display);
        }
        if closing.is_none() && (tree.root_status.is_some() || CANCELLED.load(Ordering::Acquire)) {
            closing = Some(Instant::now());
        }
        if let Some(started) = closing {
            if started.elapsed() < Duration::from_secs(3) {
                tree.signal_children(libc::SIGTERM, &mut term_sent)?;
            } else {
                tree.signal_children(libc::SIGKILL, &mut BTreeSet::new())?;
                ensure!(
                    started.elapsed() < Duration::from_secs(6),
                    "resident process-tree cleanup timed out"
                );
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn children() -> Result<Vec<libc::pid_t>> {
    let mut children = Vec::new();
    // Include the spawn thread and the process leader: Linux adopts orphaned descendants onto
    // the subreaper's thread group, so one task's children file alone is insufficient.
    for task in std::fs::read_dir("/proc/self/task")? {
        let path = task?.path().join("children");
        match std::fs::read_to_string(path) {
            Ok(content) => {
                for pid in content.split_whitespace() {
                    let pid: libc::pid_t =
                        pid.parse().context("invalid resident child identity")?;
                    ensure!(pid > 0, "invalid resident child identity");
                    children.push(pid);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(children)
}

fn restore_subreaper(previous: libc::c_int) -> Result<()> {
    // SAFETY: unsigned-long variadic arguments match prctl's ABI. Restore the captured flag.
    if unsafe {
        libc::prctl(
            libc::PR_SET_CHILD_SUBREAPER,
            previous as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("restore resident subreaper");
    }
    Ok(())
}
