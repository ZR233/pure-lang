//! [`GitTool`] 经 `Tool::execute` 的端到端行为测试。

use std::collections::BTreeMap;
use std::fmt;
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
