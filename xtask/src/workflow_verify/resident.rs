use crate::process;
use anyhow::{Context, Result, bail};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(super) struct ResidentProcess {
    child: Child,
    lines: Receiver<String>,
    readers: Vec<JoinHandle<Result<()>>>,
}

impl ResidentProcess {
    pub(super) fn start(
        command: &mut Command,
        stdout_path: &Path,
        stderr_path: &Path,
    ) -> Result<Self> {
        configure_process_group(command);
        process::configure_background_command(command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .context("failed to start the native GUI acceptance process")?;
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                terminate_process_tree(&mut child)?;
                bail!("GUI stdout pipe is missing");
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                terminate_process_tree(&mut child)?;
                bail!("GUI stderr pipe is missing");
            }
        };
        let (sender, lines) = mpsc::channel();
        let stdout_reader = match spawn_reader(stdout, stdout_path, false, Some(sender.clone())) {
            Ok(reader) => reader,
            Err(error) => {
                terminate_process_tree(&mut child)?;
                return Err(error);
            }
        };
        let stderr_reader = match spawn_reader(stderr, stderr_path, true, Some(sender)) {
            Ok(reader) => reader,
            Err(error) => {
                terminate_process_tree(&mut child)?;
                stdout_reader
                    .join()
                    .map_err(|_| anyhow::anyhow!("GUI log reader panicked"))??;
                return Err(error);
            }
        };
        let readers = vec![stdout_reader, stderr_reader];
        Ok(Self {
            child,
            lines,
            readers,
        })
    }

    pub(super) fn wait_for_vm_service(&mut self, timeout: Duration) -> Result<String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self
                .child
                .try_wait()
                .context("failed to poll GUI process")?
            {
                bail!("native GUI launcher exited before the VM service was ready: {status}");
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("timed out waiting for the Flutter VM service URL");
            }
            match self
                .lines
                .recv_timeout(remaining.min(Duration::from_millis(250)))
            {
                Ok(line) => {
                    if let Some(url) = vm_service_url(&line) {
                        return Ok(url);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("GUI log streams closed before the VM service URL appeared")
                }
            }
        }
    }

    pub(super) fn write_process_tree(&mut self, path: &Path) -> Result<()> {
        let pid = self.child.id();
        #[cfg(unix)]
        let report = {
            let output = Command::new("ps")
                .args(["-eo", "pid=,ppid=,pgid=,stat=,comm="])
                .output()
                .context("failed to inspect the GUI process group")?;
            let group = pid.to_string();
            let rows = String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| line.split_whitespace().nth(2) == Some(group.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            format!("rootPid={pid}\n{rows}\n")
        };
        #[cfg(windows)]
        let report = {
            let output = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV"])
                .output()
                .context("failed to inspect the GUI root process")?;
            format!("rootPid={pid}\n{}", String::from_utf8_lossy(&output.stdout))
        };
        fs::write(path, report)
            .with_context(|| format!("failed to write process tree `{}`", path.display()))
    }

    pub(super) fn stop(mut self) -> Result<()> {
        terminate_process_tree(&mut self.child)?;
        for reader in self.readers.drain(..) {
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("GUI log reader panicked"))??;
        }
        Ok(())
    }
}

