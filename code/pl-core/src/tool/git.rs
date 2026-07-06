use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pl_protocol::PureError;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::process::Command;

use super::{BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput};

pub const TOOL_GIT_STATUS: &str = "git_status";
pub const TOOL_GIT_DIFF: &str = "git_diff";
pub const TOOL_GIT_BRANCH: &str = "git_branch";
pub const TOOL_GIT_FETCH: &str = "git_fetch";
pub const TOOL_GIT_COMMIT: &str = "git_commit";
pub const TOOL_GIT_PUSH: &str = "git_push";
pub const TOOL_GIT_WORKSPACE_INFO: &str = "git_workspace_info";

const GIT_TIMEOUT: Duration = Duration::from_secs(600);
pub const GIT_TOKEN_ENV: &str = "PL_GIT_TOKEN";

/// 通用命令执行请求。
#[derive(Clone, PartialEq, Eq)]
pub struct ExecutionRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub timeout: Option<Duration>,
}

impl fmt::Debug for ExecutionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let env = self
            .env
            .keys()
            .map(|key| {
                let value = if key.contains("TOKEN") || key.contains("PASSWORD") {
                    "[redacted]"
                } else {
                    self.env.get(key).map(String::as_str).unwrap_or_default()
                };
                (key, value)
            })
            .collect::<BTreeMap<_, _>>();
        f.debug_struct("ExecutionRequest")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("env", &env)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// 通用命令执行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// shell/process 类工具共用的执行后端。
///
/// 实现方负责在指定工作目录运行命令，并遵守请求中给出的环境和超时。
pub trait ExecutionBackend: fmt::Debug + Send + Sync {
    fn run(
        &self,
        request: ExecutionRequest,
    ) -> impl Future<Output = Result<ExecutionOutput, PureError>> + Send;
}

/// 本地进程执行后端。
#[derive(Debug, Clone, Default)]
pub struct LocalExecutionBackend;

impl ExecutionBackend for LocalExecutionBackend {
    fn run(
        &self,
        request: ExecutionRequest,
    ) -> impl Future<Output = Result<ExecutionOutput, PureError>> + Send {
        async move {
            let mut command = Command::new(&request.program);
            command.args(&request.args);
            command.current_dir(&request.cwd);
            command.envs(&request.env);
            let output = match request.timeout {
                Some(timeout) => tokio::time::timeout(timeout, command.output())
                    .await
                    .map_err(|_| tool_error("execution", "command timed out"))?,
                None => command.output().await,
            }
            .map_err(|error| tool_error("execution", format!("failed to run command: {error}")))?;
            Ok(ExecutionOutput {
                status: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        }
    }
}

/// 需要 git 凭据的操作类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitCredentialOperation {
    Fetch,
    Push,
}

/// git 凭据请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCredentialRequest {
    pub operation: GitCredentialOperation,
    pub remote: String,
}

/// git 短期凭据。
#[derive(Clone)]
pub struct GitCredential(SecretString);

impl fmt::Debug for GitCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("GitCredential").field(&"[redacted]").finish()
    }
}

impl GitCredential {
    pub fn new(value: String) -> Self {
        Self(SecretString::from(value))
    }

    fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

/// 为需要认证的 git 操作按需提供凭据。
///
/// 实现方只返回当前 workspace 可用的短期 token；不得把 token 放进工具 schema、
/// 参数回显或错误文本。返回 `None` 表示该操作没有可用凭据。
pub trait GitCredentialProvider: fmt::Debug + Send + Sync {
    fn credential(
        &self,
        request: GitCredentialRequest,
    ) -> impl Future<Output = Result<Option<GitCredential>, PureError>> + Send;
}

/// 不提供任何 git 凭据的 provider。
#[derive(Debug, Clone, Default)]
pub struct NoGitCredentialProvider;

impl GitCredentialProvider for NoGitCredentialProvider {
    fn credential(
        &self,
        _request: GitCredentialRequest,
    ) -> impl Future<Output = Result<Option<GitCredential>, PureError>> + Send {
        async { Ok(None) }
    }
}

/// git workspace 安全策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPolicy {
    allowed_remote: String,
    default_branch: String,
}

