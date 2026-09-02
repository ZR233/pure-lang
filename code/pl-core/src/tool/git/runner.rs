//! [`GitTool`] 的通用执行管道:请求构造、凭据注入、输出脱敏与结果封装。

use std::collections::BTreeMap;
use std::time::Duration;

use pl_protocol::PureError;

use super::GitTool;
use super::credential::{
    GIT_TOKEN_ENV, GitCredential, GitCredentialOperation, GitCredentialProvider,
    GitCredentialRequest, write_askpass_script,
};
use super::execution::{ExecutionBackend, ExecutionRequest};
use crate::tool::tool_error;

const GIT_TIMEOUT: Duration = Duration::from_secs(600);

impl<B, P> GitTool<B, P>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
{
    pub(super) async fn run_plain<S>(&self, args: Vec<S>) -> Result<GitToolOutcome, PureError>
    where
        S: AsRef<str>,
    {
        let request = self.execution_request(args, BTreeMap::new());
        self.run_request(request, None).await
    }

    pub(super) async fn run_with_credential<S>(
        &self,
        args: Vec<S>,
        operation: GitCredentialOperation,
        remote: String,
    ) -> Result<GitToolOutcome, PureError>
    where
        S: AsRef<str>,
    {
        if self.config.native_credentials {
            return self.run_plain(args).await;
        }
        let credential = self
            .credential_provider
            .credential(GitCredentialRequest { operation, remote })
            .await
            .map_err(|error| tool_error(self.name(), error))?
            .ok_or_else(|| {
                tool_error(self.name(), "project git account token is not configured")
            })?;
        let askpass_path = write_askpass_script(self.name()).await?;
        let mut env = BTreeMap::new();
        env.insert("GIT_TERMINAL_PROMPT".to_string(), "0".to_string());
        env.insert(
            "GIT_ASKPASS".to_string(),
            askpass_path.display().to_string(),
        );
        env.insert(GIT_TOKEN_ENV.to_string(), credential.expose().to_string());
        let request = self.execution_request(args, env);
        let result = self.run_request(request, Some(&credential)).await;
        let _ = tokio::fs::remove_file(askpass_path).await;
        result
    }

    fn execution_request<S>(&self, args: Vec<S>, env: BTreeMap<String, String>) -> ExecutionRequest
    where
        S: AsRef<str>,
    {
        ExecutionRequest {
            program: self.config.git_binary.clone(),
            args: args
                .into_iter()
                .map(|arg| arg.as_ref().to_string())
                .collect(),
            cwd: self.config.worktree.clone(),
            env,
            timeout: Some(GIT_TIMEOUT),
        }
    }

    async fn run_request(
        &self,
        request: ExecutionRequest,
        credential: Option<&GitCredential>,
    ) -> Result<GitToolOutcome, PureError> {
        let output = self
            .backend
            .run(request)
            .await
            .map_err(|error| tool_error(self.name(), error))?;
        let stdout = redact(output.stdout, credential);
        let stderr = redact(output.stderr, credential);
        if output.status == 0 {
            return GitToolOutcome::command(self.name(), output.status, stdout, stderr);
        }
        let combined = format!("{stderr}\n{stdout}");
        Err(tool_error(
            self.name(),
            format!("git command failed: {}", combined.trim()),
        ))
    }
}

pub(super) struct GitToolOutcome {
    pub(super) description: String,
    pub(super) exit_code: Option<i32>,
    pub(super) stdout: String,
}

impl GitToolOutcome {
    fn command(tool: &str, status: i32, stdout: String, stderr: String) -> Result<Self, PureError> {
        let description = json_description(
            tool,
            serde_json::json!({
                "status": status,
                "stdout": stdout,
                "stderr": stderr,
            }),
        )?;
        Ok(Self {
            description,
            exit_code: Some(status),
            stdout,
        })
    }

    pub(super) fn json(
        tool: &str,
        value: serde_json::Value,
        exit_code: Option<i32>,
    ) -> Result<Self, PureError> {
        Ok(Self {
            description: json_description(tool, value)?,
            exit_code,
            stdout: String::new(),
        })
    }
}

fn json_description(tool: &str, value: serde_json::Value) -> Result<String, PureError> {
    serde_json::to_string(&value)
        .map_err(|error| tool_error(tool, format!("failed to serialize git output: {error}")))
}

fn redact(value: String, credential: Option<&GitCredential>) -> String {
    match credential {
        Some(credential) => value.replace(credential.expose(), "[redacted]"),
        None => value,
    }
}
