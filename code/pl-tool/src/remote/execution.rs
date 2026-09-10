use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_protocol::remote::RemoteSpawnRequest;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::execution::{ExecutionBackend, ExecutionOutput, ExecutionRequest};
use crate::shell::shell_quote_word;

use super::RemoteClient;
use super::path::relative_workspace_path;

static NEXT_EXECUTION_ID: AtomicU64 = AtomicU64::new(0);

/// 在远端 workspace 中执行宿主传入的 Git/worktree 命令。
#[derive(Debug, Clone)]
pub struct RemoteExecutionBackend {
    client: RemoteClient,
    workspace_id: String,
    canonical_root: String,
}

impl RemoteExecutionBackend {
    pub(crate) fn new(client: RemoteClient, workspace_id: String, canonical_root: String) -> Self {
        Self {
            client,
            workspace_id,
            canonical_root,
        }
    }
}

impl ExecutionBackend for RemoteExecutionBackend {
    type Error = String;

    async fn run(
        &self,
        request: ExecutionRequest,
    ) -> std::result::Result<ExecutionOutput, Self::Error> {
        let sequence = NEXT_EXECUTION_ID
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let process_id = format!("execution-{}-{sequence}", std::process::id());
        let cwd = remote_cwd(&request.cwd, &self.canonical_root)?;
        let command = std::iter::once(request.program.to_string_lossy().into_owned())
            .chain(request.args)
            .map(|part| shell_quote_word(&part))
            .collect::<Vec<_>>()
            .join(" ");
        let capture_path = format!(".pure/remote/execution/{process_id}.log");
        let transport = self
            .client
            .spawn_process(RemoteSpawnRequest {
                process_id: process_id.clone(),
                workspace_id: self.workspace_id.clone(),
                command,
                cwd,
                environment: request.env,
                capture_path,
            })
            .await
            .map_err(|error| error.to_string())?;
        drop(transport.stdin);
        let wait = collect_process_output(transport.stdout, transport.stderr, transport.exit);
        tokio::pin!(wait);
        let result = match request.timeout {
            Some(timeout) => match tokio::time::timeout(timeout, &mut wait).await {
                Ok(result) => result,
                Err(_) => {
                    let termination = self.client.terminate_process(&process_id).await;
                    let settled = wait.await;
                    return Err(match (termination, settled) {
                        (Ok(()), Ok(_)) => {
                            "remote command timed out; process exit and output drain completed"
                                .to_string()
                        }
                        (Err(error), Ok(_)) => format!(
                            "remote command timed out; termination request failed ({error}), but process exit and output drain subsequently completed"
                        ),
                        (Ok(()), Err(error)) => format!(
                            "remote command timed out; exit or output drain failed: {error}"
                        ),
                        (Err(termination), Err(settlement)) => format!(
                            "remote command timed out; termination failed: {termination}; exit or output drain failed: {settlement}"
                        ),
                    });
                }
            },
            None => wait.await,
        }?;
        Ok(result)
    }
}

async fn collect_process_output(
    stdout: impl AsyncRead + Unpin,
    stderr: impl AsyncRead + Unpin,
    exit: tokio::sync::oneshot::Receiver<
        Result<pl_protocol::remote::RemoteProcessExit, super::RemoteClientError>,
    >,
) -> Result<ExecutionOutput, String> {
    let (stdout, stderr, exit) = tokio::join!(read_all(stdout), read_all(stderr), exit);
    let exit = exit
        .map_err(|_| "remote process exit channel closed".to_string())?
        .map_err(|error| error.to_string())?;
    Ok(ExecutionOutput {
        status: exit.exit_code.unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout?).into_owned(),
        stderr: String::from_utf8_lossy(&stderr?).into_owned(),
    })
}

async fn read_all(mut reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| format!("failed to read remote process output: {error}"))?;
    Ok(bytes)
}

fn remote_cwd(cwd: &Path, canonical_root: &str) -> Result<String, String> {
    relative_workspace_path(canonical_root, cwd).map_err(|()| {
        format!(
            "remote command cwd '{}' escapes workspace '{canonical_root}'",
            cwd.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_accepts_root_and_rejects_escape() {
        let root = "/srv/project";
        assert_eq!(remote_cwd(Path::new(root), root).expect("root"), ".");
        assert_eq!(
            remote_cwd(Path::new("/srv/project/src"), root).expect("child"),
            "src"
        );
        assert!(remote_cwd(Path::new("/srv/other"), root).is_err());
    }
}