impl Default for GitPolicy {
    fn default() -> Self {
        Self {
            allowed_remote: "origin".to_string(),
            default_branch: "main".to_string(),
        }
    }
}

impl GitPolicy {
    pub fn new(default_branch: impl Into<String>) -> Self {
        Self {
            default_branch: default_branch.into(),
            ..Self::default()
        }
    }

    pub fn validate_remote(&self, remote: &str) -> Result<(), PureError> {
        if remote == self.allowed_remote {
            Ok(())
        } else {
            Err(tool_error(
                "git",
                format!(
                    "unsupported git remote `{remote}`; only `{}` is allowed",
                    self.allowed_remote
                ),
            ))
        }
    }

    pub fn validate_branch(&self, branch: &str) -> Result<(), PureError> {
        if branch.trim().is_empty()
            || branch.starts_with('/')
            || branch.ends_with('/')
            || branch.starts_with('.')
            || branch.contains("..")
            || branch.contains("//")
            || branch.contains("@{")
            || branch.contains('\\')
            || branch.ends_with(".lock")
            || branch.chars().any(char::is_control)
        {
            return Err(tool_error("git", format!("unsafe git branch `{branch}`")));
        }
        Ok(())
    }

    pub fn validate_path(&self, path: &str) -> Result<(), PureError> {
        if path.trim().is_empty()
            || path.contains('\\')
            || path.chars().any(char::is_control)
            || Path::new(path).is_absolute()
            || Path::new(path)
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(tool_error("git", format!("unsafe git path `{path}`")));
        }
        Ok(())
    }

    pub fn validate_fetch_refspec(&self, refspec: Option<&str>) -> Result<(), PureError> {
        let Some(refspec) = refspec else {
            return Ok(());
        };
        if refspec == self.default_branch
            || refspec == format!("refs/heads/{}", self.default_branch)
            || is_pull_request_head_ref(refspec)
        {
            Ok(())
        } else {
            Err(tool_error(
                "git",
                format!("unsupported git fetch refspec `{refspec}`"),
            ))
        }
    }
}

/// git 工具运行配置。
#[derive(Debug, Clone, PartialEq)]
pub struct GitWorkspaceConfig {
    pub worktree: PathBuf,
    pub git_binary: PathBuf,
    pub policy: GitPolicy,
    pub default_push_branch: Option<String>,
    pub workspace_info: BTreeMap<String, Value>,
}

impl GitWorkspaceConfig {
    pub fn local(worktree: impl Into<PathBuf>) -> Self {
        Self {
            worktree: worktree.into(),
            git_binary: PathBuf::from("git"),
            policy: GitPolicy::default(),
            default_push_branch: None,
            workspace_info: BTreeMap::new(),
        }
    }
}

/// pl-core 提供的通用 git 工具类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitToolKind {
    Status,
    Diff,
    Branch,
    Fetch,
    Commit,
    Push,
    WorkspaceInfo,
}

