use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pl_protocol::{PureError, Result};
use pretty_assertions::assert_eq;
use serde_json::Value;

use super::client::{BoxFuture, McpClient};
use super::registry::McpRuntimeServerState;
use super::tool_adapter::{McpToolAdapter, format_mcp_content};
use super::transport::{HttpMcpClient, McpStderrSeverity, classify_mcp_stderr_line};
use super::wire::{McpToolDefinition, default_input_schema};
use super::{McpAvailabilityKind, McpRuntimeRegistry, exposed_tool_name, is_mcp_tool_name};
use crate::config::{McpServerConfig, McpServerTransport, PureConfig, effective_mcp_servers};
use crate::tool::{Tool, ToolContext, ToolInput};

#[derive(Debug)]
struct FakeMcpClient {
    behavior: FakeMcpBehavior,
    shutdown_count: Option<Arc<AtomicUsize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeMcpBehavior {
    Succeed,
    FailRequests,
}

impl McpClient for FakeMcpClient {
    fn request<'a>(&'a self, _method: &'a str, _params: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            match self.behavior {
                FakeMcpBehavior::Succeed => Ok(serde_json::json!({
                    "content": [{"type": "text", "text": "ok"}],
                    "isError": false
                })),
                FakeMcpBehavior::FailRequests => Err(PureError::ToolExecutionFailed {
                    tool: "mcp".to_string(),
                    error: "transport failed".to_string(),
                }),
            }
        })
    }

    fn notify<'a>(&'a self, _method: &'a str, _params: Value) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn shutdown<'a>(&'a self) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if let Some(count) = &self.shutdown_count {
                count.fetch_add(1, Ordering::SeqCst);
            }
        })
    }
}

fn fake_client(behavior: FakeMcpBehavior) -> Arc<FakeMcpClient> {
    Arc::new(FakeMcpClient {
        behavior,
        shutdown_count: None,
    })
}

#[test]
fn exposed_tool_name_prefixes_server_and_tool() {
    let name = exposed_tool_name("github", "search_issues").unwrap();

    assert_eq!(name, "mcp__github__search_issues");
    assert!(is_mcp_tool_name(&name));
}

#[test]
fn exposed_tool_name_rejects_invalid_raw_tool() {
    let error = exposed_tool_name("github", "bad tool").unwrap_err();

    assert!(error.to_string().contains("MCP tool name"));
}

#[test]
fn format_mcp_content_prefers_text_parts() {
    let content = vec![
        serde_json::json!({"type": "text", "text": "hello"}),
        serde_json::json!({"type": "json", "json": {"ok": true}}),
    ];

    assert_eq!(format_mcp_content(&content), "hello\n{\"ok\":true}");
}

#[test]
fn mcp_stderr_info_lines_are_suppressed() {
    assert_eq!(
        classify_mcp_stderr_line(
            r#"{"timestamp":"2026-06-24T12:28:05.798Z","level":"INFO","message":"Running"}"#
        ),
        McpStderrSeverity::Info
    );
    assert_eq!(
        classify_mcp_stderr_line("[2026-06-24T12:28:05.798Z] INFO: MCP Server started"),
        McpStderrSeverity::Info
    );
    assert_eq!(classify_mcp_stderr_line(""), McpStderrSeverity::Info);
}

#[test]
fn mcp_stderr_warning_error_and_unknown_lines_are_forwarded() {
    assert_eq!(
        classify_mcp_stderr_line(
            r#"{"timestamp":"2026-06-24T12:28:05.798Z","level":"WARN","message":"Retrying"}"#
        ),
        McpStderrSeverity::Warning
    );
    assert_eq!(
        classify_mcp_stderr_line("[2026-06-24T12:28:05.798Z] ERROR: startup failed"),
        McpStderrSeverity::Error
    );
    assert_eq!(
        classify_mcp_stderr_line("child process exited unexpectedly"),
        McpStderrSeverity::Error
    );
}

#[test]
fn http_client_uses_bearer_token_override() {
    let server = McpServerConfig {
        transport: McpServerTransport::StreamableHttp,
        url: Some("https://example.com/mcp".to_string()),
        bearer_token_env_var: Some("IGNORED_ENV_VAR".to_string()),
        ..Default::default()
    };

    let client =
        HttpMcpClient::new("zhipu_search", &server, Some("coding-plan-key".to_string())).unwrap();

    assert_eq!(client.bearer_token.as_deref(), Some("coding-plan-key"));
}

