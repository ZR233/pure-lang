use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use pretty_assertions::assert_eq;
use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{ClientLifecycleMode, ClientServiceExt, ErrorData as McpError, RoleServer, ServiceExt};
use serde_json::{Map, Value, json};

use super::{ConnectedMcp, McpConnector, McpRuntime};
use crate::config::*;
use crate::tool::*;
use crate::turn::ToolEffect;

pub(crate) struct McpTestHarness {
    runtime: super::McpRuntimeHandle,
    closed: Arc<AtomicBool>,
}

impl McpTestHarness {
    pub(crate) async fn install_read_tool(core: &mut crate::TurnEngine) -> Self {
        let closed = Arc::new(AtomicBool::new(false));
        let connector = McpConnector::testing([(
            "docs".to_string(),
            test_connection(vec![test_tool("lookup")], closed.clone()).await,
        )]);
        let runtime = McpRuntime::new(connector).handle();
        runtime
            .reconcile(BTreeMap::from([(
                "docs".to_string(),
                config("docs", Some(ToolEffect::Read)),
            )]))
            .await
            .expect("reconcile test MCP runtime");
        runtime
            .acquire_turn_lease()
            .await
            .expect("test MCP lease")
            .install(core)
            .expect("install test MCP tools");
        Self { runtime, closed }
    }

    pub(crate) async fn shutdown(self) {
        self.runtime.shutdown().await;
        wait_for_closed(&self.closed).await;
    }
}

#[derive(Debug, Clone)]
struct TestServer {
    tools: Vec<rmcp::model::Tool>,
}

#[expect(
    clippy::manual_async_fn,
    reason = "RPITIT keeps the required Send bound explicit"
)]
impl ServerHandler for TestServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_protocol_version(ProtocolVersion::V_2026_07_28)
        .with_server_info(Implementation::new("pl-test-mcp", "1.0.0"))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = self.tools.clone();
        async move {
            Ok(ListToolsResult {
                tools,
                ..Default::default()
            })
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<CallToolResponse, McpError>> + Send + '_ {
        async move {
            let mut meta = Map::new();
            meta.insert("auditId".to_string(), json!("audit-1"));
            let result = if request.name == "fail" {
                CallToolResult::structured_error(json!({
                    "code": "TEST_FAILURE",
                    "details": { "retryable": false }
                }))
            } else {
                CallToolResult::structured(json!({
                    "answer": 42,
                    "arguments": request.arguments
                }))
            }
            .with_meta(Some(MetaObject(meta)));
            Ok(result.into())
        }
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListResourcesResult, McpError>> + Send + '_ {
        async {
            Ok(ListResourcesResult {
                resources: vec![Resource::new("mcp://docs/one", "one")],
                ..Default::default()
            })
        }
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListResourceTemplatesResult, McpError>> + Send + '_
    {
        async {
            Ok(ListResourceTemplatesResult {
                resource_templates: vec![ResourceTemplate::new("mcp://docs/{id}", "docs")],
                ..Default::default()
            })
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ReadResourceResponse, McpError>> + Send + '_ {
        async move {
            Ok(
                ReadResourceResult::new(vec![ResourceContents::text("resource body", request.uri)])
                    .into(),
            )
        }
    }
}

fn test_tool(name: &str) -> rmcp::model::Tool {
    let mut tool = rmcp::model::Tool::new(
        name.to_string(),
        format!("{name} description"),
        json_object(json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "additionalProperties": false
        })),
    );
    tool.output_schema = Some(Arc::new(json_object(json!({
        "type": "object",
        "properties": { "answer": { "type": "integer" } }
    }))));
    tool.annotations = Some(ToolAnnotations::new().read_only(true));
    tool.icons = Some(vec![Icon::new("data:image/png;base64,AA==")]);
    tool.meta = Some(MetaObject(Map::from_iter([(
        "displayGroup".to_string(),
        json!("tests"),
    )])));
    tool
}

async fn test_connection(tools: Vec<rmcp::model::Tool>, closed: Arc<AtomicBool>) -> ConnectedMcp {
    let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
    let server = TestServer { tools };
    tokio::spawn(async move {
        let service = server
            .serve(server_transport)
            .await
            .expect("serve test MCP server");
        let _ = service.waiting().await;
        closed.store(true, Ordering::SeqCst);
    });
    let client = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("pl-test-client", "1.0.0"),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28)
    .serve_with_lifecycle(
        client_transport,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    )
    .await
    .expect("discover test MCP server");
    ConnectedMcp::from_running(client).await
}