pub(super) fn run_logged(
    command: &mut Command,
    display: &str,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<()> {
    run_logged_inner(command, display, stdout_path, stderr_path, None)
}

pub(super) fn run_logged_with_timeout(
    command: &mut Command,
    display: &str,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
) -> Result<()> {
    run_logged_inner(command, display, stdout_path, stderr_path, Some(timeout))
}

fn run_logged_inner(
    command: &mut Command,
    display: &str,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Option<Duration>,
) -> Result<()> {
    println!("==> {display}");
    configure_process_group(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start {display}"))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_process_tree(&mut child)?;
            bail!("command stdout pipe is missing");
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate_process_tree(&mut child)?;
            bail!("command stderr pipe is missing");
        }
    };
    let stdout_reader = match spawn_reader(stdout, stdout_path, false, None) {
        Ok(reader) => reader,
        Err(error) => {
            terminate_process_tree(&mut child)?;
            return Err(error);
        }
    };
    let stderr_reader = match spawn_reader(stderr, stderr_path, true, None) {
        Ok(reader) => reader,
        Err(error) => {
            terminate_process_tree(&mut child)?;
            stdout_reader
                .join()
                .map_err(|_| anyhow::anyhow!("command log reader panicked"))??;
            return Err(error);
        }
    };
    let readers = [stdout_reader, stderr_reader];
    let mut timed_out = false;
    let status = match timeout {
        None => Some(
            child
                .wait()
                .with_context(|| format!("failed to wait for {display}"))?,
        ),
        Some(timeout) => {
            let deadline = Instant::now() + timeout;
            loop {
                if let Some(status) = child
                    .try_wait()
                    .with_context(|| format!("failed to poll {display}"))?
                {
                    break Some(status);
                }
                if Instant::now() >= deadline {
                    timed_out = true;
                    terminate_process_tree(&mut child)?;
                    break None;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    };
    for reader in readers {
        reader
            .join()
            .map_err(|_| anyhow::anyhow!("command log reader panicked"))??;
    }
    if timed_out {
        let timeout = timeout.expect("timed_out only occurs with a configured timeout");
        bail!(
            "command exceeded its timeout of {} seconds: {display}",
            timeout.as_secs()
        );
    }
    ensure_success(
        status.expect("non-timeout command must have an exit status"),
        display,
    )
}

fn spawn_reader(
    input: impl Read + Send + 'static,
    path: &Path,
    stderr: bool,
    sender: Option<Sender<String>>,
) -> Result<JoinHandle<Result<()>>> {
    let mut file =
        File::create(path).with_context(|| format!("failed to create log `{}`", path.display()))?;
    Ok(thread::spawn(move || {
        let mut reader = BufReader::new(input);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            if reader.read_until(b'\n', &mut bytes)? == 0 {
                break;
            }
            file.write_all(&bytes)?;
            file.flush()?;
            if stderr {
                std::io::stderr().write_all(&bytes)?;
                std::io::stderr().flush()?;
            } else {
                std::io::stdout().write_all(&bytes)?;
                std::io::stdout().flush()?;
            }
            if let Some(sender) = &sender {
                let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
            }
        }
        Ok(())
    }))
}

fn ensure_success(status: ExitStatus, display: &str) -> Result<()> {
    if status.success() {
        return Ok(());
    }
    let code = status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "terminated by signal".to_owned());
    bail!("command failed with exit code {code}: {display}")
}

fn vm_service_url(line: &str) -> Option<String> {
    let lowercase = line.to_ascii_lowercase();
    let markers = [
        "the dart vm service is listening on",
        "a dart vm service on",
    ];
    if !markers.iter().any(|marker| lowercase.contains(marker)) {
        return None;
    }
    let start = line.find("https://").or_else(|| line.find("http://"))?;
    let url = line[start..]
        .split_whitespace()
        .next()?
        .trim_end_matches(['.', ',', '\r', '\n']);
    (!url.is_empty()).then(|| url.to_owned())
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_process_tree(child: &mut Child) -> Result<()> {
    let pid = child.id() as i32;
    if child.try_wait()?.is_none() {
        // SAFETY: the GUI launcher was placed in a fresh process group whose id
        // equals its pid; a negative pid targets only that owned group.
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        // SAFETY: the same owned process group is still live after the grace period.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    child.wait()?;
    Ok(())
}

#[cfg(windows)]
fn terminate_process_tree(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_none() {
        let status = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .status()
            .context("failed to invoke taskkill for the GUI process tree")?;
        if !status.success() && child.try_wait()?.is_none() {
            bail!("taskkill failed to terminate GUI process tree: {status}");
        }
    }
    child.wait()?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn extracts_both_flutter_vm_service_log_formats() {
        assert_eq!(
            vm_service_url("The Dart VM service is listening on http://127.0.0.1:1234/\n"),
            Some("http://127.0.0.1:1234/".to_owned())
        );
        assert_eq!(
            vm_service_url("A Dart VM Service on Linux is available at: http://127.0.0.1:4321/,"),
            Some("http://127.0.0.1:4321/".to_owned())
        );
    }
}
