use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pl_model::ToolSchema;
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
pub const TOOL_GIT_SYNC_DEFAULT_BRANCH: &str = "git_sync_default_branch";

const GIT_TIMEOUT: Duration = Duration::from_secs(600);
pub const GIT_TOKEN_ENV: &str = "PL_GIT_TOKEN";

/// git shell 命令的凭据注入模式。
///
/// 容器或 sidecar backend 需要把 git 命令序列化成 shell 字符串时，用该枚举选择是否
/// 通过统一 askpass 脚本读取 `PL_GIT_TOKEN`。token 值只应通过环境变量传入，不能拼进
/// 命令文本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitShellCredential {
    Disabled,
    EnvToken,
}

/// 生成可在 shell backend 中执行的 git 命令。
///
/// 该请求面向容器、sidecar 等只能接收 shell 字符串的执行后端。调用方负责把工作区挂载
/// 到 `safe_directory`，并在 `credential` 为 [`GitShellCredential::EnvToken`] 时注入
/// [`GIT_TOKEN_ENV`] 环境变量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitShellCommandRequest<'a> {
    pub safe_directory: &'a str,
    pub args: &'a [&'a str],
    pub credential: GitShellCredential,
}

pub fn git_shell_command(request: GitShellCommandRequest<'_>) -> String {
    let mut command_parts = vec![
        "git".to_string(),
        "-c".to_string(),
        shell_quote_word("core.hooksPath=/dev/null"),
        "-c".to_string(),
        shell_quote_word(&format!("safe.directory={}", request.safe_directory)),
        "-c".to_string(),
        shell_quote_word("credential.helper="),
    ];
    command_parts.extend(request.args.iter().map(|arg| shell_quote_word(arg)));
    let git_command = command_parts.join(" ");
    match request.credential {
        GitShellCredential::Disabled => git_command,
        GitShellCredential::EnvToken => git_shell_command_with_askpass(&git_command),
    }
}

/// 生成 shell 脚本片段，为后续 git 命令安装统一 askpass 凭据环境。
///
/// 该片段不会设置 `set -e`，调用方可把它嵌入更大的 sidecar 脚本，并通过
/// [`GIT_TOKEN_ENV`] 环境变量传入 token。
pub fn git_shell_credential_prelude() -> String {
    format!(
        "askpass=/tmp/pl-git-askpass-$$.sh\n\
         trap 'rm -f \"$askpass\"' EXIT\n\
         cat > \"$askpass\" <<'PL_GIT_ASKPASS'\n\
         {}PL_GIT_ASKPASS\n\
         chmod 700 \"$askpass\"\n\
         export GIT_ASKPASS=\"$askpass\"\n\
         export GIT_TERMINAL_PROMPT=0\n",
        git_askpass_script()
    )
}

/// 生成 sidecar shell 脚本中可复用的 `git_with_retry` 函数。
pub fn git_shell_retry_function() -> &'static str {
    "git_with_retry() {\n\
       attempts=0\n\
       while :; do\n\
         attempts=$((attempts + 1))\n\
         git -c credential.helper= -c http.version=HTTP/1.1 \"$@\" && return 0\n\
         status=$?\n\
         if [ \"$attempts\" -ge 3 ]; then\n\
           return \"$status\"\n\
         fi\n\
         sleep $((attempts * 2))\n\
       done\n\
     }\n"
}

/// 对单个 shell word 做 POSIX 风格转义。
pub fn shell_quote_word(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':' | b'=')
    }) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

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
    type Error: fmt::Display + Send + 'static;

    fn run(
        &self,
        request: ExecutionRequest,
    ) -> impl Future<Output = std::result::Result<ExecutionOutput, Self::Error>> + Send;
}

/// 本地进程执行后端。
#[derive(Debug, Clone, Default)]
pub struct LocalExecutionBackend;

impl ExecutionBackend for LocalExecutionBackend {
    type Error = String;