impl GitToolKind {
    pub fn all() -> &'static [Self] {
        &[
            Self::Status,
            Self::Diff,
            Self::Branch,
            Self::Fetch,
            Self::Commit,
            Self::Push,
            Self::WorkspaceInfo,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Status => TOOL_GIT_STATUS,
            Self::Diff => TOOL_GIT_DIFF,
            Self::Branch => TOOL_GIT_BRANCH,
            Self::Fetch => TOOL_GIT_FETCH,
            Self::Commit => TOOL_GIT_COMMIT,
            Self::Push => TOOL_GIT_PUSH,
            Self::WorkspaceInfo => TOOL_GIT_WORKSPACE_INFO,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Status => "Show git working tree status for this workspace.",
            Self::Diff => "Show git diff for this workspace.",
            Self::Branch => "List branches or create/switch the current branch.",
            Self::Fetch => "Fetch from the repository remote using host-injected credentials.",
            Self::Commit => "Create a git commit in this workspace.",
            Self::Push => "Push the current branch using host-injected credentials.",
            Self::WorkspaceInfo => "Show information about this git workspace.",
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
        match self.kind {
            GitToolKind::Status | GitToolKind::WorkspaceInfo => object_schema(vec![]),
            GitToolKind::Diff => object_schema(vec![
                ("staged", json!({ "type": "boolean" }), false),
                ("path", json!({ "type": "string" }), false),
            ]),
            GitToolKind::Branch => object_schema(vec![
                (
                    "action",
                    json!({ "type": "string", "enum": ["list", "switch", "create"] }),
                    false,
                ),
                ("name", json!({ "type": "string" }), false),
                ("start_point", json!({ "type": "string" }), false),
            ]),
            GitToolKind::Fetch => object_schema(vec![
                ("remote", json!({ "type": "string" }), false),
                ("refspec", json!({ "type": "string" }), false),
                ("prune", json!({ "type": "boolean" }), false),
            ]),
            GitToolKind::Commit => object_schema(vec![
                ("message", json!({ "type": "string" }), true),
                ("all", json!({ "type": "boolean" }), false),
            ]),
            GitToolKind::Push => object_schema(vec![
                ("remote", json!({ "type": "string" }), false),
                ("branch", json!({ "type": "string" }), false),
                ("set_upstream", json!({ "type": "boolean" }), false),
            ]),
        }
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let outcome = match self.kind {
                GitToolKind::Status => self.run_plain(vec!["status", "--short", "--branch"]).await,
                GitToolKind::Diff => self.run_diff(input.arguments).await,
                GitToolKind::Branch => self.run_branch(input.arguments).await,
                GitToolKind::Fetch => self.run_fetch(input.arguments).await,
                GitToolKind::Commit => self.run_commit(input.arguments).await,
                GitToolKind::Push => self.run_push(input.arguments).await,
                GitToolKind::WorkspaceInfo => self.workspace_info(),
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
        let input: GitDiffInput = parse_input(self.name(), arguments)?;
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
        let input: GitBranchInput = parse_input(self.name(), arguments)?;
        match input.action.as_deref().unwrap_or("list") {
            "list" => self.run_plain(vec!["branch", "--list", "--all"]).await,
            "switch" => {
                let name = required_text(self.name(), input.name, "name")?;
                self.config.policy.validate_branch(&name)?;
                self.run_plain(vec!["switch", &name]).await
            }
            "create" => {
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
            other => Err(tool_error(
                self.name(),
                format!("unsupported git branch action `{other}`"),
            )),
        }
    }

    async fn run_fetch(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitFetchInput = parse_input(self.name(), arguments)?;
        let remote = non_empty(input.remote).unwrap_or_else(|| "origin".to_string());
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
        let input: GitCommitInput = parse_input(self.name(), arguments)?;
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
        let input: GitPushInput = parse_input(self.name(), arguments)?;
        let remote = non_empty(input.remote).unwrap_or_else(|| "origin".to_string());
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
        let description = serde_json::to_string(&payload).map_err(|error| {
            tool_error(
                self.name(),
                format!("failed to serialize workspace info: {error}"),
            )
        })?;
        Ok(GitToolOutcome {
            description,
            exit_code: Some(0),
        })
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
            .await?
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
        let output = self.backend.run(request).await?;
        let stdout = redact(output.stdout, credential);
        let stderr = redact(output.stderr, credential);
        if output.status == 0 {
            return Ok(GitToolOutcome {
                description: stdout,
                exit_code: Some(output.status),
            });
        }
        let combined = format!("{stderr}\n{stdout}");
        Err(tool_error(
            self.name(),
            format!("git command failed: {}", combined.trim()),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct GitDiffInput {
    #[serde(default)]
    staged: bool,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitBranchInput {
    action: Option<String>,
    name: Option<String>,
    start_point: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitFetchInput {
    remote: Option<String>,
    refspec: Option<String>,
    #[serde(default = "default_true")]
    prune: bool,
}

#[derive(Debug, Deserialize)]
struct GitCommitInput {
    message: String,
    #[serde(default)]
    all: bool,
}

#[derive(Debug, Deserialize)]
struct GitPushInput {
    remote: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    set_upstream: bool,
}

struct GitToolOutcome {
    description: String,
    exit_code: Option<i32>,
}

fn object_schema(properties: Vec<(&'static str, Value, bool)>) -> Value {
    let mut props = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, schema, is_required) in properties {
        props.insert(name.to_string(), schema);
        if is_required {
            required.push(Value::String(name.to_string()));
        }
    }
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn parse_input<T>(tool: &str, arguments: Value) -> Result<T, PureError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(arguments)
        .map_err(|error| tool_error(tool, format!("invalid git tool input: {error}")))
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

async fn write_askpass_script(tool: &str) -> Result<PathBuf, PureError> {
    let path = std::env::temp_dir().join(format!(
        "pl-core-git-askpass-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    tokio::fs::write(&path, git_askpass_script())
        .await
        .map_err(|error| tool_error(tool, format!("failed to write git askpass: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|error| tool_error(tool, format!("failed to chmod git askpass: {error}")))?;
    }
    Ok(path)
}

fn git_askpass_script() -> &'static str {
    "#!/bin/sh\ncase \"$1\" in\n  *Username*) printf '%s\\n' x-access-token ;;\n  *Password*) printf '%s\\n' \"$PL_GIT_TOKEN\" ;;\n  *) printf '\\n' ;;\nesac\n"
}

fn default_true() -> bool {
    true
}

fn is_pull_request_head_ref(refspec: &str) -> bool {
    let (source, destination) = match refspec.split_once(':') {
        Some((source, destination)) => (source, Some(destination)),
        None => (refspec, None),
    };
    let source = source.strip_prefix("refs/").unwrap_or(source);
    let Some(number) = pull_request_head_number(source) else {
        return false;
    };
    match destination {
        Some(destination) => is_pull_request_head_destination(destination, number),
        None => true,
    }
}

fn pull_request_head_number(refspec: &str) -> Option<&str> {
    let refspec = refspec.strip_prefix("refs/").unwrap_or(refspec);
    let rest = refspec.strip_prefix("pull/")?;
    let number = rest.strip_suffix("/head")?;
    (!number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit())).then_some(number)
}

fn is_pull_request_head_destination(destination: &str, number: &str) -> bool {
    destination == format!("pr/{number}")
        || destination == format!("refs/pull/{number}/head")
        || destination == format!("refs/remotes/origin/pr/{number}")
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
    use std::future::Future;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use pl_protocol::PureError;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{Tool, ToolContext, ToolInput};

    #[derive(Debug, Default)]
    struct RecordingBackend {
        requests: Mutex<Vec<ExecutionRequest>>,
    }

    impl ExecutionBackend for RecordingBackend {
        fn run(
            &self,
            request: ExecutionRequest,
        ) -> impl Future<Output = Result<ExecutionOutput, PureError>> + Send {
            self.requests.lock().unwrap().push(request);
            async move {
                Ok(ExecutionOutput {
                    status: 0,
                    stdout: "secret-token fetched".to_string(),
                    stderr: String::new(),
                })
            }
        }
    }

    #[derive(Debug)]
    struct StaticCredentialProvider;

    impl GitCredentialProvider for StaticCredentialProvider {
        fn credential(
            &self,
            _request: GitCredentialRequest,
        ) -> impl Future<Output = Result<Option<GitCredential>, PureError>> + Send {
            async { Ok(Some(GitCredential::new("secret-token".to_string()))) }
        }
    }

    fn workspace_config() -> GitWorkspaceConfig {
        GitWorkspaceConfig {
            worktree: PathBuf::from("/workspace/repo"),
            git_binary: PathBuf::from("git"),
            policy: GitPolicy::default(),
            default_push_branch: Some("mai-agent/test".to_string()),
            workspace_info: BTreeMap::new(),
        }
    }

    fn test_context() -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: crate::turn::TurnOptions::default(),
            workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
            mode: crate::turn::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            agent_supervisor: crate::AgentSupervisor::default(),
            lsp_runtime: None,
            parent_session: Arc::new(crate::CoreSession::new()),
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

        assert_eq!(output.description, "[redacted] fetched");
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
}
