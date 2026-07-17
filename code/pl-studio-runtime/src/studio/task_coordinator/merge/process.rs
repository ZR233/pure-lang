use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

const CHECK_TIMEOUT: Duration = Duration::from_secs(300);

pub(super) struct ProcessOutput {
    pub(super) success: bool,
    pub(super) combined: String,
}

pub(super) async fn run_process(
    cwd: impl AsRef<Path>,
    program: &str,
    arguments: Vec<String>,
) -> Result<ProcessOutput> {
    let cwd = cwd.as_ref().to_path_buf();
    let program = program.to_string();
    tokio::task::spawn_blocking(move || run_blocking(cwd, program, arguments))
        .await
        .context("merge verifier process task failed")?
}

fn run_blocking(cwd: PathBuf, program: String, arguments: Vec<String>) -> Result<ProcessOutput> {
    let child = Command::new(&program)
        .current_dir(cwd)
        .args(&arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start {program}"))?;
    let mut child = KillOnDropChild::new(child);
    let stdout = child
        .child
        .stdout
        .take()
        .context("process stdout is missing")?;
    let stderr = child
        .child
        .stderr
        .take()
        .context("process stderr is missing")?;
    let stdout_reader = std::thread::spawn(move || read_pipe(stdout));
    let stderr_reader = std::thread::spawn(move || read_pipe(stderr));
    let deadline = Instant::now() + CHECK_TIMEOUT;
    let status = loop {
        if let Some(status) = child.child.try_wait().context("failed to poll verifier")? {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill_and_wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            bail!("{program} timed out after {}s", CHECK_TIMEOUT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    child.completed = true;
    let stdout = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("verifier stdout reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("verifier stderr reader panicked"))??;
    let mut combined = String::from_utf8_lossy(&stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }
    Ok(ProcessOutput {
        success: status.success(),
        combined,
    })
}

fn read_pipe(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)?;
    Ok(output)
}

struct KillOnDropChild {
    child: Child,
    completed: bool,
}

impl KillOnDropChild {
    fn new(child: Child) -> Self {
        Self {
            child,
            completed: false,
        }
    }

    fn kill_and_wait(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.completed = true;
    }
}

impl Drop for KillOnDropChild {
    fn drop(&mut self) {
        if !self.completed {
            self.kill_and_wait();
        }
    }
}
