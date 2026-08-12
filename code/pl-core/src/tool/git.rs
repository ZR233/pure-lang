use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pl_protocol::PureError;
use serde_json::{Value, json};

use super::{
    BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput, deserialize_tool_input,
};

mod credential;
mod execution;
mod policy;
mod schema;

pub use credential::*;
pub use execution::*;
pub use policy::GitPolicy;
pub use schema::*;

const GIT_TIMEOUT: Duration = Duration::from_secs(600);

/// git 工具运行配置。
#[derive(Debug, Clone, PartialEq)]
pub struct GitWorkspaceConfig {
    pub worktree: PathBuf,
    pub git_binary: PathBuf,
    pub policy: GitPolicy,
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
            default_push_branch: None,
            remote_url: None,
            workspace_info: BTreeMap::new(),
        }
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
}

impl<B, P> Tool for GitTool<B, P>
where
    B: ExecutionBackend,
    P: GitCredentialProvider,
{
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn input_schema(&self) -> Value {
        self.kind.input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let outcome = match self.kind {
                GitToolKind::Status => {
                    deserialize_tool_input::<GitEmptyInput>(self.name(), input.arguments)?;
                    self.run_plain(vec!["status", "--short", "--branch"]).await
                }
                GitToolKind::Diff => self.run_diff(input.arguments).await,
                GitToolKind::Branch => self.run_branch(input.arguments).await,
                GitToolKind::Fetch => self.run_fetch(input.arguments).await,
                GitToolKind::Commit => self.run_commit(input.arguments).await,
                GitToolKind::Push => self.run_push(input.arguments).await,
                GitToolKind::WorkspaceInfo => {
                    deserialize_tool_input::<GitEmptyInput>(self.name(), input.arguments)?;
                    self.workspace_info()
                }
                GitToolKind::SyncDefaultBranch => {
                    self.run_sync_default_branch(input.arguments).await
                }
            }?;
            Ok(ToolOutput {
                description: outcome.description,
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: outcome.exit_code,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

impl<B, P> GitTool<B, P>
where
    B: ExecutionBackend,
    P: GitCredentialProvider,
{
    async fn run_diff(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitDiffInput = deserialize_tool_input(self.name(), arguments)?;
        let path = non_empty(input.path);
        if let Some(path) = path.as_deref() {
            self.config.policy.validate_path(path)?;
        }
        match (input.staged, path.as_deref()) {
            (true, Some(path)) => self.run_plain(vec!["diff", "--staged", "--", path]).await,
            (true, None) => self.run_plain(vec!["diff", "--staged"]).await,
            (false, Some(path)) => self.run_plain(vec!["diff", "--", path]).await,
            (false, None) => self.run_plain(vec!["diff"]).await,
        }
    }

    async fn run_branch(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitBranchInput = deserialize_tool_input(self.name(), arguments)?;
        match input.action.unwrap_or(GitBranchAction::List) {
            GitBranchAction::List => self.run_plain(vec!["branch", "--list", "--all"]).await,
            GitBranchAction::Switch => {
                let name = required_text(self.name(), input.name, "name")?;
                self.config.policy.validate_branch(&name)?;
                self.run_plain(vec!["switch", &name]).await
            }
            GitBranchAction::Create => {
                let name = required_text(self.name(), input.name, "name")?;
                self.config.policy.validate_branch(&name)?;
                if let Some(start_point) = non_empty(input.start_point) {
                    self.config.policy.validate_branch(&start_point)?;
                    self.run_plain(vec!["switch", "-c", &name, &start_point])
                        .await
                } else {
                    self.run_plain(vec!["switch", "-c", &name]).await
                }
            }
        }
    }

    async fn run_fetch(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitFetchInput = deserialize_tool_input(self.name(), arguments)?;
        let remote = non_empty(input.remote()).unwrap_or_else(|| "origin".to_string());
        self.config.policy.validate_remote(&remote)?;
        let refspec = non_empty(input.refspec);
        self.config
            .policy
            .validate_fetch_refspec(refspec.as_deref())?;
        let mut args = vec!["fetch"];
        if input.prune {
            args.push("--prune");
        }
        args.push(&remote);
        if let Some(refspec) = refspec.as_deref() {
            args.push(refspec);
        }
        self.run_with_credential(args, GitCredentialOperation::Fetch, remote.clone())
            .await
    }

    async fn run_commit(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitCommitInput = deserialize_tool_input(self.name(), arguments)?;
        let message = required_text(self.name(), Some(input.message), "message")?;
        if input.all {
            self.run_plain(vec!["commit", "--no-verify", "-am", &message])
                .await
        } else {
            self.run_plain(vec!["commit", "--no-verify", "-m", &message])
                .await
        }
    }

    async fn run_push(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitPushInput = deserialize_tool_input(self.name(), arguments)?;
        let remote = non_empty(input.remote()).unwrap_or_else(|| "origin".to_string());
        self.config.policy.validate_remote(&remote)?;
        let branch = non_empty(input.branch)
            .or_else(|| self.config.default_push_branch.clone())
            .ok_or_else(|| tool_error(self.name(), "missing string field `branch`"))?;
        self.config.policy.validate_branch(&branch)?;
        let destination = format!("HEAD:refs/heads/{branch}");
        let mut args = vec!["push", "--no-verify"];
        if input.set_upstream {
            args.push("-u");
        }
        args.push(&remote);
        args.push(&destination);
        self.run_with_credential(args, GitCredentialOperation::Push, remote.clone())
            .await
    }

    fn workspace_info(&self) -> Result<GitToolOutcome, PureError> {
        let mut payload = serde_json::Map::new();
        payload.insert("worktree".to_string(), json!(self.config.worktree));
        payload.insert("clone".to_string(), json!(self.config.worktree));
        for (key, value) in &self.config.workspace_info {
            payload.insert(key.clone(), value.clone());
        }
        GitToolOutcome::json(self.name(), Value::Object(payload), Some(0))
    }

    async fn run_sync_default_branch(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitSyncDefaultBranchInput = deserialize_tool_input(self.name(), arguments)?;
        if input.force && input.preserve_changes {
            return Err(tool_error(
                self.name(),
                "force and preserveChanges cannot both be true",
            ));
        }

        let status = self.run_plain(vec!["status", "--porcelain"]).await?;
        let dirty = !status.stdout.trim().is_empty();
        if dirty && !input.force && !input.preserve_changes {
            return Err(tool_error(
                self.name(),
                "git workspace has uncommitted changes; pass force=true to discard them or preserveChanges=true to stash them before sync",
            ));
        }
        if dirty && input.preserve_changes {
            self.run_plain(vec![
                "stash",
                "push",
                "-u",
                "-m",
                "pl-core sync default branch",
            ])
            .await?;
        }
        if let Some(remote_url) = self.config.remote_url.as_deref() {
            self.run_plain(vec!["remote", "set-url", "origin", remote_url])
                .await?;
        }
        self.run_with_credential(
            vec!["fetch", "--prune", "origin"],
            GitCredentialOperation::Fetch,
            "origin".to_string(),
        )
        .await?;
        let branch = self
            .config
            .default_push_branch
            .as_deref()
            .unwrap_or(&self.config.policy.default_branch);
        self.config.policy.validate_branch(branch)?;
        let origin_branch = format!("origin/{}", self.config.policy.default_branch);
        self.run_plain(vec!["checkout", "-B", branch, &origin_branch])
            .await?;
        self.run_plain(vec!["reset", "--hard", &origin_branch])
            .await?;
        if input.force {
            self.run_plain(vec!["clean", "-fdx"]).await?;
        }
        if dirty && input.preserve_changes {
            self.run_plain(vec!["stash", "pop"]).await?;
        }

        GitToolOutcome::json(
            self.name(),
            json!({
            "clone": self.config.worktree,
            "worktree": self.config.worktree,
            "preservedChanges": dirty && input.preserve_changes,
            "forced": input.force,
            }),
            Some(0),
        )
    }

    async fn run_plain<S>(&self, args: Vec<S>) -> Result<GitToolOutcome, PureError>
    where
        S: AsRef<str>,
    {
        let request = self.execution_request(args, BTreeMap::new());
        self.run_request(request, None).await
    }

    async fn run_with_credential<S>(
        &self,
        args: Vec<S>,
        operation: GitCredentialOperation,
        remote: String,
    ) -> Result<GitToolOutcome, PureError>
    where
        S: AsRef<str>,
    {
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

struct GitToolOutcome {
    description: String,
    exit_code: Option<i32>,
    stdout: String,
}

impl GitToolOutcome {
    fn command(tool: &str, status: i32, stdout: String, stderr: String) -> Result<Self, PureError> {
        let description = json_description(
            tool,
            json!({
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

    fn json(tool: &str, value: Value, exit_code: Option<i32>) -> Result<Self, PureError> {
        Ok(Self {
            description: json_description(tool, value)?,
            exit_code,
            stdout: String::new(),
        })
    }
}

fn json_description(tool: &str, value: Value) -> Result<String, PureError> {
    serde_json::to_string(&value)
        .map_err(|error| tool_error(tool, format!("failed to serialize git output: {error}")))
}

fn required_text(tool: &str, value: Option<String>, field: &str) -> Result<String, PureError> {
    non_empty(value).ok_or_else(|| tool_error(tool, format!("missing string field `{field}`")))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn redact(value: String, credential: Option<&GitCredential>) -> String {
    match credential {
        Some(credential) => value.replace(credential.expose(), "[redacted]"),
        None => value,
    }
}

fn tool_error(tool: &str, error: impl fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use pl_protocol::PureError;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{Tool, ToolContext, ToolInput};

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

    #[test]
    fn git_shell_command_without_credential_uses_safe_git_flags() {
        let command = git_shell_command(GitShellCommandRequest {
            safe_directory: "/workspace/repo",
            args: &["fetch", "origin", "feature branch"],
            credential: GitShellCredential::Disabled,
        });

        assert_eq!(
            command,
            "git -c core.hooksPath=/dev/null -c safe.directory=/workspace/repo -c credential.helper= fetch origin 'feature branch'"
        );
    }

    #[test]
    fn git_shell_command_with_credential_installs_askpass() {
        let command = git_shell_command(GitShellCommandRequest {
            safe_directory: "/workspace/repo",
            args: &["push", "origin", "HEAD:mai-agent/test"],
            credential: GitShellCredential::EnvToken,
        });

        assert!(command.contains("GIT_ASKPASS=\"$askpass\""));
        assert!(command.contains("GIT_TERMINAL_PROMPT=0"));
        assert!(command.contains("$PL_GIT_TOKEN"));
        assert!(command.contains("x-access-token"));
        assert!(command.contains("git -c core.hooksPath=/dev/null"));
        assert!(command.contains("safe.directory=/workspace/repo"));
        assert!(command.contains("push origin HEAD:mai-agent/test"));
    }

    #[test]
    fn git_shell_credential_prelude_installs_pl_token_askpass() {
        let prelude = git_shell_credential_prelude();

        assert!(prelude.contains("GIT_ASKPASS"));
        assert!(prelude.contains("GIT_TERMINAL_PROMPT"));
        assert!(prelude.contains("$PL_GIT_TOKEN"));
        assert!(prelude.contains("x-access-token"));
        assert!(!prelude.contains("MAI_GITHUB_INSTALLATION_TOKEN"));
    }

    #[test]
    fn git_shell_retry_function_defines_generic_retry_wrapper() {
        let function = git_shell_retry_function();

        assert!(function.contains("git_with_retry()"));
        assert!(function.contains("credential.helper="));
        assert!(function.contains("http.version=HTTP/1.1"));
        assert!(function.contains("attempts"));
    }

    fn test_context() -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: crate::turn::TurnOptions::default(),
            workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            lsp_runtime: None,
            parent_session: Arc::new(crate::AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        }
    }

    #[test]
    fn git_policy_rejects_non_origin_remote() {
        let policy = GitPolicy::default();

        assert!(policy.validate_remote("origin").is_ok());
        assert!(policy.validate_remote("upstream").is_err());
        assert!(
            policy
                .validate_remote("https://example.com/repo.git")
                .is_err()
        );
    }

    #[test]
    fn git_policy_rejects_unsafe_paths() {
        let policy = GitPolicy::default();

        assert!(policy.validate_path("src/lib.rs").is_ok());
        assert!(policy.validate_path("../secret").is_err());
        assert!(policy.validate_path("/etc/passwd").is_err());
        assert!(policy.validate_path("C:/Windows").is_err());
        assert!(policy.validate_path("bad\\path").is_err());
        assert!(policy.validate_path("bad\u{7f}path").is_err());
    }

    #[test]
    fn git_policy_rejects_unsafe_branch_names() {
        let policy = GitPolicy::default();

        assert!(policy.validate_branch("feature/safe-name").is_ok());
        assert!(policy.validate_branch("").is_err());
        assert!(policy.validate_branch("../escape").is_err());
        assert!(policy.validate_branch("/absolute").is_err());
        assert!(policy.validate_branch("bad\\branch").is_err());
        assert!(policy.validate_branch("bad\nbranch").is_err());
    }

    #[test]
    fn git_policy_allows_default_and_pr_fetch_refspecs_only() {
        let policy = GitPolicy::default();

        assert!(policy.validate_fetch_refspec(None).is_ok());
        assert!(policy.validate_fetch_refspec(Some("main")).is_ok());
        assert!(
            policy
                .validate_fetch_refspec(Some("refs/heads/main"))
                .is_ok()
        );
        assert!(policy.validate_fetch_refspec(Some("pull/42/head")).is_ok());
        assert!(
            policy
                .validate_fetch_refspec(Some("pull/42/head:pr/42"))
                .is_ok()
        );
        assert!(
            policy
                .validate_fetch_refspec(Some("refs/pull/42/head:refs/remotes/origin/pr/42"))
                .is_ok()
        );
        assert!(
            policy
                .validate_fetch_refspec(Some("pull/42/head:refs/pull/43/head"))
                .is_err()
        );
        assert!(
            policy
                .validate_fetch_refspec(Some("+refs/heads/main:refs/heads/main"))
                .is_err()
        );
        assert!(
            policy
                .validate_fetch_refspec(Some("refs/tags/v1.0.0"))
                .is_err()
        );
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
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output.description).unwrap(),
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
            .execute(
                ToolInput {
                    arguments: serde_json::json!({"remote": "origin", "prune": true}),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output.description).unwrap(),
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
            .execute(
                ToolInput {
                    arguments: serde_json::json!({"branch": "../escape"}),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                    revision_base: 0,
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
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                    revision_base: 0,
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
            .execute(
                ToolInput {
                    arguments: serde_json::json!({ "remote": "origin" }),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                    revision_base: 0,
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
            .execute(
                ToolInput {
                    arguments: serde_json::json!({ "preserveChanges": true }),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .expect("sync default branch");

        assert!(!output.description.contains("secret-token"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output.description).unwrap(),
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
