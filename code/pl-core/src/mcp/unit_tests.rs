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

use super::{ConnectedMcp, McpConnector, McpResetScope, McpRuntime};
use crate::config::*;
use crate::tool::*;
use crate::turn::ToolEffect;

pub(crate) struct McpTestHarness {
    runtime: super::McpRuntimeHandle,
    closed: Arc<AtomicBool>,
}

impl McpTestHarness {
    /// 发布一个带只读 annotation 的 MCP server 到共享注册表并挂接到 engine。
    pub(crate) async fn install_read_tool(core: &mut crate::TurnEngine) -> Self {
        let closed = Arc::new(AtomicBool::new(false));
        let connector = McpConnector::testing([(
            "docs".to_string(),
            test_connection(vec![test_tool("lookup")], closed.clone()).await,
        )]);
        let shared = Arc::new(ToolRegistry::new());
        let runtime = McpRuntime::new(connector, Some(shared.clone())).handle();
        runtime
            .reconcile(BTreeMap::from([(
                "docs".to_string(),
                config("docs", Some(ToolEffect::Read)),
            )]))
            .await
            .expect("reconcile test MCP runtime");
        core.set_shared_tool_registry(shared);
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
    annotated_tool(name, ToolAnnotations::new().read_only(true))
}

fn annotated_tool(name: &str, annotations: ToolAnnotations) -> rmcp::model::Tool {
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
    tool.annotations = Some(annotations);
    tool.icons = Some(vec![Icon::new("data:image/png;base64,AA==")]);
    tool.meta = Some(MetaObject(Map::from_iter([(
        "displayGroup".to_string(),
        json!("tests"),
    )])));
    tool
}

fn plain_tool(name: &str) -> rmcp::model::Tool {
    rmcp::model::Tool::new(
        name.to_string(),
        format!("{name} description"),
        json_object(json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })),
    )
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

async fn shared_runtime(
    tools: Vec<rmcp::model::Tool>,
    effect: Option<ToolEffect>,
) -> (Arc<ToolRegistry>, super::McpRuntimeHandle, Arc<AtomicBool>) {
    let closed = Arc::new(AtomicBool::new(false));
    let connector = McpConnector::testing([(
        "docs".to_string(),
        test_connection(tools, closed.clone()).await,
    )]);
    let shared = Arc::new(ToolRegistry::new());
    let runtime = McpRuntime::new(connector, Some(shared.clone())).handle();
    runtime
        .reconcile(BTreeMap::from([(
            "docs".to_string(),
            config("docs", effect),
        )]))
        .await
        .expect("reconcile test MCP runtime");
    (shared, runtime, closed)
}

