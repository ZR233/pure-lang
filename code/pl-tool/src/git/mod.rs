//! git 工具:workspace 配置与 [`GitTool`] 适配器。
//!
//! 按域拆分:`schema` 承载工具类型与输入 schema,`policy` 承载安全策略,
//! `credential` 承载凭据注入,执行后端由宿主显式提供,
//! `commands` 承载各 git 子命令语义,`runner` 承载带凭据注入与脱敏的执行管道。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use pl_protocol::PureError;
use serde_json::Value;

use crate::deserialize_tool_input;

mod commands;
mod credential;
mod policy;
mod runner;
mod schema;

use crate::execution::ExecutionBackend;
#[cfg(test)]
use crate::execution::{ExecutionOutput, ExecutionRequest};
pub use credential::*;

pub use policy::GitPolicy;
pub use schema::*;

/// git 工具运行配置。
#[derive(Debug, Clone, PartialEq)]
pub struct GitWorkspaceConfig {
    pub worktree: PathBuf,
    pub git_binary: PathBuf,
    pub policy: GitPolicy,
    /// 允许执行宿主直接使用其原生 Git 凭据链，而不注入本地 provider token。
    pub native_credentials: bool,
    pub default_push_branch: Option<String>,
    pub remote_url: Option<String>,
    pub workspace_info: BTreeMap<String, Value>,
}

impl GitWorkspaceConfig {
    pub fn local(worktree: impl Into<PathBuf>) -> Self {
        Self {
            worktree: worktree.into(),
            git_binary: PathBuf::from("git"),
            policy: GitPolicy::default(),
            native_credentials: false,
            default_push_branch: None,
            remote_url: None,
            workspace_info: BTreeMap::new(),
        }
    }

    pub fn with_native_credentials(mut self) -> Self {
        self.native_credentials = true;
        self
    }
}

/// 单个 git tool 适配器。
#[derive(Debug)]
pub struct GitTool<B, P> {
    kind: GitToolKind,
    config: GitWorkspaceConfig,
    backend: Arc<B>,
    credential_provider: Arc<P>,
}

impl<B, P> GitTool<B, P> {
    pub fn new(
        kind: GitToolKind,
        config: GitWorkspaceConfig,
        backend: Arc<B>,
        credential_provider: Arc<P>,
    ) -> Self {
        Self {
            kind,
            config,
            backend,
            credential_provider,
        }
    }

    fn name(&self) -> &str {
        self.kind.name()
    }
}

impl<B: ExecutionBackend + 'static, P: GitCredentialProvider + 'static> GitTool<B, P> {
    async fn invoke(&self, input: Value) -> Result<runner::GitToolOutcome, PureError> {
        match self.kind {
            GitToolKind::Status => {
                deserialize_tool_input::<GitEmptyInput>(self.name(), input)?;
                self.run_plain(vec!["status", "--short", "--branch"]).await
            }
            GitToolKind::Diff => self.run_diff(input).await,
            GitToolKind::Branch => self.run_branch(input).await,
            GitToolKind::Fetch => self.run_fetch(input).await,
            GitToolKind::Commit => self.run_commit(input).await,
            GitToolKind::Push => self.run_push(input).await,
            GitToolKind::WorkspaceInfo => {
                deserialize_tool_input::<GitEmptyInput>(self.name(), input)?;
                self.workspace_info()
            }
            GitToolKind::SyncDefaultBranch => self.run_sync_default_branch(input).await,
        }
    }

    /// Transfers a Git executor under the host's explicit repository/credential policy identity.
    ///
    /// # Errors
    /// Returns invalid tool registration errors.
    pub fn registration(
        self,
        declaration: pl_core::context::OpaquePayload,
        authorization: pl_core::tool::opaque::ToolAuthorization,
    ) -> Result<pl_core::tool::opaque::Registration, pl_core::tool::opaque::RegistryError> {
        Ok(
            pl_core::tool::opaque::Registration::new(self.kind.name().into(), declaration, self)?
                .with_authorization(authorization),
        )
    }
}

