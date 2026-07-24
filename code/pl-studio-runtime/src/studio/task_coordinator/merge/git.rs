use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::super::git::{STUDIO_GIT_EMAIL_CONFIG, STUDIO_GIT_NAME_CONFIG};

const GIT_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) struct GitCommandOutput {
    pub(super) success: bool,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

impl GitCommandOutput {
    pub(super) fn stdout_text(&self) -> Result<String> {
        String::from_utf8(self.stdout.clone()).context("git stdout is not UTF-8")
    }

    pub(super) fn stderr_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim().to_string()
    }
}

pub(super) async fn run_git(
    repository: impl AsRef<Path>,
    arguments: Vec<String>,
) -> Result<GitCommandOutput> {
    let repository = repository.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || run_git_blocking(repository, arguments))
        .await
        .context("git command task failed")?
}

pub(super) async fn checked_git(
    repository: impl AsRef<Path>,
    arguments: Vec<String>,
) -> Result<String> {
    let command = arguments.join(" ");
    let output = run_git(repository, arguments).await?;
    if !output.success {
        bail!("git {command} failed: {}", output.stderr_lossy());
    }
    output.stdout_text().map(|value| value.trim().to_string())
}

fn run_git_blocking(repository: PathBuf, arguments: Vec<String>) -> Result<GitCommandOutput> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(&repository)
        .args([
            "-c",
            STUDIO_GIT_NAME_CONFIG,
            "-c",
            STUDIO_GIT_EMAIL_CONFIG,
            "-c",
            "commit.gpgSign=false",
        ])
        .args(&arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_ASKPASS", "")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process::configure_background_std_command(&mut command);
    let child = command
        .spawn()
        .with_context(|| format!("failed to start git {}", arguments.join(" ")))?;
    let mut child = KillOnDropChild::new(child);
    let stdout = child.child.stdout.take().context("git stdout is missing")?;
    let stderr = child.child.stderr.take().context("git stderr is missing")?;
    let stdout_reader = std::thread::spawn(move || read_pipe(stdout));
    let stderr_reader = std::thread::spawn(move || read_pipe(stderr));
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        if let Some(status) = child
            .child
            .try_wait()
            .context("failed to poll git command")?
        {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill_and_wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            bail!(
                "git {} timed out after {}s",
                arguments.join(" "),
                GIT_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    child.completed = true;
    let stdout = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("git stdout reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("git stderr reader panicked"))??;
    Ok(GitCommandOutput {
        success: status.success(),
        stdout,
        stderr,
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
