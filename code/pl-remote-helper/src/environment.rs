//! Connection-scoped user environment collection; never mutates the host environment.

mod shell;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::{ffi::OsStringExt, net::UnixStream, process::CommandExt};
use std::path::Path;
use std::time::Duration;

use pl_remote_helper::client::{
    ManagedWorker, ProcessCommand, ProcessTermination, WorkerClientError,
};
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;
use tokio::task::JoinSet;

const MAX_ENVIRONMENT_BYTES: u64 = 1024 * 1024;
const COLLECTION_TIMEOUT: Duration = Duration::from_secs(15);
type EncodedEnvironment = Vec<(Vec<u8>, Vec<u8>)>;
type Environment = BTreeMap<OsString, OsString>;

#[derive(Debug, thiserror::Error)]
pub(super) enum EnvironmentError {
    #[error("SSH user environment IO: {0}")]
    Io(#[from] io::Error),
    #[error("SSH user environment supervision: {0}")]
    Worker(#[from] WorkerClientError),
    #[error("SSH user environment shell is unsupported: {0}")]
    UnsupportedShell(String),
    #[error("SSH user environment initialization timed out")]
    Timeout,
    #[error("SSH user environment shell exited before successful collection")]
    ShellExit,
    #[error("SSH user environment snapshot is invalid or too large")]
    InvalidSnapshot,
}

pub(super) fn bootstrap() -> Result<(), EnvironmentError> {
    // Resolve NSS before constructing Tokio: the lookup may perform blocking IO.
    let shell = shell::detect()?;
    let executable = std::env::current_exe()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let environment = runtime.block_on(collect(&executable, &shell))?;
    drop(runtime);
    // Re-exec preserves SSH stdio and PID without set_var in a multithreaded process.
    Err(std::process::Command::new(executable)
        .arg("--serve")
        .env_clear()
        .envs(environment)
        .exec()
        .into())
}

pub(super) fn emit(socket: &Path) -> Result<(), EnvironmentError> {
    let environment: EncodedEnvironment = std::env::vars_os()
        // Bash exports functions through special environment entries; only variables belong
        // to the connection snapshot, not executable shell definitions.
        .filter(|(key, _)| !key.as_encoded_bytes().starts_with(b"BASH_FUNC_"))
        .map(|(key, value)| (key.into_vec(), value.into_vec()))
        .collect();
    let bytes = serde_json::to_vec(&environment).map_err(|_| EnvironmentError::InvalidSnapshot)?;
    if bytes.len() as u64 > MAX_ENVIRONMENT_BYTES {
        return Err(EnvironmentError::InvalidSnapshot);
    }
    let mut socket = UnixStream::connect(socket)?;
    socket.write_all(&bytes)?;
    Ok(())
}

async fn collect(
    executable: &Path,
    shell: &shell::UserShell,
) -> Result<Environment, EnvironmentError> {
    // tempfile's private directory prevents other UIDs from injecting a snapshot.
    let directory = tempfile::Builder::new()
        .prefix("pl-ssh-env-")
        .tempdir_in("/tmp")?;
    let socket = directory.path().join("environment.sock");
    let listener = UnixListener::bind(&socket)?;
    let script = shell.collect_command(executable, &socket)?;
    let command = ProcessCommand::new("/bin/sh").args(["-c", &script]);
    collect_command(executable, command, listener, COLLECTION_TIMEOUT).await
}

async fn collect_command(
    executable: &Path,
    command: ProcessCommand,
    listener: UnixListener,
    timeout: Duration,
) -> Result<Environment, EnvironmentError> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut worker = ManagedWorker::spawn(executable, command).await?;
    drop(worker.take_stdin());
    let mut drains = JoinSet::new();
    if let Some(mut stdout) = worker.take_stdout() {
        drains.spawn(async move { tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await });
    }
    if let Some(mut stderr) = worker.take_stderr() {
        drains.spawn(async move { tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await });
    }
    let mut waited = false;
    let result = tokio::time::timeout_at(deadline, async {
        let receive = receive(&listener);
        tokio::pin!(receive);
        tokio::select! {
            environment = &mut receive => {
                let environment = environment?;
                let exit = worker.wait().await;
                waited = true;
                require_success(exit?)?;
                Ok(environment)
            }
            exit = worker.wait() => {
                waited = true;
                require_success(exit?)?;
                receive.await
            }
        }
    })
    .await
    .unwrap_or(Err(EnvironmentError::Timeout));
    // Even failed or timed-out initialization cannot leave startup descendants behind.
    worker.control().close();
    if !waited {
        worker.wait().await?;
    }
    while let Some(result) = drains.join_next().await {
        result.map_err(io::Error::other)??;
    }
    result
}

fn require_success(exit: ProcessTermination) -> Result<(), EnvironmentError> {
    match exit {
        ProcessTermination::ExitCode(0) => Ok(()),
        ProcessTermination::ExitCode(_) | ProcessTermination::Signal(_) => {
            Err(EnvironmentError::ShellExit)
        }
    }
}

async fn receive(listener: &UnixListener) -> Result<Environment, EnvironmentError> {
    let (socket, _) = listener.accept().await?;
    let mut bytes = Vec::new();
    socket
        .take(MAX_ENVIRONMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 > MAX_ENVIRONMENT_BYTES {
        return Err(EnvironmentError::InvalidSnapshot);
    }
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> Result<Environment, EnvironmentError> {
    let entries: EncodedEnvironment =
        serde_json::from_slice(bytes).map_err(|_| EnvironmentError::InvalidSnapshot)?;
    let mut environment = Environment::new();
    for (key, value) in entries {
        if key.is_empty()
            || key.contains(&b'=')
            || key.contains(&0)
            || value.contains(&0)
            || environment
                .insert(OsString::from_vec(key), OsString::from_vec(value))
                .is_some()
        {
            return Err(EnvironmentError::InvalidSnapshot);
        }
    }
    if environment.is_empty() {
        return Err(EnvironmentError::InvalidSnapshot);
    }
    Ok(environment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_preserves_non_utf8_and_rejects_invalid_entries_without_values_in_errors() {
        let encoded = vec![(b"KEY".to_vec(), vec![255, b'\n', b'='])];
        let environment = decode(&serde_json::to_vec(&encoded).unwrap()).unwrap();
        assert_eq!(
            environment[&OsString::from("KEY")],
            OsString::from_vec(vec![255, b'\n', b'='])
        );
        for invalid in [
            br#"[[[65],[0]]]"#.as_slice(),
            br#"[[[],[65]]]"#,
            br#"[[[65],[66]],[[65],[67]]]"#,
            b"[]",
            b"secret-value",
        ] {
            assert!(matches!(
                decode(invalid),
                Err(EnvironmentError::InvalidSnapshot)
            ));
        }
    }
}