    async fn run(
        &self,
        request: ExecutionRequest,
    ) -> std::result::Result<ExecutionOutput, Self::Error> {
        let mut command = Command::new(&request.program);
        command.args(&request.args);
        command.current_dir(&request.cwd);
        command.envs(&request.env);
        let output = match request.timeout {
            Some(timeout) => tokio::time::timeout(timeout, command.output())
                .await
                .map_err(|_| "command timed out".to_string())?,
            None => command.output().await,
        }
        .map_err(|error| format!("failed to run command: {error}"))?;
        Ok(ExecutionOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
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
    type Error: fmt::Display + Send + 'static;

    fn credential(
        &self,
        request: GitCredentialRequest,
    ) -> impl Future<Output = std::result::Result<Option<GitCredential>, Self::Error>> + Send;
}

/// 不提供任何 git 凭据的 provider。
#[derive(Debug, Clone, Default)]
pub struct NoGitCredentialProvider;

impl GitCredentialProvider for NoGitCredentialProvider {
    type Error = String;

    async fn credential(
        &self,
        _request: GitCredentialRequest,
    ) -> std::result::Result<Option<GitCredential>, Self::Error> {
        Ok(None)
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
        let normalized = path.trim();
        if normalized.is_empty()
            || normalized != path
            || normalized.starts_with('/')
            || normalized.contains('\\')
            || has_windows_drive_prefix(normalized)
            || normalized.chars().any(char::is_control)
            || Path::new(normalized).is_absolute()
            || Path::new(normalized)
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
    SyncDefaultBranch,
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
            Self::SyncDefaultBranch,
        ]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            TOOL_GIT_STATUS => Some(Self::Status),
            TOOL_GIT_DIFF => Some(Self::Diff),
            TOOL_GIT_BRANCH => Some(Self::Branch),
            TOOL_GIT_FETCH => Some(Self::Fetch),
            TOOL_GIT_COMMIT => Some(Self::Commit),
            TOOL_GIT_PUSH => Some(Self::Push),
            TOOL_GIT_WORKSPACE_INFO => Some(Self::WorkspaceInfo),
            TOOL_GIT_SYNC_DEFAULT_BRANCH => Some(Self::SyncDefaultBranch),
            _ => None,
        }
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
            Self::SyncDefaultBranch => TOOL_GIT_SYNC_DEFAULT_BRANCH,
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
            Self::SyncDefaultBranch => {
                "Synchronize this workspace branch with the configured default branch."
            }
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::Status | Self::WorkspaceInfo => object_schema(vec![]),
            Self::Diff => object_schema(vec![
                ("staged", json!({ "type": "boolean" }), false),
                ("path", json!({ "type": "string" }), false),
            ]),
            Self::Branch => object_schema(vec![
                (
                    "action",
                    json!({ "type": "string", "enum": ["list", "switch", "create"] }),
                    false,
                ),
                ("name", json!({ "type": "string" }), false),
                ("startPoint", json!({ "type": "string" }), false),
            ]),
            Self::Fetch => object_schema(vec![
                ("remote", json!({ "type": "string" }), false),
                ("refspec", json!({ "type": "string" }), false),
                ("prune", json!({ "type": "boolean" }), false),
            ]),
            Self::Commit => object_schema(vec![
                ("message", json!({ "type": "string" }), true),
                ("all", json!({ "type": "boolean" }), false),
            ]),
            Self::Push => object_schema(vec![
                ("remote", json!({ "type": "string" }), false),
                ("branch", json!({ "type": "string" }), false),
                ("setUpstream", json!({ "type": "boolean" }), false),
            ]),
            Self::SyncDefaultBranch => object_schema(vec![
                (
                    "force",
                    json!({
                        "type": "boolean",
                        "description": "Discard uncommitted workspace changes while syncing."
                    }),
                    false,
                ),
                (
                    "preserveChanges",
                    json!({
                        "type": "boolean",
                        "description": "Stash uncommitted workspace changes before syncing and restore them afterwards."
                    }),
                    false,
                ),
            ]),
        }
    }

    pub fn to_schema(self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
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
                GitToolKind::Status => self.run_plain(vec!["status", "--short", "--branch"]).await,
                GitToolKind::Diff => self.run_diff(input.arguments).await,
                GitToolKind::Branch => self.run_branch(input.arguments).await,
                GitToolKind::Fetch => self.run_fetch(input.arguments).await,
                GitToolKind::Commit => self.run_commit(input.arguments).await,
                GitToolKind::Push => self.run_push(input.arguments).await,
                GitToolKind::WorkspaceInfo => self.workspace_info(),
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
        GitToolOutcome::json(self.name(), Value::Object(payload), Some(0))
    }

    async fn run_sync_default_branch(&self, arguments: Value) -> Result<GitToolOutcome, PureError> {
        let input: GitSyncDefaultBranchInput = parse_input(self.name(), arguments)?;
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitDiffInput {
    #[serde(default)]
    staged: bool,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitBranchInput {
    action: Option<String>,
    name: Option<String>,
    start_point: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitFetchInput {
    remote: Option<String>,
    refspec: Option<String>,
    #[serde(default = "default_true")]
    prune: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitCommitInput {
    message: String,
    #[serde(default)]
    all: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitPushInput {
    remote: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    set_upstream: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitSyncDefaultBranchInput {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    preserve_changes: bool,
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

fn git_shell_command_with_askpass(git_command: &str) -> String {
    format!(
        "askpass=$(mktemp) && cat > \"$askpass\" <<'PL_GIT_ASKPASS'\n{}PL_GIT_ASKPASS\nchmod 700 \"$askpass\" && GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=\"$askpass\" {git_command}; status=$?; rm -f \"$askpass\"; exit $status",
        git_askpass_script()
    )
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

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
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
            mode: crate::turn::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            agent_supervisor: crate::AgentSupervisor::default(),
            agent_tool_registrar: None,
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
