//! git 工具:workspace 配置与 [`GitTool`] 适配器。
//!
//! 按域拆分:`schema` 承载工具类型与输入 schema,`policy` 承载安全策略,
//! `credential` 承载凭据注入,`execution` 承载通用命令执行 backend,
//! `commands` 承载各 git 子命令语义,`runner` 承载带凭据注入与脱敏的执行管道。

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use pl_protocol::PureError;
use serde_json::Value;

use super::{
    OutputTruncation, StaticTool, ToolCallContext, ToolPolicy, ToolResult, deserialize_tool_input,
};
use crate::turn::ToolEffect;

mod commands;
mod credential;
mod execution;
mod policy;
mod runner;
mod schema;

pub use credential::*;
pub use execution::*;
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

impl<B, P> StaticTool for GitTool<B, P>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
{
    type Input = Value;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin(self.kind.name()),
            self.kind.description(),
        )
    }

    fn input_schema(&self) -> Value {
        self.kind.input_schema()
    }

    fn policy(&self) -> ToolPolicy {
        let mut policy = ToolPolicy::default()
            .with_effect(self.kind.effect())
            .with_cache_policy(self.kind.cache_policy());
        if self.kind.effect() == ToolEffect::Read {
            policy = policy.with_programmatic_calls();
        }
        policy
    }

    fn execute(
        &self,
        input: Value,
        _context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let outcome = match self.kind {
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
            }?;
            Ok(ToolResult::from_runtime_text(
                outcome.description,
                OutputTruncation::empty(),
                PathBuf::new(),
                outcome.exit_code,
                false,
                Vec::new(),
            ))
        }
    }
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
    use crate::tool::{StaticToolTestExt, ToolCallContext, ToolInput};

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

    fn test_context() -> ToolCallContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolCallContext::test(event_tx)
    }

    #[test]
    fn git_tool_schema_does_not_expose_token_fields() {
        let backend = Arc::new(RecordingBackend::default());
        let provider = Arc::new(StaticCredentialProvider);
        let tool = GitTool::new(GitToolKind::Fetch, workspace_config(), backend, provider);

        let schema = tool.input_schema().to_string();

        assert!(!schema.contains("token"));
        assert!(!schema.contains("credential"));
    }

    #[tokio::test]
    async fn git_status_returns_json_output() {
        let backend = Arc::new(RecordingBackend::default());
        let provider = Arc::new(StaticCredentialProvider);
        let tool = GitTool::new(GitToolKind::Status, workspace_config(), backend, provider);

        let output = tool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({}),
                },
                test_context(),
            )
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output.canonical_output()).unwrap(),
            serde_json::json!({
                "status": 0,
                "stdout": "secret-token fetched",
                "stderr": ""
            })
        );
        assert_eq!(output.exit_code, Some(0));
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
                ToolInput {
                    arguments: serde_json::json!({"remote": "origin", "prune": true}),
                },
                test_context(),
            )
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output.canonical_output()).unwrap(),
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
                ToolInput {
                    arguments: serde_json::json!({"branch": "../escape"}),
                },
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
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({}),
                },
                test_context(),
            )
            .await
            .expect_err("backend should fail");

        assert!(matches!(
            error,
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
                ToolInput {
                    arguments: serde_json::json!({ "remote": "origin" }),
                },
                test_context(),
            )
            .await
            .expect_err("credential provider should fail");

        assert!(matches!(
            error,
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
                ToolInput {
                    arguments: serde_json::json!({ "preserveChanges": true }),
                },
                test_context(),
            )
            .await
            .expect("sync default branch");

        assert!(!output.canonical_output().contains("secret-token"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output.canonical_output()).unwrap(),
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
}