impl<B: ExecutionBackend + 'static, P: GitCredentialProvider + 'static> pl_core::tool::opaque::Tool
    for GitTool<B, P>
{
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        context: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        use pl_core::tool::opaque::ToolError;
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(crate::tool_error(
                self.name(),
                "unsupported Git argument encoding",
            )));
        }
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let input = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        match self.invoke(input).await {
            Ok(outcome) => dynamic_output(&outcome),
            Err(source) => {
                let observed = runner::observed_failure(&source)
                    .map(dynamic_output)
                    .transpose()?;
                let error = ToolError::new(source);
                Err(match observed {
                    Some(output) => error.with_output(output),
                    None => error,
                })
            }
        }
    }
}

fn dynamic_output(
    outcome: &runner::GitToolOutcome,
) -> Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
    use pl_core::{
        context::{ContextContent, OpaquePayload},
        tool::opaque::ToolError,
    };
    let preview = pl_output::bounded_text(&outcome.description, 12 * 1024, 0);
    let context = if preview.truncated {
        format!(
            "{}\n[Git preview omitted {} bytes; full result retained in history.]",
            preview.text, preview.bytes_omitted
        )
    } else {
        preview.text
    };
    Ok(pl_core::tool::ToolOutput::new(
        OpaquePayload::new("pl.tool.git", 1, outcome.description.clone())
            .map_err(ToolError::new)?,
        vec![ContextContent::Text {
            text: Arc::from(context),
        }],
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fmt;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use pl_protocol::PureError;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::test_support::{ToolTestExt, input};
    use pl_core::tool::opaque::CallContext;

    #[derive(Debug, Clone)]
    struct DisplayGitError(&'static str);

    impl fmt::Display for DisplayGitError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    #[derive(Debug, Default)]
    struct RecordingBackend {
        requests: Mutex<Vec<ExecutionRequest>>,
    }

    impl ExecutionBackend for RecordingBackend {
        type Error = DisplayGitError;

        async fn run(
            &self,
            request: ExecutionRequest,
        ) -> std::result::Result<ExecutionOutput, Self::Error> {
            self.requests.lock().unwrap().push(request);
            Ok(ExecutionOutput {
                status: 0,
                stdout: "secret-token fetched".to_string(),
                stderr: String::new(),
            })
        }
    }

    #[derive(Debug)]
    struct ScriptedBackend {
        requests: Mutex<Vec<ExecutionRequest>>,
        outputs: Mutex<Vec<ExecutionOutput>>,
    }

    impl ScriptedBackend {
        fn new(outputs: Vec<ExecutionOutput>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                outputs: Mutex::new(outputs),
            }
        }
    }

    impl ExecutionBackend for ScriptedBackend {
        type Error = DisplayGitError;

        async fn run(
            &self,
            request: ExecutionRequest,
        ) -> std::result::Result<ExecutionOutput, Self::Error> {
            self.requests.lock().unwrap().push(request);
            Ok(self.outputs.lock().unwrap().remove(0))
        }
    }

    #[derive(Debug)]
    struct BackendErrorExecutionBackend;

    impl ExecutionBackend for BackendErrorExecutionBackend {
        type Error = DisplayGitError;

        async fn run(
            &self,
            _request: ExecutionRequest,
        ) -> std::result::Result<ExecutionOutput, Self::Error> {
            Err(DisplayGitError("git backend offline"))
        }
    }

    #[derive(Debug)]
    struct StaticCredentialProvider;

    impl GitCredentialProvider for StaticCredentialProvider {
        type Error = DisplayGitError;

        async fn credential(
            &self,
            _request: GitCredentialRequest,
        ) -> std::result::Result<Option<GitCredential>, Self::Error> {
            Ok(Some(GitCredential::new("secret-token".to_string())))
        }
    }

    #[derive(Debug)]
    struct CredentialErrorProvider;

    impl GitCredentialProvider for CredentialErrorProvider {
        type Error = DisplayGitError;

        async fn credential(
            &self,
            _request: GitCredentialRequest,
        ) -> std::result::Result<Option<GitCredential>, Self::Error> {
            Err(DisplayGitError("token unavailable"))
        }
    }

    fn workspace_config() -> GitWorkspaceConfig {
        GitWorkspaceConfig {
            worktree: PathBuf::from("/workspace/repo"),
            git_binary: PathBuf::from("git"),
            policy: GitPolicy::default(),
            native_credentials: false,
            default_push_branch: Some("mai-agent/test".to_string()),
            remote_url: None,
            workspace_info: BTreeMap::new(),
        }
    }

    fn ok(stdout: &str) -> ExecutionOutput {
        ExecutionOutput {
            status: 0,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn test_context() -> CallContext {
        crate::test_support::thread_context()
    }

    #[test]
    fn git_tool_schema_does_not_expose_token_fields() {
        let backend = Arc::new(RecordingBackend::default());
        let provider = Arc::new(StaticCredentialProvider);
        let tool = GitTool::new(GitToolKind::Fetch, workspace_config(), backend, provider);

        let schema = tool.kind.input_schema().to_string();

        assert!(!schema.contains("token"));
        assert!(!schema.contains("credential"));
    }

    #[test]
    fn only_readonly_git_operations_allow_programmatic_callers() {
        for kind in GitToolKind::all() {
            let pl_protocol::ToolSpec::Function {
                allowed_callers,
                output_schema,
                ..
            } = kind.to_spec()
            else {
                panic!("Git tools use JSON arguments")
            };
            if matches!(
                kind,
                GitToolKind::Status | GitToolKind::Diff | GitToolKind::WorkspaceInfo
            ) {
                assert_eq!(
                    allowed_callers,
                    vec![
                        pl_protocol::ToolCallerMode::Direct,
                        pl_protocol::ToolCallerMode::Programmatic
                    ]
                );
                assert!(output_schema.is_some());
            } else {
                assert!(allowed_callers.is_empty());
                assert!(output_schema.is_none());
            }
        }
    }

    #[tokio::test]
    async fn git_status_returns_json_output() {
        let backend = Arc::new(RecordingBackend::default());
        let provider = Arc::new(StaticCredentialProvider);
        let tool = GitTool::new(GitToolKind::Status, workspace_config(), backend, provider);

        let output = tool
            .execute_raw(input(serde_json::json!({})), test_context())
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(output.payload().content()).unwrap(),
            serde_json::json!({
                "status": 0,
                "stdout": "secret-token fetched",
                "stderr": ""
            })
        );
        assert!(output.payload().content().contains("stdout"));
    }

    #[tokio::test]
    async fn git_fetch_uses_provider_token_and_redacts_output() {
        let backend = Arc::new(RecordingBackend::default());
        let provider = Arc::new(StaticCredentialProvider);
        let tool = GitTool::new(
            GitToolKind::Fetch,
            workspace_config(),
            backend.clone(),
            provider,
        );

        let output = tool
            .execute_raw(
                input(serde_json::json!({"remote": "origin", "prune": true})),
                test_context(),
            )
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(output.payload().content()).unwrap(),
            serde_json::json!({
                "status": 0,
                "stdout": "[redacted] fetched",
                "stderr": ""
            })
        );
        let requests = backend.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].args,
            vec![
                "fetch".to_string(),
                "--prune".to_string(),
                "origin".to_string()
            ]
        );
        assert_eq!(
            requests[0].env.get("PL_GIT_TOKEN").map(String::as_str),
            Some("secret-token")
        );
    }

    #[tokio::test]
    async fn git_push_rejects_unsafe_branch_before_backend_runs() {
        let backend = Arc::new(RecordingBackend::default());
        let provider = Arc::new(StaticCredentialProvider);
        let tool = GitTool::new(
            GitToolKind::Push,
            workspace_config(),
            backend.clone(),
            provider,
        );

        let error = tool
            .execute_raw(
                input(serde_json::json!({"branch": "../escape"})),
                test_context(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("unsafe git branch"));
        assert!(backend.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn git_tool_maps_backend_display_error_to_current_tool() {
        let backend = Arc::new(BackendErrorExecutionBackend);
        let provider = Arc::new(StaticCredentialProvider);
        let tool = GitTool::new(GitToolKind::Status, workspace_config(), backend, provider);

        let error = tool
            .execute_raw(input(serde_json::json!({})), test_context())
            .await
            .expect_err("backend should fail");

        assert!(matches!(
            std::error::Error::source(&error).unwrap().downcast_ref::<PureError>().unwrap(),
            PureError::ToolExecutionFailed { tool, error }
                if tool == TOOL_GIT_STATUS && error == "git backend offline"
        ));
    }

    #[tokio::test]
    async fn git_tool_maps_credential_display_error_to_current_tool() {
        let backend = Arc::new(RecordingBackend::default());
        let provider = Arc::new(CredentialErrorProvider);
        let tool = GitTool::new(GitToolKind::Fetch, workspace_config(), backend, provider);

        let error = tool
            .execute_raw(
                input(serde_json::json!({ "remote": "origin" })),
                test_context(),
            )
            .await
            .expect_err("credential provider should fail");

        assert!(matches!(
            std::error::Error::source(&error).unwrap().downcast_ref::<PureError>().unwrap(),
            PureError::ToolExecutionFailed { tool, error }
                if tool == TOOL_GIT_FETCH && error == "token unavailable"
        ));
    }

    #[tokio::test]
    async fn git_sync_default_branch_preserves_dirty_workspace_with_provider_token() {
        let backend = Arc::new(ScriptedBackend::new(vec![
            ok(" M README.md\n"),
            ok("saved worktree"),
            ok("remote set"),
            ok("secret-token fetched"),
            ok("checked out"),
            ok("reset"),
            ok("restored"),
        ]));
        let provider = Arc::new(StaticCredentialProvider);
        let mut config = workspace_config();
        config.policy = GitPolicy::new("dev");
        config.remote_url = Some("https://github.com/owner/repo.git".to_string());
        let tool = GitTool::new(
            GitToolKind::SyncDefaultBranch,
            config,
            backend.clone(),
            provider,
        );

        let output = tool
            .execute_raw(
                input(serde_json::json!({ "preserveChanges": true })),
                test_context(),
            )
            .await
            .expect("sync default branch");

        assert!(!output.payload().content().contains("secret-token"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(output.payload().content()).unwrap(),
            serde_json::json!({
                "clone": "/workspace/repo",
                "worktree": "/workspace/repo",
                "preservedChanges": true,
                "forced": false
            })
        );
        let requests = backend.requests.lock().unwrap();
        let args = requests
            .iter()
            .map(|request| request.args.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            vec![
                vec!["status", "--porcelain"],
                vec!["stash", "push", "-u", "-m", "pl-core sync default branch"],
                vec![
                    "remote",
                    "set-url",
                    "origin",
                    "https://github.com/owner/repo.git"
                ],
                vec!["fetch", "--prune", "origin"],
                vec!["checkout", "-B", "mai-agent/test", "origin/dev"],
                vec!["reset", "--hard", "origin/dev"],
                vec!["stash", "pop"],
            ]
        );
        assert_eq!(
            requests[3].env.get("PL_GIT_TOKEN").map(String::as_str),
            Some("secret-token")
        );
    }
    #[tokio::test]
    async fn dynamic_git_preserves_redacted_output_and_uses_the_bound_credential_service() {
        use pl_core::tool::opaque::Tool;
        let backend = Arc::new(RecordingBackend::default());
        let tool = GitTool::new(
            GitToolKind::Fetch,
            workspace_config(),
            backend.clone(),
            Arc::new(StaticCredentialProvider),
        );
        let output = Tool::execute(
            &tool,
            pl_core::context::OpaquePayload::new("application/json", 1, "{\"remote\":\"origin\"}")
                .unwrap(),
            crate::test_support::thread_context(),
        )
        .await
        .unwrap();
        let saved: serde_json::Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(saved["status"], 0);
        assert!(!output.payload().content().contains("secret-token"));
        assert!(output.payload().content().contains("[redacted]"));
        let requests = backend.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].env.get(GIT_TOKEN_ENV).map(String::as_str),
            Some("secret-token")
        );
    }
    #[tokio::test]
    async fn failed_git_command_retains_exact_redacted_streams_and_exit_status() {
        let backend = Arc::new(ScriptedBackend::new(vec![ExecutionOutput {
            status: 23,
            stdout: "  secret-token output\r\n".into(),
            stderr: "error secret-token\n\n".into(),
        }]));
        let tool = GitTool::new(
            GitToolKind::Fetch,
            workspace_config(),
            backend.clone(),
            Arc::new(StaticCredentialProvider),
        );
        let error = tool
            .invoke(serde_json::json!({"remote":"origin"}))
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("secret-token"));
        assert!(!error.to_string().contains("output"));
        let observed =
            runner::observed_failure(&error).expect("typed command failure retains its result");
        let output = dynamic_output(observed).unwrap();
        let saved: serde_json::Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(
            saved,
            serde_json::json!({"status":23, "stdout":"  [redacted] output\r\n", "stderr":"error [redacted]\n\n"})
        );
        assert_eq!(backend.requests.lock().unwrap().len(), 1);
    }
}