#[tokio::test]
async fn shared_registry_publishes_tools_with_metadata_and_audit_pipeline() {
    let (shared, runtime, closed) = shared_runtime(vec![test_tool("lookup")], None).await;

    let snapshot = shared.snapshot();
    let entry = snapshot
        .entry("mcp__docs__lookup")
        .expect("published MCP tool");
    assert_eq!(entry.metadata().source, ToolSourceId::mcp());
    let namespace = entry.metadata().namespace.as_ref().expect("namespace");
    assert_eq!(namespace.name, "mcp_docs");
    assert!(namespace.description.contains("docs"));
    // 无显式配置时 readOnlyHint=true 推导 Read；推导出的 Read 工具可 programmatic。
    assert_eq!(entry.tool().effect(), Some(ToolEffect::Read));
    assert!(entry.metadata().programmatic_eligible);

    let tool = entry.tool();
    assert!(tool.supports_parallel_tool_calls());
    assert_eq!(tool.runtime_lock_policy(), ToolRuntimeLockPolicy::Shared);
    assert_eq!(
        tool.cache_policy(&json!({})),
        crate::tool::cache::ToolCachePolicy::Never
    );
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

    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn untrusted_annotations_keep_conservative_defaults() {
    // 无显式配置且只有 destructiveHint：不映射写 effect，保持 None（保守）。
    let (shared, runtime, closed) = shared_runtime(
        vec![annotated_tool(
            "destructive",
            ToolAnnotations::new().destructive(true),
        )],
        None,
    )
    .await;

    let snapshot = shared.snapshot();
    let entry = snapshot
        .entry("mcp__docs__destructive")
        .expect("published MCP tool");
    assert_eq!(entry.tool().effect(), None);
    assert!(!entry.metadata().programmatic_eligible);
    assert!(!entry.tool().supports_parallel_tool_calls());
    assert_eq!(
        entry.tool().runtime_lock_policy(),
        ToolRuntimeLockPolicy::Exclusive
    );

    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn explicit_server_effect_overrides_remote_hints() {
    // 无 annotation 的工具 + 显式配置 Read：优先配置。
    let (shared, runtime, closed) =
        shared_runtime(vec![plain_tool("configured")], Some(ToolEffect::Read)).await;

    let snapshot = shared.snapshot();
    let entry = snapshot
        .entry("mcp__docs__configured")
        .expect("published MCP tool");
    assert_eq!(entry.tool().effect(), Some(ToolEffect::Read));
    assert!(entry.metadata().programmatic_eligible);

    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn mcp_error_result_keeps_structured_audit_and_failed_terminal_marker() {
    let (shared, runtime, closed) = shared_runtime(vec![test_tool("fail")], None).await;

    let snapshot = shared.snapshot();
    let output = snapshot
        .entry("mcp__docs__fail")
        .expect("published MCP tool")
        .tool()
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

    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn resource_facades_are_published_and_use_rmcp_typed_resource_api() {
    let (shared, runtime, closed) = shared_runtime(vec![test_tool("lookup")], None).await;

    let snapshot = shared.snapshot();
    let names = snapshot.names();
    for facade in [
        "list_mcp_resources",
        "list_mcp_resource_templates",
        "read_mcp_resource",
    ] {
        assert!(names.contains(&facade), "missing {facade} in {names:?}");
    }
    let facade = snapshot
        .entry("read_mcp_resource")
        .expect("resource façade");
    assert!(facade.metadata().namespace.is_none());
    assert!(facade.metadata().programmatic_eligible);

    let output = facade
        .tool()
        .execute(
            tool_input(json!({ "server": "docs", "uri": "mcp://docs/one" })),
            test_context(),
        )
        .await
        .expect("read resource");
    let value: Value = serde_json::from_str(&output.description).expect("resource JSON");
    assert_eq!(value["contents"][0]["text"], "resource body");

    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn generation_replacement_republishes_the_shared_registry() {
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
    let shared = Arc::new(ToolRegistry::new());
    let runtime = McpRuntime::new(connector, Some(shared.clone())).handle();
    let servers = BTreeMap::from([("docs".to_string(), config("docs", None))]);
    runtime.reconcile(servers.clone()).await.expect("first");

    let first = shared.snapshot();
    assert!(first.entry("mcp__docs__first").is_some());
    assert!(first.entry("mcp__docs__second").is_none());
    let revision_after_first = first.revision.0;

    runtime
        .reset(McpResetScope::All, servers)
        .await
        .expect("second");
    let second = shared.snapshot();
    assert!(second.entry("mcp__docs__second").is_some());
    assert!(second.entry("mcp__docs__first").is_none());
    assert!(second.revision.0 > revision_after_first);

    runtime.shutdown().await;
    wait_for_closed(&first_closed).await;
    wait_for_closed(&second_closed).await;
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
    let runtime = McpRuntime::new(connector, None).handle();
    let servers = BTreeMap::from([("docs".to_string(), config("docs", None))]);
    runtime.reconcile(servers.clone()).await.expect("first");
    let first = runtime.acquire_turn_lease().await.expect("first lease");
    runtime
        .reset(McpResetScope::All, servers)
        .await
        .expect("second");
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
        tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
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