#[tokio::test]
async fn rmcp_schema_and_structured_result_survive_registered_tool() {
    let closed = Arc::new(AtomicBool::new(false));
    let connector = McpConnector::testing([(
        "docs".to_string(),
        test_connection(vec![test_tool("lookup")], closed.clone()).await,
    )]);
    let runtime = McpRuntime::new(connector).handle();
    runtime
        .reconcile(BTreeMap::from([("docs".to_string(), config("docs", None))]))
        .await
        .expect("reconcile");
    let lease = runtime.acquire_turn_lease().await.expect("lease");
    let descriptor = &lease.tools()[0];
    assert_eq!(descriptor.exposed_name, "mcp__docs__lookup");
    assert!(descriptor.output_schema.is_some());
    assert_eq!(
        descriptor
            .annotations
            .as_ref()
            .and_then(|value| value.get("readOnlyHint")),
        Some(&Value::Bool(true))
    );
    assert!(descriptor.icons.is_some());
    assert!(descriptor.metadata.is_some());

    let mut core = crate::TurnEngine::default_provider().expect("core");
    lease.install(&mut core).expect("install lease");
    let tool = core
        .registered_tool("mcp__docs__lookup")
        .expect("registered MCP tool");
    assert_eq!(tool.effect(), None, "remote annotation is not trusted");
    assert!(!tool.supports_parallel_tool_calls());
    assert_eq!(tool.runtime_lock_policy(), ToolRuntimeLockPolicy::Exclusive);
    assert_eq!(tool.cache_policy(&json!({})), ToolCachePolicy::Never);
    let display = tool.display_metadata().expect("MCP display metadata");
    assert_eq!(
        display
            .annotations
            .as_ref()
            .and_then(|value| value.get("readOnlyHint")),
        Some(&Value::Bool(true))
    );
    assert!(display.icons.is_some());
    assert!(display.metadata.is_some());
    assert!(matches!(
        tool.to_schema(),
        pl_model::ToolSchema::Function {
            output_schema: Some(_),
            ..
        }
    ));

    let output = tool
        .execute(tool_input(json!({ "query": "meaning" })), test_context())
        .await
        .expect("execute MCP tool");
    assert!(output.description.contains("\"answer\":42"));
    let audit = audit_metadata(&output.runtime_events);
    assert_eq!(audit["result"]["structuredContent"]["answer"], 42);
    assert_eq!(audit["result"]["_meta"]["auditId"], "audit-1");
    assert_eq!(audit["result"]["resultType"], "complete");

    drop(lease);
    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn trusted_read_effect_enables_shared_parallel_registered_tool() {
    let closed = Arc::new(AtomicBool::new(false));
    let connector = McpConnector::testing([(
        "docs".to_string(),
        test_connection(vec![test_tool("lookup")], closed.clone()).await,
    )]);
    let runtime = McpRuntime::new(connector).handle();
    runtime
        .reconcile(BTreeMap::from([(
            "docs".to_string(),
            config("docs", Some(ToolEffect::Read)),
        )]))
        .await
        .expect("reconcile");
    let lease = runtime.acquire_turn_lease().await.expect("lease");
    let mut core = crate::TurnEngine::default_provider().expect("core");
    lease.install(&mut core).expect("install lease");
    let tool = core
        .registered_tool("mcp__docs__lookup")
        .expect("registered MCP tool");
    assert_eq!(tool.effect(), Some(ToolEffect::Read));
    assert!(tool.supports_parallel_tool_calls());
    assert_eq!(tool.runtime_lock_policy(), ToolRuntimeLockPolicy::Shared);
    assert_eq!(tool.cache_policy(&json!({})), ToolCachePolicy::Never);
    drop(lease);
    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn mcp_error_result_keeps_structured_audit_and_failed_terminal_marker() {
    let closed = Arc::new(AtomicBool::new(false));
    let connector = McpConnector::testing([(
        "docs".to_string(),
        test_connection(vec![test_tool("fail")], closed.clone()).await,
    )]);
    let runtime = McpRuntime::new(connector).handle();
    runtime
        .reconcile(BTreeMap::from([("docs".to_string(), config("docs", None))]))
        .await
        .expect("reconcile");
    let lease = runtime.acquire_turn_lease().await.expect("lease");
    let mut core = crate::TurnEngine::default_provider().expect("core");
    lease.install(&mut core).expect("install lease");
    let output = core
        .registered_tool("mcp__docs__fail")
        .expect("registered MCP tool")
        .execute(tool_input(json!({})), test_context())
        .await
        .expect("typed MCP error remains an auditable output");
    assert!(output.description.starts_with("Tool execution error: "));
    assert!(
        output
            .runtime_events
            .contains(&ToolRuntimeEvent::ExecutionFailed)
    );
    assert_eq!(
        audit_metadata(&output.runtime_events)["result"]["structuredContent"]["code"],
        "TEST_FAILURE"
    );
    drop(lease);
    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn retired_generation_closes_only_after_last_lease_releases() {
    let first_closed = Arc::new(AtomicBool::new(false));
    let second_closed = Arc::new(AtomicBool::new(false));
    let connector = McpConnector::testing([
        (
            "docs".to_string(),
            test_connection(vec![test_tool("first")], first_closed.clone()).await,
        ),
        (
            "docs".to_string(),
            test_connection(vec![test_tool("second")], second_closed.clone()).await,
        ),
    ]);
    let runtime = McpRuntime::new(connector).handle();
    let servers = BTreeMap::from([("docs".to_string(), config("docs", None))]);
    runtime.reconcile(servers.clone()).await.expect("first");
    let first = runtime.acquire_turn_lease().await.expect("first lease");
    runtime.recheck(servers).await.expect("second");
    let second = runtime.acquire_turn_lease().await.expect("second lease");
    assert_eq!(first.tools()[0].raw_name, "first");
    assert_eq!(second.tools()[0].raw_name, "second");
    assert!(!first_closed.load(Ordering::SeqCst));

    drop(first);
    wait_for_closed(&first_closed).await;
    assert!(!second_closed.load(Ordering::SeqCst));
    drop(second);
    runtime.shutdown().await;
    wait_for_closed(&second_closed).await;
}

#[tokio::test]
async fn resource_facades_use_rmcp_typed_resource_api() {
    let closed = Arc::new(AtomicBool::new(false));
    let connector = McpConnector::testing([(
        "docs".to_string(),
        test_connection(vec![test_tool("lookup")], closed.clone()).await,
    )]);
    let runtime = McpRuntime::new(connector).handle();
    runtime
        .reconcile(BTreeMap::from([("docs".to_string(), config("docs", None))]))
        .await
        .expect("reconcile");
    let lease = runtime.acquire_turn_lease().await.expect("lease");
    let mut core = crate::TurnEngine::default_provider().expect("core");
    lease.install(&mut core).expect("install lease");
    let output = core
        .registered_tool("read_mcp_resource")
        .expect("resource façade")
        .execute(
            tool_input(json!({ "server": "docs", "uri": "mcp://docs/one" })),
            test_context(),
        )
        .await
        .expect("read resource");
    let value: Value = serde_json::from_str(&output.description).expect("resource JSON");
    assert_eq!(value["contents"][0]["text"], "resource body");
    drop(lease);
    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

fn config(server_id: &str, effect: Option<ToolEffect>) -> EffectiveMcpServerConfig {
    EffectiveMcpServerConfig {
        id: server_id.to_string(),
        config: McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            url: Some("http://test.invalid/mcp".to_string()),
            ..McpServerConfig::default()
        },
        source_kind: McpServerSourceKind::User,
        source_label: "test".to_string(),
        source_detail: None,
        status_kind: McpServerStatusKind::Enabled,
        status_message: None,
        mutation_policy: McpServerMutationPolicy::UserEditable,
        bearer_token: None,
        tool_effect: effect,
    }
}

fn json_object(value: Value) -> Map<String, Value> {
    value.as_object().expect("JSON object").clone()
}

fn tool_input(arguments: Value) -> ToolInput {
    ToolInput {
        arguments,
        session_id: "session".to_string(),
        tool_id: "tool".to_string(),
        revision_base: 0,
    }
}

fn test_context() -> ToolContext {
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    ToolContext {
        event_tx,
        options: crate::TurnOptions::default(),
        workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
        workspace: AgentWorkspace::local(std::env::temp_dir()),
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        lsp_runtime: None,
        parent_session: Arc::new(crate::AgentSession::new()),
        working_set: crate::TurnWorkingSetHandle::default(),
        tool_cache: crate::TurnToolCacheHandle::default(),
    }
}

fn audit_metadata(events: &[ToolRuntimeEvent]) -> &Value {
    events
        .iter()
        .find_map(|event| match event {
            ToolRuntimeEvent::AuditMetadata { metadata } => Some(metadata),
            _ => None,
        })
        .expect("MCP audit metadata")
}

async fn wait_for_closed(closed: &AtomicBool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !closed.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("MCP service closed");
}
