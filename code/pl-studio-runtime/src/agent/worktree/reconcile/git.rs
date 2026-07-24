use super::*;

pub(super) fn remove_registration(repository: &Path, path: &Path) -> Result<()> {
    let path = path.to_string_lossy().to_string();
    git_status(repository, &["worktree", "remove", "--force", &path])
}

pub(super) fn delete_branch(repository: &Path, branch: &str) -> Result<()> {
    if !is_pure_branch(branch) {
        bail!("refusing to delete non-Pure worktree branch {branch}");
    }
    git_status(repository, &["branch", "-D", branch])
}

pub(super) fn git_output(repository: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repository)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process::configure_background_std_command(&mut command);
    let child = command
        .spawn()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    let mut child = KillOnDropChild::new(child);
    let stdout = child
        .child
        .stdout
        .take()
        .context("git stdout pipe is missing")?;
    let stderr = child
        .child
        .stderr
        .take()
        .context("git stderr pipe is missing")?;
    let stdout_reader = std::thread::spawn(move || read_pipe(stdout));
    let stderr_reader = std::thread::spawn(move || read_pipe(stderr));
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        if let Some(status) = child
            .child
            .try_wait()
            .context("failed to poll git process")?
        {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill_and_wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            bail!(
                "git {} timed out after {}s",
                args.join(" "),
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
    if !status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&stdout).to_string())
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

fn git_status(repository: &Path, args: &[&str]) -> Result<()> {
    let _ = git_output(repository, args)?;
    Ok(())
}
