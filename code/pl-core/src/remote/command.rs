use std::collections::BTreeMap;
use std::path::Path;

use pl_protocol::remote::RemoteSpawnRequest;
use pl_protocol::{PureError, Result};
use serde_json::Value;

use crate::tool::{
    CommandBackend, CommandCaptureStream, CommandExit, CommandIo, CommandOutputSizes,
    CommandOutputTarget, CommandSpawnRequest, ManagedCommand, command_output_model_path,
};

use super::RemoteClient;

#[derive(Debug, Clone)]
pub struct RemoteCommandBackend {
    client: RemoteClient,
    workspace_id: String,
}

impl RemoteCommandBackend {
    pub(crate) fn new(client: RemoteClient, workspace_id: String) -> Self {
        Self {
            client,
            workspace_id,
        }
    }
}

impl CommandBackend for RemoteCommandBackend {
    type Error = PureError;

    async fn resolve_cwd(
        &self,
        cwd: Option<&Path>,
        _allow_workspace_escape: bool,
    ) -> Result<String> {
        normalize_cwd(cwd)
    }

    async fn output_target(
        &self,
        session_id: &str,
        tool_id: &str,
        _call_id: &str,
        _command: &str,
    ) -> Result<CommandOutputTarget> {
        let model_file = command_output_model_path(session_id, tool_id);
        Ok(CommandOutputTarget::new(model_file.clone(), model_file))
    }

    async fn spawn(&self, request: CommandSpawnRequest) -> Result<ManagedCommand> {
        let process_id = request.process_id.clone();
        let client = self.client.clone();
        let transport = self
            .client
            .spawn_process(RemoteSpawnRequest {
                process_id: request.process_id,
                workspace_id: self.workspace_id.clone(),
                command: request.command,
                cwd: request.cwd,
                environment: BTreeMap::new(),
                capture_path: path_to_posix(request.output_target.capture_file())?,
            })
            .await
            .map_err(remote_exec_error)?;
        let mut exit = transport.exit;
        Ok(ManagedCommand::new(
            None,
            CommandIo {
                stdin: Some(Box::pin(transport.stdin)),
                stdout: Some(Box::pin(transport.stdout)),
                stderr: Some(Box::pin(transport.stderr)),
            },
            move |cancellation| async move {
                let result = tokio::select! {
                    result = &mut exit => result,
                    _ = cancellation.cancelled() => {
                        // The exit receiver remains owned even if the cancellation RPC fails.
                        let _ = client.terminate_process(&process_id).await;
                        exit.await
                    }
                };
                result
                    .map_err(|_| "remote process exit channel closed".to_string())?
                    .map(|exit| CommandExit {
                        exit_code: exit.exit_code,
                    })
                    .map_err(|error| error.to_string())
            },
        ))
    }

    async fn prepare_output(
        &self,
        _target: &CommandOutputTarget,
        _command: &str,
        _working_directory: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn append_output_chunk(
        &self,
        _target: &CommandOutputTarget,
        _stream: CommandCaptureStream,
        _chunk: &[u8],
    ) -> Result<()> {
        // The helper writes the full capture next to the remote workspace while streaming bytes.
        Ok(())
    }

    async fn publish_output(&self, _target: &CommandOutputTarget) -> Result<()> {
        Ok(())
    }

    async fn collect_output_artifacts(
        &self,
        _target: &CommandOutputTarget,
        _sizes: CommandOutputSizes,
    ) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }
}

fn normalize_cwd(cwd: Option<&Path>) -> Result<String> {
    let Some(cwd) = cwd else {
        return Ok(".".to_string());
    };
    path_to_posix(cwd)
}

fn path_to_posix(path: &Path) -> Result<String> {
    let value = path.to_string_lossy().replace('\\', "/");
    if value.starts_with('/') {
        return Err(remote_exec_error(
            "exec.cwd must be workspace-relative for SSH; use \".\" for the workspace root",
        ));
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(remote_exec_error("remote command path escapes workspace")),
            value => parts.push(value),
        }
    }
    Ok(if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    })
}

fn remote_exec_error(error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "exec".to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_cwd_is_posix_and_confined() {
        assert_eq!(normalize_cwd(None).expect("default cwd"), ".");
        assert_eq!(normalize_cwd(Some(Path::new("."))).expect("root cwd"), ".");
        assert_eq!(
            normalize_cwd(Some(Path::new("src/bin"))).expect("cwd"),
            "src/bin"
        );
        let absolute = normalize_cwd(Some(Path::new("/home/runner/project")))
            .unwrap_err()
            .to_string();
        assert!(absolute.contains(
            "exec.cwd must be workspace-relative for SSH; use \".\" for the workspace root"
        ));
        let parent = normalize_cwd(Some(Path::new("../outside")))
            .unwrap_err()
            .to_string();
        assert!(parent.contains("remote command path escapes workspace"));
    }
}