#[tokio::test]
async fn registry_marks_disabled_and_missing_credential_without_probe() {
    let mut config = PureConfig::default();
    config.mcp_servers.insert(
        "draft".to_string(),
        McpServerConfig {
            enabled: false,
            ..Default::default()
        },
    );
    let registry = McpRuntimeRegistry::new();

    registry.reconcile(effective_mcp_servers(&config)).await;
    let snapshots = registry.snapshots().await;

    assert_eq!(
        snapshots["draft"].availability_kind,
        McpAvailabilityKind::Disabled
    );
    assert_eq!(
        snapshots["zhipu_search"].availability_kind,
        McpAvailabilityKind::MissingCredential
    );
    assert!(registry.available_server_names().await.is_empty());
}

#[tokio::test]
async fn registry_registers_only_available_tools() {
    let registry = McpRuntimeRegistry::new();
    registry.state.lock().await.servers.insert(
        "github".to_string(),
        McpRuntimeServerState::available(
            1,
            123,
            fake_client(FakeMcpBehavior::Succeed),
            vec![McpToolDefinition {
                name: "search_issues".to_string(),
                description: Some("Search issues".to_string()),
                input_schema: default_input_schema(),
            }],
        ),
    );
    registry
        .state
        .lock()
        .await
        .servers
        .insert("draft".to_string(), McpRuntimeServerState::disabled(1));
    let mut core = crate::PureCore::default_provider().unwrap();

    registry.register_available_tools(&mut core).await.unwrap();

    assert!(core.has_tool("mcp__github__search_issues"));
    assert!(!core.has_tool("mcp__draft__anything"));
}

#[tokio::test]
async fn registry_shutdown_closes_available_clients() {
    let registry = McpRuntimeRegistry::new();
    let shutdown_count = Arc::new(AtomicUsize::new(0));
    registry.state.lock().await.servers.insert(
        "github".to_string(),
        McpRuntimeServerState::available(
            1,
            123,
            Arc::new(FakeMcpClient {
                behavior: FakeMcpBehavior::Succeed,
                shutdown_count: Some(shutdown_count.clone()),
            }),
            Vec::new(),
        ),
    );

    registry.shutdown().await;

    assert_eq!(shutdown_count.load(Ordering::SeqCst), 1);
    assert!(registry.snapshots().await.is_empty());
}

#[tokio::test]
async fn reconcile_disabled_server_closes_previous_client() {
    let registry = McpRuntimeRegistry::new();
    let shutdown_count = Arc::new(AtomicUsize::new(0));
    registry.state.lock().await.servers.insert(
        "github".to_string(),
        McpRuntimeServerState::available(
            1,
            123,
            Arc::new(FakeMcpClient {
                behavior: FakeMcpBehavior::Succeed,
                shutdown_count: Some(shutdown_count.clone()),
            }),
            Vec::new(),
        ),
    );
    let mut config = PureConfig::default();
    config.mcp_servers.insert(
        "github".to_string(),
        McpServerConfig {
            enabled: false,
            ..Default::default()
        },
    );

    registry.reconcile(effective_mcp_servers(&config)).await;
    let snapshots = registry.snapshots().await;

    assert_eq!(shutdown_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        snapshots["github"].availability_kind,
        McpAvailabilityKind::Disabled
    );
}

#[tokio::test]
async fn tool_transport_failure_marks_server_unavailable() {
    let registry = McpRuntimeRegistry::new();
    registry.state.lock().await.servers.insert(
        "github".to_string(),
        McpRuntimeServerState::available(1, 123, fake_client(FakeMcpBehavior::Succeed), Vec::new()),
    );
    let adapter = McpToolAdapter::new(
        "github",
        McpToolDefinition {
            name: "search_issues".to_string(),
            description: Some("Search issues".to_string()),
            input_schema: default_input_schema(),
        },
        fake_client(FakeMcpBehavior::FailRequests),
        Some(registry.clone()),
    )
    .unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(1);
    let context = ToolContext {
        event_tx,
        options: crate::turn::TurnOptions::default(),
        workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
        mode: crate::turn::CompileMode::Auto,
        workspace_root: std::env::temp_dir(),
        workspace_instructions: None,
        instruction_snapshot: None,
        active_subagent: None,
        agent_control: crate::AgentControl::default(),
        lsp_runtime: None,
        parent_session: Arc::new(crate::CoreSession::new()),
    };

    let error = adapter
        .execute(
            ToolInput {
                arguments: serde_json::json!({}),
                session_id: "session".to_string(),
                tool_id: "tool".to_string(),
                revision_base: 0,
            },
            context,
        )
        .await
        .unwrap_err();
    let snapshots = registry.snapshots().await;

    assert!(error.to_string().contains("transport failed"));
    assert_eq!(
        snapshots["github"].availability_kind,
        McpAvailabilityKind::Unavailable
    );
    assert_eq!(
        registry.available_server_names().await,
        Vec::<String>::new()
    );
}
